//! AgentVerify HTTP
//!
//! HTTP-based observers and clients.

#[cfg(not(target_arch = "wasm32"))]
mod client;

#[cfg(not(target_arch = "wasm32"))]
mod observer;

#[cfg(target_arch = "wasm32")]
mod wasm_client;

#[cfg(not(target_arch = "wasm32"))]
pub use client::{
    ControlCenterClient, ControlCenterClientConfig, ControlCenterClientError, SubmissionResponse,
};

#[cfg(not(target_arch = "wasm32"))]
pub use observer::{RestObserver, RestObserverConfig, RestObserverError};

#[cfg(target_arch = "wasm32")]
pub use wasm_client::{WasmHttpClient, WasmFetchOptions, WasmHttpError};
