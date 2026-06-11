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

    #[error("Subxt client block error: {0}")]
    SubxtClientBlock(#[from] subxt::error::OnlineClientAtBlockError),

    #[error("Subxt storage error: {0}")]
    SubxtStorage(#[from] subxt::error::StorageError),

    #[error("Subxt storage decode error: {0}")]
    SubxtStorageDecode(#[from] subxt::error::StorageValueError),

    #[error("Subxt constant error: {0}")]
    SubxtConstant(#[from] subxt::error::ConstantError),

    #[error("Subxt extrinsic error: {0}")]
    SubxtExtrinsic(#[from] subxt::error::ExtrinsicError),

    #[error("Subxt transaction progress error: {0}")]
    SubxtTxProgress(#[from] subxt::error::TransactionProgressError),

    #[error("Light client error: {0}")]
    LightClient(String),

    #[error("Invalid data: {0}")]
    InvalidData(String),

    #[error("Invalid address: {0}")]
    InvalidAddress(String),
}
