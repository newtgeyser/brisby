//! Brisby DHT - Kademlia DHT implementation over Nym
//!
//! This crate provides a distributed hash table for peer discovery,
//! mapping content hashes to seeders who have the file.

pub mod routing;
pub mod storage;

use brisby_core::ContentHash;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum DhtError {
    #[error("Node not found")]
    NodeNotFound,

    #[error("Timeout waiting for response")]
    Timeout,

    #[error("Network error: {0}")]
    Network(String),
}

pub type Result<T> = std::result::Result<T, DhtError>;

/// DHT node configuration
#[derive(Debug, Clone)]
pub struct DhtConfig {
    /// Number of nodes per k-bucket
    pub k: usize,
    /// Parallelism factor for lookups
    pub alpha: usize,
    /// Node ID (32 bytes)
    pub node_id: ContentHash,
}

impl Default for DhtConfig {
    fn default() -> Self {
        Self {
            k: 20,
            alpha: 3,
            node_id: generate_random_node_id(),
        }
    }
}

/// Generate a cryptographically random node ID
pub fn generate_random_node_id() -> ContentHash {
    let mut node_id = [0u8; 32];
    getrandom::getrandom(&mut node_id).expect("Failed to generate random bytes");
    node_id
}
