//! AgentVerify Core
//!
//! Pure Rust, zero network dependencies. Contains the core verification
//! types and state machine.
//!
//! # Core Principle
//!
//! UNKNOWN is a first-class state. A timeout does NOT equal failure.

mod action;
mod contract;
mod observation;
mod predicate;
mod receipt;
mod state_machine;
mod verification_result;

pub use action::{Action, ActionId, IdempotencyKey};
pub use contract::{
    Contract, ContractId, Postcondition, Precondition, SchemaVersion, CONTRACT_SCHEMA_VERSION,
};
pub use observation::{Evidence, Observation, SourceId};
pub use predicate::{CountOperator, Predicate};
pub use receipt::{PostconditionResult, Receipt};
pub use state_machine::{State, StateMachine};
pub use verification_result::VerificationResult;

/// Core identifier types
pub mod id {
    use serde::{Deserialize, Serialize};
    use uuid::Uuid;

    /// Unique action identifier
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
    pub struct ActionId(pub Uuid);

    impl ActionId {
        pub fn new() -> Self {
            Self(Uuid::new_v4())
        }
    }

    impl Default for ActionId {
        fn default() -> Self {
            Self::new()
        }
    }

    /// Idempotency key for deduplication
    #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
    pub struct IdempotencyKey(pub String);

    impl IdempotencyKey {
        pub fn new(key: impl Into<String>) -> Self {
            Self(key.into())
        }

        pub fn from_action_id(id: ActionId) -> Self {
            Self(format!("av_{}", id.0))
        }
    }

    /// Unique contract identifier
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
    pub struct ContractId(pub Uuid);

    impl ContractId {
        pub fn new() -> Self {
            Self(Uuid::new_v4())
        }
    }

    impl Default for ContractId {
        fn default() -> Self {
            Self::new()
        }
    }

    /// Unique receipt identifier
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
    pub struct ReceiptId(pub Uuid);

    impl ReceiptId {
        pub fn new() -> Self {
            Self(Uuid::new_v4())
        }
    }

    impl Default for ReceiptId {
        fn default() -> Self {
            Self::new()
        }
    }

    /// Source identifier (e.g., "postgres", "rest", "redis")
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct SourceId(pub String);
}
