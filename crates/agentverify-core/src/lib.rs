//! `AgentVerify` Core
//!
//! Pure Rust, zero network dependencies. Contains the core verification
//! types and state machine.
//!
//! # Core Principle
//!
//! UNKNOWN is a first-class state. A timeout does NOT equal failure.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
mod action;
mod contract;
mod observation;
mod predicate;
mod receipt;
mod state_machine;
mod verification_result;

pub use action::{Action, IdempotencyKey};
pub use contract::{
    BackoffConfig, BackoffType, ConsistencyMode, Contract, ContractValidationError, Postcondition,
    Precondition, RecoveryAction, RecoveryConfig, RecoveryStrategy, SchemaVersion,
    CONTRACT_SCHEMA_VERSION,
};
pub use id::{ActionId, ContractId, ReceiptId, SourceId};
pub use observation::{Evidence, Observation};
pub use predicate::{CountOperator, Predicate};
#[cfg(not(target_arch = "wasm32"))]
pub use receipt::FileReceiptStore;
pub use receipt::{
    InMemoryReceiptStore, PostconditionResult, Receipt, ReceiptStore, ReceiptStoreError,
    RECEIPT_SCHEMA_VERSION,
};
pub use state_machine::{State, StateMachine};
pub use verification_result::VerificationResult;

/// Core identifier types
pub mod id {
    use serde::{Deserialize, Serialize};
    use uuid::Uuid;

    // Re-export SourceId from observation module for convenience
    pub use crate::observation::SourceId;

    /// Unique action identifier.
    ///
    /// `ActionId` is a type-safe wrapper around a UUID that identifies a specific
    /// action execution. Each action created via [`crate::Action::new`] or
    /// [`crate::Action::with_idempotency`] receives a unique `ActionId`.
    ///
    /// # Example
    ///
    /// ```
    /// use agentverify_core::{Action, ActionId};
    ///
    /// let action = Action::new("create_user", serde_json::json!({}));
    /// assert!(matches!(action.id, ActionId(_)));
    /// ```
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
    pub struct ActionId(pub Uuid);

    impl ActionId {
        /// Generate a new unique `ActionId`.
        #[must_use]
        pub fn new() -> Self {
            Self(Uuid::new_v4())
        }
    }

    impl Default for ActionId {
        fn default() -> Self {
            Self::new()
        }
    }

