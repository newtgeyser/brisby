//! Message handler for the index provider
//!
//! Processes incoming protocol messages and routes them to appropriate handlers.

use brisby_core::proto::{
    self, error_codes, Envelope, Payload, PublishRequest, PublishResponse, SearchRequest,
    SearchResponse, SearchResult as ProtoSearchResult,
};
use brisby_core::{IndexEntry, ReceivedMessage, SenderTag, Transport};

use crate::search::SearchIndex;

/// Handler for processing protocol messages
pub struct MessageHandler {
    index: SearchIndex,
}

impl MessageHandler {
    /// Create a new message handler
    pub fn new(index: SearchIndex) -> Self {
        Self { index }
    }

    /// Process an incoming message and return a response
    pub fn handle(&self, msg: &ReceivedMessage) -> Option<(SenderTag, Vec<u8>)> {
        // We need a sender_tag to reply
        let sender_tag = msg.sender_tag.as_ref()?;

        // Decode the envelope
        let envelope = match Envelope::from_bytes(&msg.data) {
            Ok(env) => env,
            Err(e) => {
                tracing::warn!("Failed to decode message: {}", e);
                let response = proto::error_response(
                    0,
                    error_codes::INVALID_MESSAGE,
                    format!("decode error: {}", e),
                );
                return Some((sender_tag.clone(), response.to_bytes()));
            }
        };

        let request_id = envelope.request_id;
        let response = match envelope.payload {
            Some(Payload::PublishRequest(req)) => self.handle_publish(request_id, req),
            Some(Payload::SearchRequest(req)) => self.handle_search(request_id, req),
            Some(other) => {
                tracing::warn!("Unexpected message type: {:?}", other);
                proto::error_response(
                    request_id,
                    error_codes::INVALID_MESSAGE,
                    "unexpected message type".to_string(),
                )
            }
            None => {
                tracing::warn!("Empty payload in message");
                proto::error_response(
                    request_id,
                    error_codes::INVALID_MESSAGE,
                    "empty payload".to_string(),
                )
            }
        };

        Some((sender_tag.clone(), response.to_bytes()))
    }

    /// Handle a publish request
    fn handle_publish(&self, request_id: u64, req: PublishRequest) -> Envelope {
        tracing::info!(
            "Publish request: {} ({} bytes, {} chunks)",
            req.filename,
            req.size,
            req.chunk_count
        );

        // Validate content hash
        if req.content_hash.len() != 32 {
            return proto::error_response(
                request_id,
                error_codes::INVALID_DATA,
                "invalid content hash length".to_string(),
            );
        }

        let mut content_hash = [0u8; 32];
        content_hash.copy_from_slice(&req.content_hash);

        // Create index entry
        let entry = IndexEntry {
            content_hash,
            filename: req.filename.clone(),
            keywords: req.keywords.clone(),
            size: req.size,
            chunk_count: req.chunk_count,
            published_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            ttl: 3600 * 24, // 24 hour default TTL
        };

        // Store in index
        match self.index.upsert(&entry, &req.nym_address) {
            Ok(()) => {
                tracing::info!("Published: {}", brisby_core::hash_to_hex(&content_hash));
                Envelope::new(
                    request_id,
                    Payload::PublishResponse(PublishResponse {
                        success: true,
                        error: String::new(),
                    }),
                )
            }
            Err(e) => {
                tracing::error!("Failed to store entry: {}", e);
                Envelope::new(
                    request_id,
                    Payload::PublishResponse(PublishResponse {
                        success: false,
                        error: format!("storage error: {}", e),
                    }),
                )
            }
        }
    }

    /// Handle a search request
    fn handle_search(&self, request_id: u64, req: SearchRequest) -> Envelope {
        tracing::info!("Search request: '{}' (max {})", req.query, req.max_results);

        let max_results = if req.max_results == 0 || req.max_results > 100 {
            100
        } else {
            req.max_results
        };

        match self.index.search(&req.query, max_results) {
            Ok(results) => {
                tracing::info!("Found {} results", results.len());

                let proto_results: Vec<ProtoSearchResult> = results
                    .into_iter()
                    .map(|r| ProtoSearchResult {
                        content_hash: r.content_hash.to_vec(),
                        filename: r.filename,
                        size: r.size,
                        chunk_count: r.chunk_count,
                        relevance: r.relevance,
                        seeders: r.seeders,
                    })
                    .collect();

                Envelope::new(
                    request_id,
                    Payload::SearchResponse(SearchResponse {
                        results: proto_results,
                    }),
                )
            }
            Err(e) => {
                tracing::error!("Search failed: {}", e);
                proto::error_response(
                    request_id,
                    error_codes::UNAVAILABLE,
                    format!("search error: {}", e),
                )
            }
        }
    }
}

