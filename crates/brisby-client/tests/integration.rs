//! Integration tests for the Brisby client
//!
//! These tests use the mock transport to verify the full flow without
//! requiring a real Nym network connection.

use brisby_core::proto::{self, Envelope, Payload};
use brisby_core::transport::mock::MockTransport;
use brisby_core::{ContentHash, ReceivedMessage, Transport};
use tempfile::TempDir;

/// Test the full publish -> search -> download flow using mock transport
#[tokio::test]
async fn test_full_flow_mock() {
    // Setup
    let temp_dir = TempDir::new().unwrap();
    let test_content = b"Hello, Brisby! This is integration test content.";
    let test_file = temp_dir.path().join("test.txt");
    std::fs::write(&test_file, test_content).unwrap();

    // 1. Chunk the file (simulating share)
    let (metadata, chunks) = brisby_core::chunk::chunk_file(&test_file).unwrap();
    assert_eq!(metadata.filename, "test.txt");
    assert_eq!(chunks.len(), 1); // Small file = 1 chunk

    // 2. Create mock index provider
    let mut index_transport = MockTransport::new();
    index_transport.connect().await.unwrap();

    // 3. Simulate publishing to index
    let publish_request = proto::Envelope::new(
        1,
        proto::Payload::PublishRequest(proto::PublishRequest {
            content_hash: metadata.content_hash.to_vec(),
            filename: metadata.filename.clone(),
            keywords: metadata.keywords.clone(),
            size: metadata.size,
            chunk_count: metadata.chunks.len() as u32,
            nym_address: "test-seeder-address".to_string(),
        }),
    );

    // Verify the publish request is valid
    let decoded = Envelope::from_bytes(&publish_request.to_bytes()).unwrap();
    match decoded.payload {
        Some(Payload::PublishRequest(req)) => {
            assert_eq!(req.filename, "test.txt");
            assert_eq!(req.nym_address, "test-seeder-address");
        }
        _ => panic!("Expected PublishRequest"),
    }

    // 4. Simulate search response
    let search_response = proto::search_response(
        2,
        vec![proto::SearchResult {
            content_hash: metadata.content_hash.to_vec(),
            filename: metadata.filename.clone(),
            size: metadata.size,
            chunk_count: metadata.chunks.len() as u32,
            relevance: 1.0,
            seeders: vec!["test-seeder-address".to_string()],
        }],
    );

    // Queue the search response for the client to receive
    index_transport.queue_message(ReceivedMessage::new(search_response.to_bytes(), None));

    // 5. Simulate chunk request/response
    let chunk_request = proto::chunk_request(3, metadata.content_hash.to_vec(), 0, vec![]);

    // Verify chunk request
    let decoded = Envelope::from_bytes(&chunk_request.to_bytes()).unwrap();
    match decoded.payload {
        Some(Payload::ChunkRequest(req)) => {
            assert_eq!(req.chunk_index, 0);
            assert_eq!(req.content_hash, metadata.content_hash.to_vec());
        }
        _ => panic!("Expected ChunkRequest"),
    }

    // Create chunk response
    let chunk_hash = *blake3::hash(&chunks[0]).as_bytes();
    let chunk_response = Envelope::new(
        3,
        Payload::ChunkResponse(proto::ChunkResponse {
            content_hash: metadata.content_hash.to_vec(),
            chunk_index: 0,
            data: chunks[0].clone(),
            chunk_hash: chunk_hash.to_vec(),
        }),
    );

    // Verify chunk response contains correct data
    let decoded = Envelope::from_bytes(&chunk_response.to_bytes()).unwrap();
    match decoded.payload {
        Some(Payload::ChunkResponse(resp)) => {
            assert_eq!(resp.chunk_index, 0);
            assert_eq!(resp.data, test_content.to_vec());

            // Verify chunk hash
            let computed_hash = *blake3::hash(&resp.data).as_bytes();
            assert_eq!(resp.chunk_hash, computed_hash.to_vec());
        }
        _ => panic!("Expected ChunkResponse"),
    }

    // 6. Verify reassembly
    let reassembled = chunks.concat();
    assert_eq!(reassembled, test_content.to_vec());

    // 7. Verify content hash matches metadata
    // Note: content_hash is computed from chunk hashes, not raw content
    // This is verified by chunk_file() returning consistent metadata
    assert!(!metadata.content_hash.iter().all(|&b| b == 0));
}

