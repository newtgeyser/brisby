//! Core data types for Brisby

use serde::{Deserialize, Serialize};

/// A 32-byte BLAKE3 hash
pub type ContentHash = [u8; 32];

/// Information about a file chunk
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkInfo {
    /// Index of the chunk (0-based)
    pub index: u32,
    /// BLAKE3 hash of the chunk data
    pub hash: ContentHash,
    /// Size of the chunk in bytes (may be smaller for last chunk)
    pub size: u32,
}

/// Metadata for a shared file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMetadata {
    /// BLAKE3 hash of the file (computed from chunk hashes)
    pub content_hash: ContentHash,
    /// Original filename
    pub filename: String,
    /// File size in bytes
    pub size: u64,
    /// MIME type (if detected)
    pub mime_type: Option<String>,
    /// List of chunks
    pub chunks: Vec<ChunkInfo>,
    /// Searchable keywords extracted from filename
    pub keywords: Vec<String>,
    /// Unix timestamp when the file was added
    pub created_at: u64,
}

/// Entry stored in the search index (at index providers)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexEntry {
    /// Content hash of the file
    pub content_hash: ContentHash,
    /// Filename (searchable)
    pub filename: String,
    /// Keywords (searchable)
    pub keywords: Vec<String>,
    /// File size in bytes
    pub size: u64,
    /// Number of chunks
    pub chunk_count: u32,
    /// Unix timestamp when published
    pub published_at: u64,
    /// Time-to-live in seconds
    pub ttl: u64,
}

/// A seeder (peer with file chunks) in the DHT
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Seeder {
    /// Nym address to contact this seeder
    pub nym_address: String,
    /// Bitmap indicating which chunks this seeder has
    pub chunk_bitmap: Vec<u8>,
    /// Unix timestamp when last seen
    pub last_seen: u64,
}

/// Search result returned by index providers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    /// Content hash of the file
    pub content_hash: ContentHash,
    /// Filename
    pub filename: String,
    /// File size in bytes
    pub size: u64,
    /// Number of chunks
    pub chunk_count: u32,
    /// Relevance score (higher is better)
    pub relevance: f32,
}

impl FileMetadata {
    /// Extract keywords from a filename
    pub fn extract_keywords(filename: &str) -> Vec<String> {
        filename
            .split(|c: char| !c.is_alphanumeric())
            .filter(|s| s.len() >= 2)
            .map(|s| s.to_lowercase())
            .collect()
    }
}

/// Helper to format a content hash as hex string
pub fn hash_to_hex(hash: &ContentHash) -> String {
    hex::encode(hash)
}

/// Helper to parse a hex string into a content hash
pub fn hex_to_hash(s: &str) -> Result<ContentHash, hex::FromHexError> {
    let bytes = hex::decode(s)?;
    if bytes.len() != 32 {
        return Err(hex::FromHexError::InvalidStringLength);
    }
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&bytes);
    Ok(hash)
}
