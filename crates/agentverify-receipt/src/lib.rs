//! AgentVerify Receipt
//!
//! Receipt signing and verification.

mod signing;

pub use signing::{Ed25519SigningService, SigningError, SigningService};
