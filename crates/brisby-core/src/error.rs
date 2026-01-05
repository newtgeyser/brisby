//! Error types for Brisby

use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Hash mismatch: expected {expected}, got {actual}")]
    HashMismatch { expected: String, actual: String },

    #[error("Invalid chunk index: {index}, file has {total} chunks")]
    InvalidChunkIndex { index: u32, total: u32 },

    #[error("Protocol error: {0}")]
    Protocol(String),

    #[error("Version mismatch: expected {expected}, got {actual}")]
    VersionMismatch { expected: u8, actual: u32 },

    #[error("Decode error: {0}")]
    Decode(#[from] prost::DecodeError),

    #[error("Database error: {0}")]
    Database(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Invalid data: {0}")]
    InvalidData(String),

    #[error("Transport error: {0}")]
    Transport(String),

    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    #[error("Send failed: {0}")]
    SendFailed(String),

    #[error("Receive failed: {0}")]
    ReceiveFailed(String),

    #[error("Invalid address: {0}")]
    InvalidAddress(String),
}

pub type Result<T> = std::result::Result<T, Error>;
