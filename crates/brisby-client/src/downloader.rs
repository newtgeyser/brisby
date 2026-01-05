//! File downloader module
//!
//! Handles downloading files chunk by chunk from seeders via the Nym network.

use anyhow::{anyhow, Result};
use brisby_core::proto::{self, Envelope, Payload};
use brisby_core::{chunk::verify_chunk, ContentHash, FileMetadata, NymAddress, Transport};
use std::collections::HashMap;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

/// Download state for tracking progress
#[derive(Debug, Clone)]
pub struct DownloadState {
    /// Content hash we're downloading
    pub content_hash: ContentHash,
    /// Expected total chunks
    pub total_chunks: u32,
    /// Chunks we've received
    pub received_chunks: HashMap<u32, Vec<u8>>,
    /// Seeders we know about
    pub seeders: Vec<NymAddress>,
}

impl DownloadState {
    pub fn new(content_hash: ContentHash, total_chunks: u32) -> Self {
        Self {
            content_hash,
            total_chunks,
            received_chunks: HashMap::new(),
            seeders: Vec::new(),
        }
    }

    pub fn is_complete(&self) -> bool {
        self.received_chunks.len() as u32 == self.total_chunks
    }

    pub fn progress(&self) -> f64 {
        if self.total_chunks == 0 {
            return 0.0;
        }
        (self.received_chunks.len() as f64 / self.total_chunks as f64) * 100.0
    }

    pub fn missing_chunks(&self) -> Vec<u32> {
        (0..self.total_chunks)
            .filter(|i| !self.received_chunks.contains_key(i))
            .collect()
    }
}

/// Downloader for fetching files from the network
pub struct Downloader<'a, T: Transport> {
    transport: &'a T,
    request_counter: AtomicU64,
}

impl<'a, T: Transport> Downloader<'a, T> {
    /// Create a new downloader
    pub fn new(transport: &'a T) -> Self {
        Self {
            transport,
            request_counter: AtomicU64::new(1),
        }
    }

    /// Get a unique request ID
    fn next_request_id(&self) -> u64 {
        self.request_counter.fetch_add(1, Ordering::SeqCst)
    }

    /// Request a specific chunk from a seeder
    pub async fn request_chunk(
        &self,
        seeder: &NymAddress,
        content_hash: &ContentHash,
        chunk_index: u32,
    ) -> Result<()> {
        let request_id = self.next_request_id();

        // Create SURB placeholder - in real implementation, we'd use Nym's SURB system
        // For now we use an empty SURB since we're doing request-response pattern
        let surb = Vec::new();

        let envelope = proto::chunk_request(
            request_id,
            content_hash.to_vec(),
            chunk_index,
            surb,
        );

        self.transport
            .send(seeder, envelope.to_bytes())
            .await
            .map_err(|e| anyhow!("Failed to send chunk request: {}", e))?;

        tracing::debug!(
            "Requested chunk {} from {}",
            chunk_index,
            seeder.as_str()
        );

        Ok(())
    }

    /// Wait for and process a chunk response
    pub async fn receive_chunk(
        &self,
        timeout: std::time::Duration,
    ) -> Result<Option<(u32, Vec<u8>, ContentHash)>> {
        match self.transport.receive_timeout(timeout).await {
            Ok(Some(msg)) => {
                let envelope = Envelope::from_bytes(&msg.data)
                    .map_err(|e| anyhow!("Failed to decode response: {}", e))?;

                match envelope.payload {
                    Some(Payload::ChunkResponse(resp)) => {
                        // Verify chunk hash
                        if resp.chunk_hash.len() != 32 {
                            return Err(anyhow!("Invalid chunk hash length"));
                        }
                        let mut expected_hash = [0u8; 32];
                        expected_hash.copy_from_slice(&resp.chunk_hash);

                        if !verify_chunk(&resp.data, &expected_hash) {
                            return Err(anyhow!("Chunk hash verification failed"));
                        }

                        // Convert content hash
                        if resp.content_hash.len() != 32 {
                            return Err(anyhow!("Invalid content hash length"));
                        }
                        let mut content_hash = [0u8; 32];
                        content_hash.copy_from_slice(&resp.content_hash);

                        Ok(Some((resp.chunk_index, resp.data, content_hash)))
                    }
                    Some(Payload::ErrorResponse(err)) => {
                        Err(anyhow!("Error from seeder: {} ({})", err.message, err.code))
                    }
                    _ => Err(anyhow!("Unexpected response type")),
                }
            }
            Ok(None) => Ok(None), // Timeout
            Err(e) => Err(anyhow!("Failed to receive: {}", e)),
        }
    }

