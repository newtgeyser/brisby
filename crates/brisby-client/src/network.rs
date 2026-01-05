//! Network module for Nym communication
//!
//! Handles connecting to the Nym mixnet and communicating with index providers.

use anyhow::{anyhow, Result};
use brisby_core::proto::{self, Envelope, Payload};
use brisby_core::{NymAddress, Transport};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::LazyLock;
use std::time::Duration;

/// Request ID counter, initialized with a random offset to avoid collisions across sessions
static REQUEST_COUNTER: LazyLock<AtomicU64> = LazyLock::new(|| {
    let mut buf = [0u8; 8];
    // If getrandom fails, use current time as fallback
    if getrandom::getrandom(&mut buf).is_err() {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        return AtomicU64::new(ts);
    }
    AtomicU64::new(u64::from_le_bytes(buf))
});

/// Get a unique request ID
pub fn next_request_id() -> u64 {
    REQUEST_COUNTER.fetch_add(1, Ordering::SeqCst)
}

/// Search for files on an index provider
pub async fn search_index_provider<T: Transport>(
    transport: &T,
    index_provider: &NymAddress,
    query: &str,
    max_results: u32,
) -> Result<Vec<brisby_core::SearchResult>> {
    let request_id = next_request_id();

    // Create search request
    let envelope = proto::search_request(request_id, query.to_string(), max_results);

    tracing::debug!("Sending search request to {}", index_provider.as_str());

    // Send request
    transport
        .send(index_provider, envelope.to_bytes())
        .await
        .map_err(|e| anyhow!("Failed to send search request: {}", e))?;

    // Wait for response with timeout
    let timeout = Duration::from_secs(30);
    let response = transport
        .receive_timeout(timeout)
        .await
        .map_err(|e| anyhow!("Failed to receive response: {}", e))?
        .ok_or_else(|| anyhow!("Timeout waiting for search response"))?;

    // Decode response
    let envelope = Envelope::from_bytes(&response.data)
        .map_err(|e| anyhow!("Failed to decode response: {}", e))?;

    // Verify request ID matches
    if envelope.request_id != request_id {
        tracing::warn!(
            "Request ID mismatch: expected {}, got {}",
            request_id,
            envelope.request_id
        );
    }

    // Process response
    match envelope.payload {
        Some(Payload::SearchResponse(resp)) => {
            let results: Vec<brisby_core::SearchResult> = resp
                .results
                .into_iter()
                .filter_map(|r| {
                    if r.content_hash.len() != 32 {
                        return None;
                    }
                    let mut hash = [0u8; 32];
                    hash.copy_from_slice(&r.content_hash);
                    Some(brisby_core::SearchResult {
                        content_hash: hash,
                        filename: r.filename,
                        size: r.size,
                        chunk_count: r.chunk_count,
                        relevance: r.relevance,
                        seeders: r.seeders,
                    })
                })
                .collect();
            Ok(results)
        }
        Some(Payload::ErrorResponse(err)) => {
            Err(anyhow!("Index provider error: {} (code {})", err.message, err.code))
        }
        _ => Err(anyhow!("Unexpected response type")),
    }
}

/// Publish file metadata to an index provider
pub async fn publish_to_index_provider<T: Transport>(
    transport: &T,
    index_provider: &NymAddress,
    metadata: &brisby_core::FileMetadata,
    our_address: &NymAddress,
) -> Result<()> {
    let request_id = next_request_id();

    // Create publish request
    let envelope = Envelope::new(
        request_id,
        Payload::PublishRequest(proto::PublishRequest {
            content_hash: metadata.content_hash.to_vec(),
            filename: metadata.filename.clone(),
            keywords: metadata.keywords.clone(),
            size: metadata.size,
            chunk_count: metadata.chunks.len() as u32,
            nym_address: our_address.as_str().to_string(),
        }),
    );

    tracing::debug!("Sending publish request to {}", index_provider.as_str());

    // Send request
    transport
        .send(index_provider, envelope.to_bytes())
        .await
        .map_err(|e| anyhow!("Failed to send publish request: {}", e))?;

    // Wait for response with timeout
    let timeout = Duration::from_secs(30);
    let response = transport
        .receive_timeout(timeout)
        .await
        .map_err(|e| anyhow!("Failed to receive response: {}", e))?
        .ok_or_else(|| anyhow!("Timeout waiting for publish response"))?;

    // Decode response
    let envelope = Envelope::from_bytes(&response.data)
        .map_err(|e| anyhow!("Failed to decode response: {}", e))?;

    // Process response
    match envelope.payload {
        Some(Payload::PublishResponse(resp)) => {
            if resp.success {
                Ok(())
            } else {
                Err(anyhow!("Publish failed: {}", resp.error))
            }
        }
        Some(Payload::ErrorResponse(err)) => {
            Err(anyhow!("Index provider error: {} (code {})", err.message, err.code))
        }
        _ => Err(anyhow!("Unexpected response type")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use brisby_core::transport::mock::MockTransport;
    use brisby_core::{proto, ReceivedMessage, SenderTag};

    #[tokio::test]
    async fn test_search_index_provider() {
        let mut transport = MockTransport::new();
        transport.connect().await.unwrap();

        let index_provider = NymAddress::new("test-index-provider");

        // Queue a search response (request_id mismatch is logged but doesn't fail)
        let response = proto::search_response(
            0, // Doesn't need to match - mismatch is just logged
            vec![proto::SearchResult {
                content_hash: vec![1u8; 32],
                filename: "test.txt".to_string(),
                size: 1024,
                chunk_count: 1,
                relevance: 1.0,
                seeders: vec!["test-seeder".to_string()],
            }],
        );
        transport.queue_message(ReceivedMessage::new(response.to_bytes(), None));

        let results = search_index_provider(&transport, &index_provider, "test", 10)
            .await
            .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].filename, "test.txt");
    }
}
