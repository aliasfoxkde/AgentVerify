//! Receipt store trait and implementations
//!
//! Separates receipt persistence from the core executor.

use agentverify_core::{Receipt, ReceiptId};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ReceiptStoreError {
    #[error("Receipt not found: {0}")]
    NotFound(ReceiptId),

    #[error("Failed to store receipt: {0}")]
    StoreError(String),
}

/// Receipt store trait for persisting receipts
///
/// Implement this trait to integrate with various storage backends
/// (in-memory, file system, database, etc.)
#[async_trait::async_trait]
pub trait ReceiptStore: Send + Sync {
    /// Store a receipt
    async fn store(&self, receipt: &Receipt) -> Result<(), ReceiptStoreError>;

    /// Retrieve a receipt by ID
    async fn get(&self, id: ReceiptId) -> Result<Receipt, ReceiptStoreError>;

    /// Check if a receipt exists
    async fn exists(&self, id: ReceiptId) -> bool;
}
