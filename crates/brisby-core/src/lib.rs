//! Brisby Core - Shared types, protocols, and utilities
//!
//! This crate provides the fundamental building blocks for the Brisby
//! privacy-preserving P2P file sharing system.

pub mod chunk;
pub mod error;
pub mod proto;
pub mod types;

pub use error::{Error, Result};
pub use types::*;

/// Protocol version
pub const PROTOCOL_VERSION: u8 = 1;

/// Default chunk size: 256 KB
pub const CHUNK_SIZE: usize = 256 * 1024;
