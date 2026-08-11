//! Action types

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Unique action identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ActionId(pub uuid::Uuid);

impl ActionId {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4())
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

/// An action to be verified
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Action {
    /// Unique identifier
    pub id: ActionId,
    /// Action name (e.g., "create_customer")
    pub name: String,
    /// JSON arguments passed to the action
    pub arguments: Value,
    /// Optional idempotency key
    pub idempotency_key: Option<IdempotencyKey>,
    /// When the action was created
    pub created_at: DateTime<Utc>,
}

impl Action {
    /// Create a new action with generated ID and timestamp
    pub fn new(name: impl Into<String>, arguments: Value) -> Self {
        Self {
            id: ActionId::new(),
            name: name.into(),
            arguments,
            idempotency_key: None,
            created_at: Utc::now(),
        }
    }

    /// Create a new action with idempotency key
    pub fn with_idempotency(
        name: impl Into<String>,
        arguments: Value,
        key: IdempotencyKey,
    ) -> Self {
        Self {
            id: ActionId::new(),
            name: name.into(),
            arguments,
            idempotency_key: Some(key),
            created_at: Utc::now(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_new_creates_unique_id() {
        let action1 = Action::new("test", serde_json::json!({}));
        let action2 = Action::new("test", serde_json::json!({}));

        assert_ne!(action1.id, action2.id);
    }

    #[test]
    fn action_with_idempotency_key() {
        let key = IdempotencyKey::new("user-123-action-456");
        let action = Action::with_idempotency("create", serde_json::json!({}), key);

        assert!(action.idempotency_key.is_some());
    }
}
