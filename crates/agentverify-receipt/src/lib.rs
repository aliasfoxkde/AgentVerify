//! `AgentVerify` Receipt
//!
//! Receipt signing and verification.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
mod signing;

pub use signing::{Ed25519SigningService, SigningError, SigningService};
