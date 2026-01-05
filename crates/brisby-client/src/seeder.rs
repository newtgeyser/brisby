//! Seeder module for serving files to other peers
//!
//! Handles storing chunks locally and responding to chunk requests over Nym.

use anyhow::Result;
use brisby_core::proto::{self, Envelope, Payload};
use brisby_core::{chunk::chunk_file, ContentHash, FileMetadata, ReceivedMessage, SenderTag, Transport};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Chunk storage for seeding files
pub struct ChunkStore {
    /// Base directory for chunk storage
    storage_dir: PathBuf,
    /// In-memory cache of file metadata
    metadata: HashMap<ContentHash, FileMetadata>,
    /// In-memory chunk cache (content_hash -> chunk_index -> chunk_data)
    chunks: HashMap<ContentHash, HashMap<u32, Vec<u8>>>,
}

impl ChunkStore {
    /// Create a new chunk store
    pub fn new(storage_dir: PathBuf) -> Self {
        Self {
            storage_dir,
            metadata: HashMap::new(),
            chunks: HashMap::new(),
        }
    }

    /// Add a file to the store
    pub fn add_file(&mut self, path: &Path) -> Result<FileMetadata> {
        // Chunk the file
        let (metadata, chunks) = chunk_file(path)?;

        // Store chunks in memory
        let mut chunk_map = HashMap::new();
        for (index, chunk) in chunks.into_iter().enumerate() {
            chunk_map.insert(index as u32, chunk);
        }

        self.chunks.insert(metadata.content_hash, chunk_map);
        self.metadata.insert(metadata.content_hash, metadata.clone());

        // Also persist chunks to disk for durability
        let file_dir = self.storage_dir.join(brisby_core::hash_to_hex(&metadata.content_hash));
        std::fs::create_dir_all(&file_dir)?;

        // Save metadata
        let metadata_path = file_dir.join("metadata.json");
        let metadata_json = serde_json::to_string_pretty(&metadata)?;
        std::fs::write(&metadata_path, metadata_json)?;

        // Save chunks
        if let Some(chunks) = self.chunks.get(&metadata.content_hash) {
            for (index, data) in chunks {
                let chunk_path = file_dir.join(format!("chunk_{:06}", index));
                std::fs::write(&chunk_path, data)?;
            }
        }

        tracing::info!(
            "Added file {} ({} chunks)",
            metadata.filename,
            metadata.chunks.len()
        );

        Ok(metadata)
    }

    /// Load a file's chunks from disk
    pub fn load_file(&mut self, content_hash: &ContentHash) -> Result<bool> {
        let file_dir = self.storage_dir.join(brisby_core::hash_to_hex(content_hash));
        let metadata_path = file_dir.join("metadata.json");

        if !metadata_path.exists() {
            return Ok(false);
        }

        // Load metadata
        let metadata_json = std::fs::read_to_string(&metadata_path)?;
        let metadata: FileMetadata = serde_json::from_str(&metadata_json)?;

        // Load chunks
        let mut chunk_map = HashMap::new();
        for i in 0..metadata.chunks.len() {
            let chunk_path = file_dir.join(format!("chunk_{:06}", i));
            if chunk_path.exists() {
                let data = std::fs::read(&chunk_path)?;
                chunk_map.insert(i as u32, data);
            }
        }

        self.chunks.insert(*content_hash, chunk_map);
        self.metadata.insert(*content_hash, metadata);

        Ok(true)
    }

    /// Load all files from storage directory
    pub fn load_all(&mut self) -> Result<usize> {
        if !self.storage_dir.exists() {
            std::fs::create_dir_all(&self.storage_dir)?;
            return Ok(0);
        }

        let mut count = 0;
        for entry in std::fs::read_dir(&self.storage_dir)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                if let Ok(hash) = brisby_core::hex_to_hash(&name_str) {
                    if self.load_file(&hash)? {
                        count += 1;
                    }
                }
            }
        }

        Ok(count)
    }

    /// Get a chunk
    pub fn get_chunk(&self, content_hash: &ContentHash, chunk_index: u32) -> Option<&Vec<u8>> {
        self.chunks
            .get(content_hash)
            .and_then(|chunks| chunks.get(&chunk_index))
    }

    /// Get metadata for a file
    pub fn get_metadata(&self, content_hash: &ContentHash) -> Option<&FileMetadata> {
        self.metadata.get(content_hash)
    }

    /// List all stored files
    pub fn list_files(&self) -> Vec<&FileMetadata> {
        self.metadata.values().collect()
    }
}

