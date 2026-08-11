//! Contract types

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub use super::id::ContractId;
use super::predicate::Predicate;

/// Verification configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationConfig {
    /// Consistency mode
    #[serde(default)]
    pub consistency: ConsistencyMode,
    /// Verification timeout
    #[serde(default)]
    pub timeout: chrono::Duration,
}

impl Default for VerificationConfig {
    fn default() -> Self {
        Self {
            consistency: ConsistencyMode::Strong,
            timeout: chrono::Duration::seconds(5),
        }
    }
}

/// Consistency mode for verification
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsistencyMode {
    /// Strong consistency - read after write completes
    Strong,
    /// Eventual consistency - poll until consistent
    Eventual,
    /// Polling - wait interval, max attempts
    Polling,
    /// Webhook - wait for callback
    Webhook,
}

impl Default for ConsistencyMode {
    fn default() -> Self {
        Self::Strong
    }
}

/// Recovery configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryConfig {
    /// Recovery strategy
    pub strategy: RecoveryStrategy,
    /// Maximum retry attempts
    #[serde(default = "default_max_attempts")]
    pub max_attempts: u32,
    /// Backoff configuration
    pub backoff: Option<BackoffConfig>,
    /// Actions to take on unknown
    #[serde(default)]
    pub on_unknown: Vec<RecoveryAction>,
}

fn default_max_attempts() -> u32 {
    3
}

/// Recovery strategy
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryStrategy {
    /// No action - leave as-is
    NoAction,
    /// Standard retry without verification
    Retry,
    /// Verify before retry (RECOMMENDED)
    VerifyThenRetry,
    /// Poll until consistent
    Poll,
    /// Compensate (saga pattern)
    Compensate,
    /// Rollback transaction
    Rollback,
    /// Escalate to human
    Escalate,
    /// Require human approval
    HumanApproval,
    /// Abort entirely
    Abort,
}

/// Backoff configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackoffConfig {
    /// Backoff type
    #[serde(default)]
    pub backoff_type: BackoffType,
    /// Initial delay
    pub initial: chrono::Duration,
    /// Maximum delay
    pub max: chrono::Duration,
    /// Multiplier (for exponential)
    #[serde(default = "default_multiplier")]
    pub multiplier: f64,
}

fn default_multiplier() -> f64 {
    2.0
}

/// Backoff type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackoffType {
    /// Linear backoff
    Linear,
    /// Exponential backoff
    Exponential,
}

impl Default for BackoffType {
    fn default() -> Self {
        Self::Linear
    }
}

/// Action to take on recovery
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum RecoveryAction {
    /// Verify state
    Verify,
    /// Poll for result
    Poll { interval: chrono::Duration, max_attempts: u32 },
    /// Send alert
    Alert { severity: AlertSeverity },
    /// Human approval required
    RequireApproval,
}

/// Alert severity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertSeverity {
    Info,
    Warning,
    Error,
    Critical,
}

/// A precondition that must be true before action execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Precondition {
    /// The predicate to evaluate
    pub predicate: Predicate,
    /// Human-readable description
    #[serde(default)]
    pub description: String,
}

/// A postcondition that must be true after action completes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Postcondition {
    /// The predicate to evaluate
    pub predicate: Predicate,
    /// Human-readable description
    #[serde(default)]
    pub description: String,
    /// Whether this postcondition is mandatory
    /// If false, PARTIAL is allowed when this fails
    #[serde(default = "default_mandatory")]
    pub mandatory: bool,
}

fn default_mandatory() -> bool {
    true
}

/// A verification contract
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contract {
    /// Unique identifier
    pub id: ContractId,
    /// Action name this contract applies to
    pub action_name: String,
    /// Preconditions
    #[serde(default)]
    pub preconditions: Vec<Precondition>,
    /// Postconditions
    #[serde(default)]
    pub postconditions: Vec<Postcondition>,
    /// Recovery configuration
    pub recovery: Option<RecoveryConfig>,
    /// Verification configuration
    #[serde(default)]
    pub verification: VerificationConfig,
    /// When the contract was created
    #[serde(default = "utc_now")]
    pub created_at: DateTime<Utc>,
}

fn utc_now() -> DateTime<Utc> {
    Utc::now()
}

impl Contract {
    /// Create a new contract for an action
    pub fn new(action_name: impl Into<String>) -> Self {
        Self {
            id: ContractId::new(),
            action_name: action_name.into(),
            preconditions: Vec::new(),
            postconditions: Vec::new(),
            recovery: None,
            verification: VerificationConfig::default(),
            created_at: Utc::now(),
        }
    }

    /// Add a precondition
    pub fn with_precondition(mut self, predicate: Predicate, description: impl Into<String>) -> Self {
        self.preconditions.push(Precondition {
            predicate,
            description: description.into(),
        });
        self
    }

    /// Add a postcondition
    pub fn with_postcondition(mut self, predicate: Predicate, description: impl Into<String>) -> Self {
        self.postconditions.push(Postcondition {
            predicate,
            description: description.into(),
            mandatory: true,
        });
        self
    }

    /// Add recovery configuration
    pub fn with_recovery(mut self, recovery: RecoveryConfig) -> Self {
        self.recovery = Some(recovery);
        self
    }
}
