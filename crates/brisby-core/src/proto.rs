//! Protocol buffer message definitions
//!
//! These are manually defined to match the brisby.proto schema,
//! avoiding the need for protoc at build time.

use crate::{Error, Result, PROTOCOL_VERSION};
use prost::Message;

/// Message envelope wrapping all protocol messages
#[derive(Clone, PartialEq, Message)]
pub struct Envelope {
    /// Protocol version
    #[prost(uint32, tag = "1")]
    pub version: u32,
    /// Request ID for correlation
    #[prost(uint64, tag = "2")]
    pub request_id: u64,
    /// The actual message payload
    #[prost(oneof = "Payload", tags = "10, 11, 20, 21, 30, 31, 40, 41, 42, 43, 44, 45, 46, 47, 100")]
    pub payload: Option<Payload>,
}

/// Payload variants for the envelope
#[derive(Clone, PartialEq, prost::Oneof)]
pub enum Payload {
    #[prost(message, tag = "10")]
    SearchRequest(SearchRequest),
    #[prost(message, tag = "11")]
    SearchResponse(SearchResponse),
    #[prost(message, tag = "20")]
    ChunkRequest(ChunkRequest),
    #[prost(message, tag = "21")]
    ChunkResponse(ChunkResponse),
    #[prost(message, tag = "30")]
    PublishRequest(PublishRequest),
    #[prost(message, tag = "31")]
    PublishResponse(PublishResponse),
    #[prost(message, tag = "40")]
    FindNodeRequest(FindNodeRequest),
    #[prost(message, tag = "41")]
    FindNodeResponse(FindNodeResponse),
    #[prost(message, tag = "42")]
    FindValueRequest(FindValueRequest),
    #[prost(message, tag = "43")]
    FindValueResponse(FindValueResponse),
    #[prost(message, tag = "44")]
    StoreRequest(StoreRequest),
    #[prost(message, tag = "45")]
    StoreResponse(StoreResponse),
    #[prost(message, tag = "46")]
    PingRequest(PingRequest),
    #[prost(message, tag = "47")]
    PingResponse(PingResponse),
    #[prost(message, tag = "100")]
    ErrorResponse(ErrorResponse),
}

// Search messages

#[derive(Clone, PartialEq, Message)]
pub struct SearchRequest {
    #[prost(string, tag = "1")]
    pub query: String,
    #[prost(uint32, tag = "2")]
    pub max_results: u32,
}

#[derive(Clone, PartialEq, Message)]
pub struct SearchResponse {
    #[prost(message, repeated, tag = "1")]
    pub results: Vec<SearchResult>,
}

#[derive(Clone, PartialEq, Message)]
pub struct SearchResult {
    #[prost(bytes, tag = "1")]
    pub content_hash: Vec<u8>,
    #[prost(string, tag = "2")]
    pub filename: String,
    #[prost(uint64, tag = "3")]
    pub size: u64,
    #[prost(uint32, tag = "4")]
    pub chunk_count: u32,
    #[prost(float, tag = "5")]
    pub relevance: f32,
}

// Transfer messages

#[derive(Clone, PartialEq, Message)]
pub struct ChunkRequest {
    #[prost(bytes, tag = "1")]
    pub content_hash: Vec<u8>,
    #[prost(uint32, tag = "2")]
    pub chunk_index: u32,
    #[prost(bytes, tag = "3")]
    pub surb: Vec<u8>,
}

#[derive(Clone, PartialEq, Message)]
pub struct ChunkResponse {
    #[prost(bytes, tag = "1")]
    pub content_hash: Vec<u8>,
    #[prost(uint32, tag = "2")]
    pub chunk_index: u32,
    #[prost(bytes, tag = "3")]
    pub data: Vec<u8>,
    #[prost(bytes, tag = "4")]
    pub chunk_hash: Vec<u8>,
}

// Publishing messages