/// Seeder service that handles incoming chunk requests
pub struct Seeder {
    store: Arc<RwLock<ChunkStore>>,
}

impl Seeder {
    /// Create a new seeder
    pub fn new(store: ChunkStore) -> Self {
        Self {
            store: Arc::new(RwLock::new(store)),
        }
    }

    /// Get access to the chunk store
    pub fn store(&self) -> &Arc<RwLock<ChunkStore>> {
        &self.store
    }

    /// Handle an incoming message
    pub async fn handle_message(&self, msg: &ReceivedMessage) -> Option<(SenderTag, Vec<u8>)> {
        let sender_tag = msg.sender_tag.as_ref()?;

        let envelope = match Envelope::from_bytes(&msg.data) {
            Ok(env) => env,
            Err(e) => {
                tracing::warn!("Failed to decode message: {}", e);
                let response = proto::error_response(
                    0,
                    proto::error_codes::INVALID_MESSAGE,
                    format!("decode error: {}", e),
                );
                return Some((sender_tag.clone(), response.to_bytes()));
            }
        };

        let request_id = envelope.request_id;
        let response = match envelope.payload {
            Some(Payload::ChunkRequest(req)) => {
                self.handle_chunk_request(request_id, req).await
            }
            Some(Payload::PingRequest(_)) => {
                proto::Envelope::new(
                    request_id,
                    Payload::PingResponse(proto::PingResponse {
                        responder_id: vec![], // Empty for now, could use node ID
                    }),
                )
            }
            Some(other) => {
                tracing::warn!("Unexpected message type: {:?}", other);
                proto::error_response(
                    request_id,
                    proto::error_codes::INVALID_MESSAGE,
                    "unexpected message type".to_string(),
                )
            }
            None => {
                proto::error_response(
                    request_id,
                    proto::error_codes::INVALID_MESSAGE,
                    "empty payload".to_string(),
                )
            }
        };

        Some((sender_tag.clone(), response.to_bytes()))
    }

    /// Handle a chunk request
    async fn handle_chunk_request(
        &self,
        request_id: u64,
        req: proto::ChunkRequest,
    ) -> Envelope {
        // Validate content hash
        if req.content_hash.len() != 32 {
            return proto::error_response(
                request_id,
                proto::error_codes::INVALID_DATA,
                "invalid content hash length".to_string(),
            );
        }

        let mut content_hash = [0u8; 32];
        content_hash.copy_from_slice(&req.content_hash);

        tracing::info!(
            "Chunk request: {} chunk {}",
            &brisby_core::hash_to_hex(&content_hash)[..8],
            req.chunk_index
        );

        let store = self.store.read().await;

        // Get the chunk
        match store.get_chunk(&content_hash, req.chunk_index) {
            Some(data) => {
                // Compute chunk hash
                let chunk_hash = *blake3::hash(data).as_bytes();

                tracing::debug!(
                    "Sending chunk {} ({} bytes)",
                    req.chunk_index,
                    data.len()
                );

                Envelope::new(
                    request_id,
                    Payload::ChunkResponse(proto::ChunkResponse {
                        content_hash: content_hash.to_vec(),
                        chunk_index: req.chunk_index,
                        data: data.clone(),
                        chunk_hash: chunk_hash.to_vec(),
                    }),
                )
            }
            None => {
                tracing::warn!(
                    "Chunk not found: {} index {}",
                    &brisby_core::hash_to_hex(&content_hash)[..8],
                    req.chunk_index
                );
                proto::error_response(
                    request_id,
                    proto::error_codes::NOT_FOUND,
                    "chunk not found".to_string(),
                )
            }
        }
    }
}

