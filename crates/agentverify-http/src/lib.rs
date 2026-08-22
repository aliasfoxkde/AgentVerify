//! AgentVerify HTTP
//!
//! HTTP-based observers and clients.

#[cfg(not(target_arch = "wasm32"))]
mod client;

#[cfg(not(target_arch = "wasm32"))]
mod observer;

#[cfg(target_arch = "wasm32")]
mod wasm_client;

#[cfg(target_arch = "wasm32")]
mod wasm_observer;

#[cfg(target_arch = "wasm32")]
mod wasm_storage;

#[cfg(not(target_arch = "wasm32"))]
pub use client::{
    ControlCenterClient, ControlCenterClientConfig, ControlCenterClientError, SubmissionResponse,
};

#[cfg(not(target_arch = "wasm32"))]
pub use observer::{RestObserver, RestObserverConfig, RestObserverError};

#[cfg(target_arch = "wasm32")]
pub use wasm_client::{WasmFetchOptions, WasmHttpClient, WasmHttpError};

#[cfg(target_arch = "wasm32")]
pub use wasm_observer::{WasmRestObserver, WasmRestObserverConfig};

#[cfg(target_arch = "wasm32")]
pub use wasm_storage::{ClaimResult, WasmIdempotencyStore, WasmReceiptStore, WasmStorageError};