/// Test chunk store persistence
#[test]
fn test_chunk_store_persistence() {
    let temp_dir = TempDir::new().unwrap();
    let chunks_dir = temp_dir.path().join("chunks");

    // Create test file
    let test_file = temp_dir.path().join("persist-test.txt");
    std::fs::write(&test_file, b"Persistent content").unwrap();

    let content_hash: ContentHash;

    // Add file to store
    {
        let mut store = brisby_client::seeder::ChunkStore::new(chunks_dir.clone());
        let metadata = store.add_file(&test_file).unwrap();
        content_hash = metadata.content_hash;

        // Verify chunk is accessible
        let chunk = store.get_chunk(&content_hash, 0).unwrap();
        assert_eq!(chunk, b"Persistent content");
    }

    // Create new store and load
    {
        let mut store = brisby_client::seeder::ChunkStore::new(chunks_dir);
        let loaded = store.load_all().unwrap();
        assert_eq!(loaded, 1);

        // Verify chunk is still accessible
        let chunk = store.get_chunk(&content_hash, 0).unwrap();
        assert_eq!(chunk, b"Persistent content");
    }
}

/// Test message encoding/decoding roundtrip
#[test]
fn test_message_roundtrip() {
    let messages = vec![
        proto::search_request(1, "test query".to_string(), 10),
        proto::search_response(
            2,
            vec![proto::SearchResult {
                content_hash: vec![1u8; 32],
                filename: "test.txt".to_string(),
                size: 1024,
                chunk_count: 4,
                relevance: 0.95,
                seeders: vec!["seeder1".to_string(), "seeder2".to_string()],
            }],
        ),
        proto::chunk_request(3, vec![2u8; 32], 5, vec![0u8; 16]),
        proto::Envelope::new(
            4,
            Payload::ChunkResponse(proto::ChunkResponse {
                content_hash: vec![3u8; 32],
                chunk_index: 2,
                data: vec![4u8; 100],
                chunk_hash: vec![5u8; 32],
            }),
        ),
        proto::error_response(5, 404, "Not found".to_string()),
    ];

    for original in messages {
        let bytes = original.to_bytes();
        let decoded = Envelope::from_bytes(&bytes).unwrap();

        assert_eq!(decoded.request_id, original.request_id);
        assert_eq!(decoded.version, original.version);
        // Payload comparison depends on type
    }
}

/// Test search result with multiple seeders
#[test]
fn test_search_result_seeders() {
    let result = brisby_core::SearchResult {
        content_hash: [1u8; 32],
        filename: "multi-seeder.txt".to_string(),
        size: 2048,
        chunk_count: 8,
        relevance: 0.8,
        seeders: vec![
            "seeder1.nym".to_string(),
            "seeder2.nym".to_string(),
            "seeder3.nym".to_string(),
        ],
    };

    assert_eq!(result.seeders.len(), 3);
    assert!(result.seeders.contains(&"seeder1.nym".to_string()));
}

/// Test file chunking with various sizes
#[test]
fn test_chunking_sizes() {
    let temp_dir = TempDir::new().unwrap();

    // Test small file (< chunk size)
    let small_file = temp_dir.path().join("small.txt");
    std::fs::write(&small_file, vec![0u8; 1000]).unwrap();
    let (meta, chunks) = brisby_core::chunk::chunk_file(&small_file).unwrap();
    assert_eq!(chunks.len(), 1);
    assert_eq!(meta.size, 1000);

    // Test file exactly at chunk boundary
    let boundary_file = temp_dir.path().join("boundary.txt");
    std::fs::write(&boundary_file, vec![0u8; brisby_core::CHUNK_SIZE]).unwrap();
    let (meta, chunks) = brisby_core::chunk::chunk_file(&boundary_file).unwrap();
    assert_eq!(chunks.len(), 1);
    assert_eq!(meta.size, brisby_core::CHUNK_SIZE as u64);

    // Test file slightly over chunk boundary
    let over_file = temp_dir.path().join("over.txt");
    std::fs::write(&over_file, vec![0u8; brisby_core::CHUNK_SIZE + 100]).unwrap();
    let (meta, chunks) = brisby_core::chunk::chunk_file(&over_file).unwrap();
    assert_eq!(chunks.len(), 2);
    assert_eq!(meta.size, (brisby_core::CHUNK_SIZE + 100) as u64);
}
