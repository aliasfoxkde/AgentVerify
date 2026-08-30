//! Contract types
//!
//! # Schema Version
//!
//! The current schema version is 1.0. The schema version determines compatibility
//! of contract definitions across versions. Breaking changes will increment the major version.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub use super::id::ContractId;
use super::predicate::Predicate;

/// Current contract schema version
pub const CONTRACT_SCHEMA_VERSION: &str = "1.0";

/// Contract schema version with compatibility info
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaVersion {
    pub major: u32,
    pub minor: u32,
}

impl SchemaVersion {
    #[must_use]
    pub fn new(version: &str) -> Option<Self> {
        let parts: Vec<&str> = version.split('.').collect();
        if parts.len() != 2 {
            return None;
        }
        let major = parts[0].parse().ok()?;
        let minor = parts[1].parse().ok()?;
        Some(Self { major, minor })
    }

    /// Returns true if this version is compatible with another version
    #[must_use]
    pub fn is_compatible_with(&self, other: &Self) -> bool {
        self.major == other.major
    }

    /// Returns the version string
    #[must_use]
    pub fn version_string(&self) -> String {
        format!("{}.{}", self.major, self.minor)
    }
}

impl Default for SchemaVersion {
    fn default() -> Self {
        Self { major: 1, minor: 0 }
    }
}

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsistencyMode {
    /// Strong consistency - read after write completes
    #[default]
    Strong,
    /// Eventual consistency - poll until consistent
    Eventual,
    /// Polling - wait interval, max attempts
    Polling,
    /// Webhook - wait for callback
    Webhook,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackoffType {
    /// Linear backoff
    #[default]
    Linear,
    /// Exponential backoff
    Exponential,
}

/// Action to take on recovery
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum RecoveryAction {
    /// Verify state
    Verify,
    /// Poll for result
    Poll {
        interval: chrono::Duration,
        max_attempts: u32,
    },
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
    /// Schema version for compatibility tracking
    #[serde(default = "default_schema_version")]
    pub schema_version: String,
    /// Unique identifier
    #[serde(default)]
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
    #[serde(default)]
    pub recovery: Option<RecoveryConfig>,
    /// Verification configuration
    #[serde(default)]
    pub verification: VerificationConfig,
    /// When the contract was created
    #[serde(default = "utc_now")]
    pub created_at: DateTime<Utc>,
}

fn default_schema_version() -> String {
    CONTRACT_SCHEMA_VERSION.to_string()
}

fn utc_now() -> DateTime<Utc> {
    Utc::now()
}

impl Contract {
    /// Create a new contract for an action
    pub fn new(action_name: impl Into<String>) -> Self {
        Self {
            schema_version: CONTRACT_SCHEMA_VERSION.to_string(),
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
    pub fn with_precondition(
        mut self,
        predicate: Predicate,
        description: impl Into<String>,
    ) -> Self {
        self.preconditions.push(Precondition {
            predicate,
            description: description.into(),
        });
        self
    }

    /// Add a postcondition
    pub fn with_postcondition(
        mut self,
        predicate: Predicate,
        description: impl Into<String>,
    ) -> Self {
        self.postconditions.push(Postcondition {
            predicate,
            description: description.into(),
            mandatory: true,
        });
        self
    }

    /// Add recovery configuration
    #[must_use]
    pub fn with_recovery(mut self, recovery: RecoveryConfig) -> Self {
        self.recovery = Some(recovery);
        self
    }

    /// Validate the contract for internal consistency and semantic correctness
    ///
    /// # Validation Rules
    ///
    /// - Schema version must be parseable and compatible with current version
    /// - Action name must not be empty
    /// - At least one postcondition is required
    /// - Postconditions should not have duplicate paths that could indicate copy-paste errors
    /// - Recovery config: `max_attempts` must be > 0
    /// - Recovery config: backoff max must be >= initial
    ///
    /// # Semantic Notes
    ///
    /// - `Partial` is a **terminal** failure state when ANY mandatory postcondition fails
    /// - `Partial` is **success** when only non-mandatory postconditions fail
    /// - `Duplicate` is always a **terminal success** state (idempotent)
    /// - `Unknown` requires explicit recovery action, never treated as success or failure
    pub fn validate(&self) -> Result<(), ContractValidationError> {
        // Validate schema version
        if let Some(version) = SchemaVersion::new(&self.schema_version) {
            let current = SchemaVersion::default();
            if !version.is_compatible_with(&current) {
                return Err(ContractValidationError::IncompatibleSchemaVersion {
                    expected: current.version_string(),
                    actual: self.schema_version.clone(),
                });
            }
        } else {
            return Err(ContractValidationError::InvalidSchemaVersion(
                self.schema_version.clone(),
            ));
        }

        // Validate action name
        if self.action_name.is_empty() {
            return Err(ContractValidationError::EmptyActionName);
        }

        // Must have at least one postcondition
        if self.postconditions.is_empty() {
            return Err(ContractValidationError::NoPostconditions);
        }

        // Check for duplicate postcondition paths (potential copy-paste error)
        let mut paths_seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        for postcond in &self.postconditions {
            let path = extract_predicate_path(&postcond.predicate);
            if !path.is_empty() && !paths_seen.insert(path.clone()) {
                return Err(ContractValidationError::DuplicatePostconditionPath(path));
            }
        }

        // Validate recovery configuration
        if let Some(ref recovery) = self.recovery {
            if recovery.max_attempts == 0 {
                return Err(ContractValidationError::InvalidMaxAttempts);
            }

            if let Some(ref backoff) = recovery.backoff {
                if backoff.max < backoff.initial {
                    return Err(ContractValidationError::InvalidBackoff {
                        initial: backoff.initial,
                        max: backoff.max,
                    });
                }
                if backoff.multiplier <= 0.0 {
                    return Err(ContractValidationError::InvalidBackoffMultiplier(
                        backoff.multiplier,
                    ));
                }
            }
        }

        Ok(())
    }
}

/// Extract the primary path from a predicate for duplicate checking
fn extract_predicate_path(predicate: &Predicate) -> String {
    match predicate {
        Predicate::Exists { path } => path.clone(),
        Predicate::NotExists { path } => path.clone(),
        Predicate::Equals { path, .. } => path.clone(),
        Predicate::NotEquals { path, .. } => path.clone(),
        Predicate::Contains { path, .. } => path.clone(),
        Predicate::Matches { path, .. } => path.clone(),
        Predicate::GreaterThan { path, .. } => path.clone(),
        Predicate::LessThan { path, .. } => path.clone(),
        Predicate::Count { path, .. } => path.clone(),
        Predicate::IsEmpty { path } => path.clone(),
        Predicate::IsNotEmpty { path } => path.clone(),
        Predicate::All { predicates } => predicates
            .first()
            .map(extract_predicate_path)
            .unwrap_or_default(),
        Predicate::Any { predicates } => predicates
            .first()
            .map(extract_predicate_path)
            .unwrap_or_default(),
        Predicate::Not { predicate } => extract_predicate_path(predicate),
        Predicate::Implies { antecedent, .. } => extract_predicate_path(antecedent),
    }
}

/// Contract validation error
#[derive(Debug, Clone, thiserror::Error)]
pub enum ContractValidationError {
    #[error("Invalid schema version: {0}")]
    InvalidSchemaVersion(String),

    #[error("Incompatible schema version: expected {expected}, got {actual}")]
    IncompatibleSchemaVersion { expected: String, actual: String },

    #[error("Action name cannot be empty")]
    EmptyActionName,

    #[error("Contract must have at least one postcondition")]
    NoPostconditions,

    #[error("Duplicate postcondition path: {0}")]
    DuplicatePostconditionPath(String),

    #[error("max_attempts must be greater than 0")]
    InvalidMaxAttempts,

    #[error("Backoff max ({max}) must be >= initial ({initial})")]
    InvalidBackoff {
        initial: chrono::Duration,
        max: chrono::Duration,
    },

    #[error("Backoff multiplier must be positive, got {0}")]
    InvalidBackoffMultiplier(f64),
}
