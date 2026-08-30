//! `AgentVerify` Runtime
//!
//! Verified execution runtime

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
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
#[cfg(all(not(target_arch = "wasm32"), feature = "redis"))]
pub use idempotency_store::RedisIdempotencyStore;
#[cfg(not(target_arch = "wasm32"))]
pub use receipt_store::FileReceiptStore;
pub use receipt_store::{InMemoryReceiptStore, ReceiptStore};
