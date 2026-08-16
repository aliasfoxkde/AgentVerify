//! AgentVerify HTTP
//!
//! HTTP-based observers and clients.
//!
//! Note: This crate is only available on non-WASM platforms due to
//! reqwest limitations on wasm32-wasip1 target.

#[cfg(not(target_arch = "wasm32"))]
mod client;

#[cfg(not(target_arch = "wasm32"))]
mod observer;

#[cfg(not(target_arch = "wasm32"))]
pub use client::{
    ControlCenterClient, ControlCenterClientConfig, ControlCenterClientError, SubmissionResponse,
};

#[cfg(not(target_arch = "wasm32"))]
pub use observer::{RestObserver, RestObserverConfig, RestObserverError};
