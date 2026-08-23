//! AgentVerify Observe
//!
//! Observation adapters for collecting evidence from various data sources.
//!
//! This crate provides adapters for observing system state from different sources
//! such as PostgreSQL, REST APIs, Redis, and other data stores.
//!
//! # Core Concept
//!
//! An [`Observation`] captures evidence from an external system at a point in time.
//! Observations are used during the verification phase to determine whether
//! postconditions are satisfied.
//!
//! # Implementations
//!
//! - [`PostgresObserver`] - PostgreSQL-based observation via deadpool-postgres
//! - [`RedisObserver`] - Redis-based observation via deadpool-redis
//! - [`RestObserver`] - HTTP/REST-based observation (via agentverify-http)

mod postgres;
mod redis_observer;

pub use postgres::{PostgresObserver, PostgresObserverConfig, PostgresObserverError};
pub use redis_observer::{RedisObserver, RedisObserverConfig, RedisObserverError};