/// Run the seeder message loop
pub async fn run_seeder_loop<T: Transport>(
    transport: &T,
    seeder: &Seeder,
) -> Result<()> {
    tracing::info!("Starting seeder message loop");

    loop {
        match transport.receive_timeout(std::time::Duration::from_secs(30)).await {
            Ok(Some(msg)) => {
                if let Some((sender_tag, response_bytes)) = seeder.handle_message(&msg).await {
                    if let Err(e) = transport.send_reply(&sender_tag, response_bytes).await {
                        tracing::error!("Failed to send reply: {}", e);
                    }
                }
            }
            Ok(None) => {
                // Timeout, continue
                tracing::debug!("No messages received in timeout period");
            }
            Err(e) => {
                tracing::error!("Error receiving message: {}", e);
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use brisby_core::transport::mock::MockTransport;
    use tempfile::{NamedTempFile, TempDir};
    use std::io::Write;

    #[test]
    fn test_chunk_store_add_file() {
        let temp_dir = TempDir::new().unwrap();
        let mut store = ChunkStore::new(temp_dir.path().join("chunks"));

        // Create a test file
        let mut test_file = NamedTempFile::new().unwrap();
        test_file.write_all(b"Hello, World! This is test data for chunking.").unwrap();
        test_file.flush().unwrap();

        let metadata = store.add_file(test_file.path()).unwrap();
        assert_eq!(metadata.filename, test_file.path().file_name().unwrap().to_string_lossy());
        assert_eq!(metadata.chunks.len(), 1); // Small file = 1 chunk

        // Verify chunk retrieval
        let chunk = store.get_chunk(&metadata.content_hash, 0);
        assert!(chunk.is_some());
        assert_eq!(chunk.unwrap(), b"Hello, World! This is test data for chunking.");
    }

    #[test]
    fn test_chunk_store_load_file() {
        let temp_dir = TempDir::new().unwrap();
        let storage_dir = temp_dir.path().join("chunks");

        let content_hash;
        {
            let mut store = ChunkStore::new(storage_dir.clone());

            let mut test_file = NamedTempFile::new().unwrap();
            test_file.write_all(b"Persistent test data").unwrap();
            test_file.flush().unwrap();

            let metadata = store.add_file(test_file.path()).unwrap();
            content_hash = metadata.content_hash;
        }

        // Create new store and load
        let mut store2 = ChunkStore::new(storage_dir);
        assert!(store2.load_file(&content_hash).unwrap());

        let chunk = store2.get_chunk(&content_hash, 0);
        assert!(chunk.is_some());
        assert_eq!(chunk.unwrap(), b"Persistent test data");
    }

    #[tokio::test]
    async fn test_seeder_handle_chunk_request() {
        let temp_dir = TempDir::new().unwrap();
        let mut store = ChunkStore::new(temp_dir.path().join("chunks"));

        let mut test_file = NamedTempFile::new().unwrap();
        test_file.write_all(b"Seeder test data").unwrap();
        test_file.flush().unwrap();

        let metadata = store.add_file(test_file.path()).unwrap();
        let seeder = Seeder::new(store);

        // Create a chunk request
        let request = Envelope::new(
            1,
            Payload::ChunkRequest(proto::ChunkRequest {
                content_hash: metadata.content_hash.to_vec(),
                chunk_index: 0,
                surb: vec![],
            }),
        );

        let msg = ReceivedMessage::new(
            request.to_bytes(),
            Some(SenderTag::new(vec![0u8; 16])),
        );

        let (_, response_bytes) = seeder.handle_message(&msg).await.unwrap();
        let response = Envelope::from_bytes(&response_bytes).unwrap();

        match response.payload {
            Some(Payload::ChunkResponse(resp)) => {
                assert_eq!(resp.chunk_index, 0);
                assert_eq!(resp.data, b"Seeder test data");
            }
            _ => panic!("Expected ChunkResponse"),
        }
    }
}