#[derive(Clone, PartialEq, Message)]
pub struct PublishRequest {
    #[prost(bytes, tag = "1")]
    pub content_hash: Vec<u8>,
    #[prost(string, tag = "2")]
    pub filename: String,
    #[prost(string, repeated, tag = "3")]
    pub keywords: Vec<String>,
    #[prost(uint64, tag = "4")]
    pub size: u64,
    #[prost(uint32, tag = "5")]
    pub chunk_count: u32,
    #[prost(string, tag = "6")]
    pub nym_address: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct PublishResponse {
    #[prost(bool, tag = "1")]
    pub success: bool,
    #[prost(string, tag = "2")]
    pub error: String,
}

// DHT messages

#[derive(Clone, PartialEq, Message)]
pub struct FindNodeRequest {
    #[prost(bytes, tag = "1")]
    pub target_id: Vec<u8>,
}

#[derive(Clone, PartialEq, Message)]
pub struct FindNodeResponse {
    #[prost(message, repeated, tag = "1")]
    pub nodes: Vec<NodeInfo>,
}

#[derive(Clone, PartialEq, Message)]
pub struct NodeInfo {
    #[prost(bytes, tag = "1")]
    pub node_id: Vec<u8>,
    #[prost(string, tag = "2")]
    pub nym_address: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct FindValueRequest {
    #[prost(bytes, tag = "1")]
    pub key: Vec<u8>,
}

#[derive(Clone, PartialEq, Message)]
pub struct FindValueResponse {
    #[prost(message, repeated, tag = "1")]
    pub seeders: Vec<ProtoSeeder>,
    #[prost(message, repeated, tag = "2")]
    pub nodes: Vec<NodeInfo>,
}

#[derive(Clone, PartialEq, Message)]
pub struct ProtoSeeder {
    #[prost(string, tag = "1")]
    pub nym_address: String,
    #[prost(bytes, tag = "2")]
    pub chunk_bitmap: Vec<u8>,
    #[prost(uint64, tag = "3")]
    pub last_seen: u64,
}

#[derive(Clone, PartialEq, Message)]
pub struct StoreRequest {
    #[prost(bytes, tag = "1")]
    pub key: Vec<u8>,
    #[prost(message, optional, tag = "2")]
    pub seeder: Option<ProtoSeeder>,
}

#[derive(Clone, PartialEq, Message)]
pub struct StoreResponse {
    #[prost(bool, tag = "1")]
    pub success: bool,
}

#[derive(Clone, PartialEq, Message)]
pub struct PingRequest {
    #[prost(bytes, tag = "1")]
    pub sender_id: Vec<u8>,
}

#[derive(Clone, PartialEq, Message)]
pub struct PingResponse {
    #[prost(bytes, tag = "1")]
    pub responder_id: Vec<u8>,
}

// Error message

#[derive(Clone, PartialEq, Message)]
pub struct ErrorResponse {
    #[prost(uint32, tag = "1")]
    pub code: u32,
    #[prost(string, tag = "2")]
    pub message: String,
}

// Helper implementations

impl Envelope {
    /// Create a new envelope with the current protocol version
    pub fn new(request_id: u64, payload: Payload) -> Self {
        Self {
            version: PROTOCOL_VERSION as u32,
            request_id,
            payload: Some(payload),
        }
    }

    /// Encode the envelope to bytes
    pub fn to_bytes(&self) -> Vec<u8> {
        self.encode_to_vec()
    }

    /// Decode an envelope from bytes, checking version compatibility
    pub fn from_bytes(buf: &[u8]) -> Result<Self> {
        let envelope = Self::decode(buf)?;

        if envelope.version != PROTOCOL_VERSION as u32 {
            return Err(Error::VersionMismatch {
                expected: PROTOCOL_VERSION,
                actual: envelope.version as u8,
            });
        }

        Ok(envelope)
    }
}

/// Error codes
pub mod error_codes {
    // Protocol errors (1xx)
    pub const VERSION_MISMATCH: u32 = 100;
    pub const INVALID_MESSAGE: u32 = 101;

    // Resource errors (2xx)
    pub const NOT_FOUND: u32 = 200;
    pub const UNAVAILABLE: u32 = 201;

    // Validation errors (3xx)
    pub const HASH_MISMATCH: u32 = 300;
    pub const INVALID_DATA: u32 = 301;
}

/// Helper functions to create common message types

pub fn search_request(request_id: u64, query: String, max_results: u32) -> Envelope {
    Envelope::new(
        request_id,
        Payload::SearchRequest(SearchRequest { query, max_results }),
    )
}

pub fn search_response(request_id: u64, results: Vec<SearchResult>) -> Envelope {
    Envelope::new(
        request_id,
        Payload::SearchResponse(SearchResponse { results }),
    )
}

pub fn chunk_request(
    request_id: u64,
    content_hash: Vec<u8>,
    chunk_index: u32,
    surb: Vec<u8>,
) -> Envelope {
    Envelope::new(
        request_id,
        Payload::ChunkRequest(ChunkRequest {
            content_hash,
            chunk_index,
            surb,
        }),
    )
}

pub fn chunk_response(
    request_id: u64,
    content_hash: Vec<u8>,
    chunk_index: u32,
    data: Vec<u8>,
    chunk_hash: Vec<u8>,
) -> Envelope {
    Envelope::new(
        request_id,
        Payload::ChunkResponse(ChunkResponse {
            content_hash,
            chunk_index,
            data,
            chunk_hash,
        }),
    )
}

pub fn error_response(request_id: u64, code: u32, message: String) -> Envelope {
    Envelope::new(
        request_id,
        Payload::ErrorResponse(ErrorResponse { code, message }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_envelope_roundtrip() {
        let original = search_request(42, "test query".to_string(), 10);
        let bytes = original.to_bytes();
        let decoded = Envelope::from_bytes(&bytes).unwrap();

        assert_eq!(original.version, decoded.version);
        assert_eq!(original.request_id, decoded.request_id);
    }
}
