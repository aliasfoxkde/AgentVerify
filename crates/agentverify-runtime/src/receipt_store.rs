//! Receipt store re-exports from agentverify-core
//!
//! The ReceiptStore trait, InMemoryReceiptStore, and FileReceiptStore are defined
//! in agentverify-core.

pub use agentverify_core::{InMemoryReceiptStore, ReceiptStore};
#[cfg(not(target_arch = "wasm32"))]
pub use agentverify_core::FileReceiptStore;