    impl std::fmt::Display for ActionId {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.0)
        }
    }

    /// Idempotency key for deduplication.
    ///
    /// `IdempotencyKey` ensures that the same logical action is not executed multiple times.
    /// When an action is created with an `IdempotencyKey`, the verification system can detect
    /// duplicate attempts and return the cached result.
    ///
    /// # Example
    ///
    /// ```
    /// use agentverify_core::{Action, IdempotencyKey};
    ///
    /// let key = IdempotencyKey::new("create_user_user@example.com");
    /// let action = Action::with_idempotency(
    ///     "create_user",
    ///     serde_json::json!({}),
    ///     key,
    /// );
    /// ```
    #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
    pub struct IdempotencyKey(pub String);

    impl IdempotencyKey {
        /// Create a new idempotency key from a string.
        pub fn new(key: impl Into<String>) -> Self {
            Self(key.into())
        }

        /// Create an idempotency key from an `ActionId`.
        ///
        /// The resulting key will be formatted as `av_{uuid}`.
        #[must_use]
        pub fn from_action_id(id: ActionId) -> Self {
            Self(format!("av_{}", id.0))
        }
    }

    /// Unique contract identifier.
    ///
    /// `ContractId` is a type-safe wrapper around a UUID that identifies a specific
    /// contract definition. Contracts define the preconditions, postconditions,
    /// and recovery strategies for verifying an action.
    ///
    /// # Example
    ///
    /// ```
    /// use agentverify_core::ContractId;
    ///
    /// let contract_id = ContractId::new();
    /// println!("Contract: {}", contract_id);
    /// ```
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
    pub struct ContractId(pub Uuid);

    impl ContractId {
        /// Generate a new unique `ContractId`.
        #[must_use]
        pub fn new() -> Self {
            Self(Uuid::new_v4())
        }
    }

    impl Default for ContractId {
        fn default() -> Self {
            Self::new()
        }
    }

    impl std::fmt::Display for ContractId {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.0)
        }
    }

    /// Unique receipt identifier.
    ///
    /// `ReceiptId` identifies a specific verification receipt. Receipts are created
    /// after verification completes and contain the evidence and results of the
    /// verification process.
    ///
    /// # Example
    ///
    /// ```
    /// use agentverify_core::ReceiptId;
    ///
    /// let receipt_id = ReceiptId::new();
    /// println!("Receipt: {}", receipt_id);
    /// ```
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
    pub struct ReceiptId(pub Uuid);

    impl ReceiptId {
        /// Generate a new unique `ReceiptId`.
        #[must_use]
        pub fn new() -> Self {
            Self(Uuid::new_v4())
        }
    }

    impl Default for ReceiptId {
        fn default() -> Self {
            Self::new()
        }
    }

    impl std::fmt::Display for ReceiptId {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.0)
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn action_id_default_generates_unique_ids() {
            let a = ActionId::default();
            let b = ActionId::default();
            assert_ne!(a, b);
            assert_ne!(a.0, Uuid::nil());
        }

        #[test]
        fn contract_id_default_generates_unique_ids() {
            let a = ContractId::default();
            let b = ContractId::default();
            assert_ne!(a, b);
            assert_ne!(a.0, Uuid::nil());
        }

        #[test]
        fn receipt_id_default_generates_unique_ids() {
            let a = ReceiptId::default();
            let b = ReceiptId::default();
            assert_ne!(a, b);
            assert_ne!(a.0, Uuid::nil());
        }

        #[test]
        fn ids_display_as_uuids() {
            let action_id = ActionId::new();
            assert_eq!(action_id.to_string(), action_id.0.to_string());

            let contract_id = ContractId::new();
            assert_eq!(contract_id.to_string(), contract_id.0.to_string());

            let receipt_id = ReceiptId::new();
            assert_eq!(receipt_id.to_string(), receipt_id.0.to_string());
        }

        #[test]
        fn idempotency_key_from_action_id_is_prefixed() {
            let action_id = ActionId::new();
            let key = IdempotencyKey::from_action_id(action_id);
            assert_eq!(key, IdempotencyKey(format!("av_{}", action_id.0)));
            assert!(key.0.starts_with("av_"));
        }

        #[test]
        fn idempotency_key_accepts_any_string() {
            let key = IdempotencyKey::new(String::from("caller-supplied-key"));
            assert_eq!(key, IdempotencyKey(String::from("caller-supplied-key")));
        }

        #[test]
        fn ids_roundtrip_through_serde() {
            let action_id = ActionId::new();
            let json = serde_json::to_string(&action_id).unwrap();
            let back: ActionId = serde_json::from_str(&json).unwrap();
            assert_eq!(back, action_id);

            let contract_id = ContractId::new();
            let json = serde_json::to_string(&contract_id).unwrap();
            let back: ContractId = serde_json::from_str(&json).unwrap();
            assert_eq!(back, contract_id);

            let receipt_id = ReceiptId::new();
            let json = serde_json::to_string(&receipt_id).unwrap();
            let back: ReceiptId = serde_json::from_str(&json).unwrap();
            assert_eq!(back, receipt_id);

            let key = IdempotencyKey::new("k");
            let json = serde_json::to_string(&key).unwrap();
            let back: IdempotencyKey = serde_json::from_str(&json).unwrap();
            assert_eq!(back, key);
        }

        #[test]
        fn source_id_roundtrips_through_serde() {
            let source = SourceId(String::from("postgres"));
            let json = serde_json::to_string(&source).unwrap();
            assert_eq!(json, r#""postgres""#);
            let back: SourceId = serde_json::from_str(&json).unwrap();
            assert_eq!(back, source);
        }
    }
}
