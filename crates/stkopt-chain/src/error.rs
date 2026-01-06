//! Error types for chain operations.

use thiserror::Error;

#[derive(Error, Debug)]
pub enum ChainError {
    #[error("Failed to connect to chain: {0}")]
    Connection(String),

    #[error("RPC error: {0}")]
    Rpc(String),

    #[error("Storage query failed: {0}")]
    Storage(String),

    #[error("Subxt error: {0}")]
    Subxt(#[from] subxt::Error),

    #[error("Decode error: {0}")]
    Decode(#[from] subxt::error::DecodeError),

    #[error("Light client error: {0}")]
    LightClient(String),

    #[error("Invalid data: {0}")]
    InvalidData(String),
}