    /// Download all chunks for a file sequentially
    pub async fn download_sequential(
        &self,
        metadata: &FileMetadata,
        seeders: &[NymAddress],
        progress_callback: impl Fn(u32, u32),
    ) -> Result<Vec<(u32, Vec<u8>)>> {
        if seeders.is_empty() {
            return Err(anyhow!("No seeders available"));
        }

        let mut chunks = Vec::new();
        let total_chunks = metadata.chunks.len() as u32;
        let timeout = std::time::Duration::from_secs(30);

        for chunk_idx in 0..total_chunks {
            progress_callback(chunk_idx, total_chunks);

            let mut received = false;

            // Try each seeder until we get the chunk
            for seeder in seeders {
                tracing::debug!("Requesting chunk {} from {}", chunk_idx, seeder.as_str());

                self.request_chunk(seeder, &metadata.content_hash, chunk_idx)
                    .await?;

                match self.receive_chunk(timeout).await {
                    Ok(Some((idx, data, hash))) => {
                        if idx == chunk_idx && hash == metadata.content_hash {
                            chunks.push((idx, data));
                            received = true;
                            break;
                        }
                    }
                    Ok(None) => {
                        tracing::warn!(
                            "Timeout waiting for chunk {} from {}",
                            chunk_idx,
                            seeder.as_str()
                        );
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Error receiving chunk {} from {}: {}",
                            chunk_idx,
                            seeder.as_str(),
                            e
                        );
                    }
                }
            }

            if !received {
                return Err(anyhow!(
                    "Failed to download chunk {} after trying all seeders",
                    chunk_idx
                ));
            }
        }

        progress_callback(total_chunks, total_chunks);
        Ok(chunks)
    }

    /// Reassemble chunks into the final file
    pub fn reassemble_to_file(
        &self,
        chunks: Vec<(u32, Vec<u8>)>,
        metadata: &FileMetadata,
        output_path: &Path,
    ) -> Result<()> {
        // Sort chunks by index
        let mut sorted: Vec<_> = chunks.into_iter().collect();
        sorted.sort_by_key(|(idx, _)| *idx);

        // Create output file
        let mut file = std::fs::File::create(output_path)?;

        // Write chunks in order
        let mut total_written = 0u64;
        for (idx, data) in sorted {
            tracing::trace!("Writing chunk {} ({} bytes)", idx, data.len());
            file.write_all(&data)?;
            total_written += data.len() as u64;
        }

        // Verify total size if the metadata included it
        if metadata.size != 0 && total_written != metadata.size {
            return Err(anyhow!(
                "Size mismatch: expected {} bytes, wrote {} bytes",
                metadata.size,
                total_written
            ));
        }

        // Verify final file hash
        file.sync_all()?;
        drop(file);

        let final_hash = {
            let data = std::fs::read(output_path)?;
            *blake3::hash(&data).as_bytes()
        };

        if final_hash != metadata.content_hash {
            std::fs::remove_file(output_path)?;
            return Err(anyhow!("Final file hash verification failed"));
        }

        tracing::info!(
            "Successfully downloaded and verified {} ({} bytes)",
            metadata.filename,
            metadata.size
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use brisby_core::transport::mock::MockTransport;

    #[test]
    fn test_download_state() {
        let mut state = DownloadState::new([1u8; 32], 5);
        assert!(!state.is_complete());
        assert_eq!(state.missing_chunks(), vec![0, 1, 2, 3, 4]);

        state.received_chunks.insert(0, vec![1, 2, 3]);
        state.received_chunks.insert(2, vec![4, 5, 6]);

        assert!(!state.is_complete());
        assert_eq!(state.missing_chunks(), vec![1, 3, 4]);
        assert!((state.progress() - 40.0).abs() < 0.1);

        state.received_chunks.insert(1, vec![7]);
        state.received_chunks.insert(3, vec![8]);
        state.received_chunks.insert(4, vec![9]);

        assert!(state.is_complete());
        assert!((state.progress() - 100.0).abs() < 0.1);
    }

    #[tokio::test]
    async fn test_downloader_request() {
        let mut transport = MockTransport::new();
        transport.connect().await.unwrap();

        let downloader = Downloader::new(&transport);
        let seeder = NymAddress::new("seeder-address");
        let content_hash = [1u8; 32];

        // Should not error when sending request
        let result = downloader.request_chunk(&seeder, &content_hash, 0).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_reassemble_allows_unknown_sizes() {
        let mut transport = MockTransport::new();
        transport.connect().await.unwrap();
        let downloader = Downloader::new(&transport);

        let data = b"short-file";
        let content_hash = *blake3::hash(data).as_bytes();
        let metadata = FileMetadata {
            content_hash,
            filename: "short.txt".to_string(),
            size: 0, // unknown total size
            mime_type: None,
            chunks: vec![brisby_core::ChunkInfo {
                index: 0,
                hash: content_hash, // not used in reassemble_to_file, but provide something consistent
                size: 0, // unknown chunk size
            }],
            keywords: vec![],
            created_at: 0,
        };

        let output = tempfile::NamedTempFile::new().unwrap();
        downloader
            .reassemble_to_file(vec![(0, data.to_vec())], &metadata, output.path())
            .unwrap();

        let written = std::fs::read(output.path()).unwrap();
        assert_eq!(written, data);
    }
}
