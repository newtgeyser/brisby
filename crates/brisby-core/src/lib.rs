//! Brisby Core - Shared types, protocols, and utilities
//!
//! This crate provides the fundamental building blocks for the Brisby
//! privacy-preserving P2P file sharing system.

pub mod chunk;
pub mod error;
pub mod proto;
pub mod transport;
pub mod types;

#[cfg(feature = "nym")]
pub mod nym_transport;

pub use error::{Error, Result};
pub use transport::{NymAddress, ReceivedMessage, SenderTag, Transport, TransportConfig, TransportHandle};
pub use types::*;

#[cfg(feature = "nym")]
pub use nym_transport::NymTransport;

/// Protocol version
pub const PROTOCOL_VERSION: u8 = 1;

/// Default chunk size: 256 KB
pub const CHUNK_SIZE: usize = 256 * 1024;
