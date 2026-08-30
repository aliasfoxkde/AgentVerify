//! `AgentVerify` Predicate Engine
//!
//! Deterministic predicate evaluation

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
mod engine;

pub use engine::PredicateEngine;
