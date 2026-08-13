//! AgentVerify HTTP
//!
//! HTTP-based observers and clients.

mod observer;

pub use observer::{RestObserver, RestObserverConfig, RestObserverError};