/// Run the index provider message loop
pub async fn run_message_loop<T: Transport>(
    transport: &T,
    handler: &MessageHandler,
) -> brisby_core::Result<()> {
    tracing::info!("Starting message loop");

    loop {
        // Wait for incoming message
        match transport.receive_timeout(std::time::Duration::from_secs(30)).await {
            Ok(Some(msg)) => {
                if let Some((sender_tag, response_bytes)) = handler.handle(&msg) {
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
                // Brief sleep before retrying
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use brisby_core::proto;
    use brisby_core::transport::mock::MockTransport;
    use tempfile::NamedTempFile;

    fn setup_handler() -> (MessageHandler, NamedTempFile) {
        let temp = NamedTempFile::new().unwrap();
        let index = SearchIndex::open(temp.path()).unwrap();
        (MessageHandler::new(index), temp)
    }

    #[test]
    fn test_handle_publish() {
        let (handler, _temp) = setup_handler();

        let request = proto::Envelope::new(
            1,
            proto::Payload::PublishRequest(proto::PublishRequest {
                content_hash: vec![1u8; 32],
                filename: "test.txt".to_string(),
                keywords: vec!["test".to_string()],
                size: 1024,
                chunk_count: 1,
                nym_address: "test-address".to_string(),
            }),
        );

        let msg = ReceivedMessage::new(
            request.to_bytes(),
            Some(SenderTag::new(vec![0u8; 16])),
        );

        let (_, response_bytes) = handler.handle(&msg).unwrap();
        let response = Envelope::from_bytes(&response_bytes).unwrap();

        match response.payload {
            Some(Payload::PublishResponse(resp)) => {
                assert!(resp.success);
            }
            _ => panic!("Expected PublishResponse"),
        }
    }

    #[test]
    fn test_handle_search() {
        let (handler, _temp) = setup_handler();

        // First publish something
        let entry = IndexEntry {
            content_hash: [1u8; 32],
            filename: "movie.mkv".to_string(),
            keywords: vec!["action".to_string(), "movie".to_string()],
            size: 1024 * 1024 * 100,
            chunk_count: 400,
            published_at: 1000,
            ttl: 3600,
        };
        handler.index.upsert(&entry, "test-address").unwrap();

        // Now search
        let request = proto::Envelope::new(
            2,
            proto::Payload::SearchRequest(proto::SearchRequest {
                query: "movie".to_string(),
                max_results: 10,
            }),
        );

        let msg = ReceivedMessage::new(
            request.to_bytes(),
            Some(SenderTag::new(vec![0u8; 16])),
        );

        let (_, response_bytes) = handler.handle(&msg).unwrap();
        let response = Envelope::from_bytes(&response_bytes).unwrap();

        match response.payload {
            Some(Payload::SearchResponse(resp)) => {
                assert_eq!(resp.results.len(), 1);
                assert_eq!(resp.results[0].filename, "movie.mkv");
            }
            _ => panic!("Expected SearchResponse"),
        }
    }

    #[tokio::test]
    async fn test_message_loop_with_mock() {
        let (handler, _temp) = setup_handler();
        let mut transport = MockTransport::new();
        transport.connect().await.unwrap();

        // Queue a search request
        let request = proto::Envelope::new(
            1,
            proto::Payload::SearchRequest(proto::SearchRequest {
                query: "test".to_string(),
                max_results: 10,
            }),
        );
        transport.queue_message(ReceivedMessage::new(
            request.to_bytes(),
            Some(SenderTag::new(vec![0u8; 16])),
        ));

        // Run with timeout - should process the message and then timeout
        let result = tokio::time::timeout(
            std::time::Duration::from_millis(200),
            run_message_loop(&transport, &handler),
        )
        .await;

        // Should timeout (message loop runs forever)
        assert!(result.is_err());

        // But should have sent a reply
        let replies = transport.get_sent_replies();
        assert_eq!(replies.len(), 1);
    }
}
