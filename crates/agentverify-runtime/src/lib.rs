//! AgentVerify Runtime
//!
//! Verified execution runtime

mod action_executor;
mod executor;
mod idempotency_store;
mod receipt_store;

pub use action_executor::{
    ActionExecutor, DispatchError, DispatchOutcome, SimulatedActionExecutor,
};
pub use executor::{
    ClaimResult, Executor, ExecutorConfig, ExecutorError, IdempotencyRegistry, IdempotencyStore,
    Observer,
};
#[cfg(not(target_arch = "wasm32"))]
pub use idempotency_store::FileIdempotencyStore;
#[cfg(not(target_arch = "wasm32"))]
pub use receipt_store::FileReceiptStore;
pub use receipt_store::{InMemoryReceiptStore, ReceiptStore};
