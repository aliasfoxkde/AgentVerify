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
    /// Major version; must match the current major version for compatibility.
    pub major: u32,
    /// Minor version; backwards-compatible additions increment this value.
    pub minor: u32,
}

impl SchemaVersion {
    /// Parses a `"<major>.<minor>"` version string.
    ///
    /// Returns `None` when the string does not have exactly two
    /// dot-separated components or when either component is not a `u32`.
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
        /// Interval between polls.
        interval: chrono::Duration,
        /// Maximum number of polls before giving up.
        max_attempts: u32,
    },
    /// Send alert
    Alert {
        /// Severity assigned to the alert.
        severity: AlertSeverity,
    },
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
    #[must_use]
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
    #[must_use]
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
    ///
    /// # Errors
    ///
    /// Returns the first [`ContractValidationError`] encountered, in the order
    /// the rules above are listed: schema version problems first, then an empty
    /// action name, missing postconditions, duplicate postcondition paths, and
    /// finally invalid recovery configuration.
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
        Predicate::Exists { path }
        | Predicate::NotExists { path }
        | Predicate::Equals { path, .. }
        | Predicate::NotEquals { path, .. }
        | Predicate::Contains { path, .. }
        | Predicate::Matches { path, .. }
        | Predicate::GreaterThan { path, .. }
        | Predicate::LessThan { path, .. }
        | Predicate::Count { path, .. }
        | Predicate::IsEmpty { path }
        | Predicate::IsNotEmpty { path } => path.clone(),
        Predicate::All { predicates } | Predicate::Any { predicates } => predicates
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
    /// The schema version string could not be parsed as `"<major>.<minor>"`.
    #[error("Invalid schema version: {0}")]
    InvalidSchemaVersion(String),

    /// The schema version has a different major version than the current one.
    #[error("Incompatible schema version: expected {expected}, got {actual}")]
    IncompatibleSchemaVersion {
        /// The schema version supported by this build of the library.
        expected: String,
        /// The schema version declared by the contract.
        actual: String,
    },

    /// The contract's action name is empty.
    #[error("Action name cannot be empty")]
    EmptyActionName,

    /// The contract declares no postconditions to verify.
    #[error("Contract must have at least one postcondition")]
    NoPostconditions,

    /// Two postconditions target the same state path.
    #[error("Duplicate postcondition path: {0}")]
    DuplicatePostconditionPath(String),

    /// The recovery configuration allows zero retry attempts.
    #[error("max_attempts must be greater than 0")]
    InvalidMaxAttempts,

    /// The backoff configuration's maximum delay is shorter than its initial delay.
    #[error("Backoff max ({max}) must be >= initial ({initial})")]
    InvalidBackoff {
        /// The configured initial backoff delay.
        initial: chrono::Duration,
        /// The configured maximum backoff delay.
        max: chrono::Duration,
    },

    /// The backoff multiplier is zero or negative.
    #[error("Backoff multiplier must be positive, got {0}")]
    InvalidBackoffMultiplier(f64),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::predicate::CountOperator;

    fn valid_contract() -> Contract {
        Contract::new("refund_customer")
            .with_postcondition(Predicate::exists("refund.status"), "refund recorded")
    }

    /// Recovery config with the optional fields left at their defaults.
    fn recovery(
        strategy: RecoveryStrategy,
        max_attempts: u32,
        backoff: Option<BackoffConfig>,
    ) -> RecoveryConfig {
        RecoveryConfig {
            strategy,
            max_attempts,
            backoff,
            on_unknown: Vec::new(),
        }
    }

    /// Postcondition predicates carry no `PartialEq`, so equality is checked
    /// through the serialized contract format that crosses service boundaries.
    fn json_of<T: Serialize>(value: &T) -> serde_json::Value {
        serde_json::to_value(value).expect("serializable")
    }

    // ------------------------------------------------------------------
    // SchemaVersion
    // ------------------------------------------------------------------

    #[test]
    fn schema_version_parses_major_and_minor() {
        let version = SchemaVersion::new("2.7").expect("parseable version");
        assert_eq!(version.major, 2);
        assert_eq!(version.minor, 7);
        assert_eq!(version.version_string(), "2.7");
    }

    #[test]
    fn schema_version_rejects_malformed_strings() {
        for candidate in ["", "1", "1.0.0", "1.", ".1", "a.b", "1.x", "x.1", "1.-2"] {
            assert!(
                SchemaVersion::new(candidate).is_none(),
                "{candidate} must not parse as a schema version"
            );
        }
    }

    #[test]
    fn schema_version_compatibility_requires_matching_major() {
        let current = SchemaVersion::default();
        assert!(SchemaVersion::new("1.0")
            .unwrap()
            .is_compatible_with(&current));
        assert!(SchemaVersion::new("1.9")
            .unwrap()
            .is_compatible_with(&current));
        assert!(!SchemaVersion::new("2.0")
            .unwrap()
            .is_compatible_with(&current));
        assert!(!SchemaVersion::new("0.9")
            .unwrap()
            .is_compatible_with(&current));
    }

    #[test]
    fn schema_version_default_is_one_zero() {
        let version = SchemaVersion::default();
        assert_eq!(version.major, 1);
        assert_eq!(version.minor, 0);
        assert_eq!(version.version_string(), "1.0");
    }

    #[test]
    fn contract_schema_version_constant_is_current() {
        assert_eq!(CONTRACT_SCHEMA_VERSION, "1.0");
        assert_eq!(default_schema_version(), "1.0");
        assert_eq!(
            default_schema_version(),
            SchemaVersion::default().version_string()
        );
    }

    // ------------------------------------------------------------------
    // Builders
    // ------------------------------------------------------------------

    #[test]
    fn contract_new_sets_schema_defaults() {
        let contract = Contract::new("create_customer");
        assert_eq!(contract.schema_version, "1.0");
        assert_eq!(contract.action_name, "create_customer");
        assert!(contract.preconditions.is_empty());
        assert!(contract.postconditions.is_empty());
        assert!(contract.recovery.is_none());
        assert_eq!(
            contract.verification.consistency,
            VerificationConfig::default().consistency
        );
    }

    #[test]
    fn contract_new_accepts_owned_string() {
        let contract = Contract::new(String::from("from_owned"));
        assert_eq!(contract.action_name, "from_owned");
    }

    #[test]
    fn contract_builders_accumulate() {
        let contract = Contract::new("charge")
            .with_precondition(Predicate::exists("account.id"), "account exists")
            .with_postcondition(Predicate::equals("charge.status", "captured"), "captured")
            .with_recovery(recovery(RecoveryStrategy::VerifyThenRetry, 4, None));

        assert_eq!(contract.preconditions.len(), 1);
        assert_eq!(contract.preconditions[0].description, "account exists");
        assert_eq!(contract.postconditions.len(), 1);
        assert!(
            contract.postconditions[0].mandatory,
            "builder marks mandatory"
        );
        let recovery = contract.recovery.expect("recovery configured");
        assert_eq!(recovery.strategy, RecoveryStrategy::VerifyThenRetry);
        assert_eq!(recovery.max_attempts, 4);
    }

    #[test]
    fn contract_ids_are_unique_per_instance() {
        let a = Contract::new("a");
        let b = Contract::new("a");
        assert_ne!(a.id, b.id);
    }

    // ------------------------------------------------------------------
    // validate()
    // ------------------------------------------------------------------

    #[test]
    fn validate_accepts_well_formed_contract() {
        let mut contract = valid_contract();
        contract.preconditions.push(Precondition {
            predicate: Predicate::exists("account.id"),
            description: "account".into(),
        });
        assert!(contract.validate().is_ok());
    }

    #[test]
    fn validate_rejects_unparseable_schema_version() {
        let mut contract = valid_contract();
        contract.schema_version = "latest".into();
        let err = contract.validate().expect_err("invalid version");
        assert!(matches!(
            err,
            ContractValidationError::InvalidSchemaVersion(ref s) if s == "latest"
        ));
        assert_eq!(err.to_string(), "Invalid schema version: latest");
    }

    #[test]
    fn validate_rejects_incompatible_schema_version() {
        let mut contract = valid_contract();
        contract.schema_version = "2.0".into();
        let err = contract.validate().expect_err("incompatible version");
        assert_eq!(
            err.to_string(),
            "Incompatible schema version: expected 1.0, got 2.0"
        );
    }

    #[test]
    fn validate_rejects_empty_action_name() {
        let mut contract = valid_contract();
        contract.action_name = String::new();
        let err = contract.validate().expect_err("empty name");
        assert!(matches!(err, ContractValidationError::EmptyActionName));
        assert_eq!(err.to_string(), "Action name cannot be empty");
    }

    #[test]
    fn validate_requires_at_least_one_postcondition() {
        let contract = Contract::new("no_postconditions");
        let err = contract.validate().expect_err("no postconditions");
        assert!(matches!(err, ContractValidationError::NoPostconditions));
        assert_eq!(
            err.to_string(),
            "Contract must have at least one postcondition"
        );
    }

    #[test]
    fn validate_detects_duplicate_simple_path() {
        let contract = Contract::new("dup")
            .with_postcondition(Predicate::exists("refund.status"), "first")
            .with_postcondition(Predicate::equals("refund.status", "ok"), "second");
        let err = contract.validate().expect_err("duplicate path");
        assert_eq!(
            err.to_string(),
            "Duplicate postcondition path: refund.status"
        );
    }

    #[test]
    fn extract_predicate_path_unwraps_compound_predicates() {
        let inner = Predicate::exists("ledger.entry");
        let compounds = vec![
            Predicate::all(vec![inner.clone()]),
            Predicate::any(vec![inner.clone()]),
            Predicate::negate(inner.clone()),
            Predicate::Implies {
                antecedent: Box::new(inner.clone()),
                consequent: Box::new(Predicate::exists("other.path")),
            },
        ];

        for predicate in compounds {
            let contract = Contract::new("compound")
                .with_postcondition(predicate, "one")
                .with_postcondition(Predicate::exists("ledger.entry"), "two");
            assert!(
                contract.validate().is_err(),
                "compound predicate must expose its inner path for duplicate detection"
            );
        }
    }

    #[test]
    fn extract_predicate_path_covers_every_predicate_shape() {
        fn path_of(predicate: &Predicate) -> String {
            extract_predicate_path(predicate)
        }

        assert_eq!(
            extract_predicate_path(&Predicate::Exists { path: "a".into() }),
            "a"
        );
        assert_eq!(
            extract_predicate_path(&Predicate::NotExists { path: "b".into() }),
            "b"
        );
        assert_eq!(
            path_of(&Predicate::Equals {
                path: "c".into(),
                value: 1.into()
            }),
            "c"
        );
        assert_eq!(
            path_of(&Predicate::NotEquals {
                path: "d".into(),
                value: 1.into()
            }),
            "d"
        );
        assert_eq!(
            path_of(&Predicate::Contains {
                path: "e".into(),
                value: 1.into()
            }),
            "e"
        );
        assert_eq!(
            path_of(&Predicate::Matches {
                path: "f".into(),
                pattern: "^x".into()
            }),
            "f"
        );
        assert_eq!(
            path_of(&Predicate::GreaterThan {
                path: "g".into(),
                value: 1.into()
            }),
            "g"
        );
        assert_eq!(
            path_of(&Predicate::LessThan {
                path: "h".into(),
                value: 1.into()
            }),
            "h"
        );
        assert_eq!(
            path_of(&Predicate::Count {
                path: "i".into(),
                operator: CountOperator::Eq,
                value: 1,
            }),
            "i"
        );
        assert_eq!(path_of(&Predicate::IsEmpty { path: "j".into() }), "j");
        assert_eq!(path_of(&Predicate::IsNotEmpty { path: "k".into() }), "k");

        // Empty compounds have no leading path to expose.
        assert_eq!(path_of(&Predicate::All { predicates: vec![] }), "");
        assert_eq!(path_of(&Predicate::Any { predicates: vec![] }), "");
    }

    #[test]
    fn validate_ignores_empty_and_distinct_paths() {
        let contract = Contract::new("paths")
            .with_postcondition(Predicate::exists(""), "empty path is skipped")
            .with_postcondition(Predicate::exists(""), "second empty path")
            .with_postcondition(Predicate::exists("a.one"), "a")
            .with_postcondition(Predicate::exists("a.two"), "b");
        assert!(contract.validate().is_ok());
    }

    #[test]
    fn validate_rejects_zero_max_attempts() {
        let contract = valid_contract().with_recovery(recovery(RecoveryStrategy::Retry, 0, None));
        let err = contract.validate().expect_err("zero attempts");
        assert!(matches!(err, ContractValidationError::InvalidMaxAttempts));
        assert_eq!(err.to_string(), "max_attempts must be greater than 0");
    }

    #[test]
    fn validate_rejects_backoff_max_below_initial() {
        let backoff = BackoffConfig {
            backoff_type: BackoffType::Exponential,
            initial: chrono::Duration::seconds(30),
            max: chrono::Duration::seconds(1),
            multiplier: 2.0,
        };
        let contract =
            valid_contract().with_recovery(recovery(RecoveryStrategy::Retry, 3, Some(backoff)));
        let err = contract.validate().expect_err("backoff max < initial");
        assert!(matches!(
            err,
            ContractValidationError::InvalidBackoff { initial, max }
                if initial == chrono::Duration::seconds(30)
                    && max == chrono::Duration::seconds(1)
        ));
        assert_eq!(
            err.to_string(),
            "Backoff max (PT1S) must be >= initial (PT30S)"
        );
    }

    #[test]
    fn validate_rejects_non_positive_backoff_multiplier() {
        for multiplier in [0.0, -1.5] {
            let backoff = BackoffConfig {
                backoff_type: BackoffType::Linear,
                initial: chrono::Duration::seconds(1),
                max: chrono::Duration::seconds(10),
                multiplier,
            };
            let contract =
                valid_contract().with_recovery(recovery(RecoveryStrategy::Retry, 3, Some(backoff)));
            let err = contract.validate().expect_err("bad multiplier");
            assert_eq!(
                err.to_string(),
                format!("Backoff multiplier must be positive, got {multiplier}")
            );
        }
    }

    #[test]
    fn validate_accepts_zero_and_negative_multipliers_without_backoff() {
        // No backoff section means the multiplier is never consulted.
        let contract =
            valid_contract().with_recovery(recovery(RecoveryStrategy::Escalate, 1, None));
        assert!(contract.validate().is_ok());
    }

    #[test]
    fn validate_accepts_equal_backoff_bounds_and_positive_multiplier() {
        let backoff = BackoffConfig {
            backoff_type: BackoffType::Linear,
            initial: chrono::Duration::seconds(2),
            max: chrono::Duration::seconds(2),
            multiplier: 1.0,
        };
        let contract =
            valid_contract().with_recovery(recovery(RecoveryStrategy::Poll, 2, Some(backoff)));
        assert!(contract.validate().is_ok());
    }

    #[test]
    fn validation_errors_are_debug_and_cloneable() {
        let err = ContractValidationError::EmptyActionName;
        let cloned = err.clone();
        assert!(std::format!("{cloned:?}").contains("EmptyActionName"));
    }

    // ------------------------------------------------------------------
    // Serde
    // ------------------------------------------------------------------

    #[test]
    fn verification_config_field_defaults_on_deserialization() {
        let config: VerificationConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(config.consistency, ConsistencyMode::Strong);
        // `#[serde(default)]` on the `timeout` field falls back to
        // `Duration::default()` (zero) rather than `VerificationConfig::default()`.
        // Contracts that omit the whole `verification` section do get the struct
        // default, which is asserted in the contract-level deserialization test.
        assert_eq!(config.timeout, chrono::Duration::zero());
        assert_eq!(
            config.consistency,
            VerificationConfig::default().consistency
        );
    }

    #[test]
    fn verification_config_roundtrips_explicit_values() {
        let config = VerificationConfig {
            consistency: ConsistencyMode::Polling,
            timeout: chrono::Duration::seconds(30),
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains(r#""consistency":"polling""#), "{json}");
        assert!(json.contains(r#""timeout":[30,0]"#), "{json}");

        let back: VerificationConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back.consistency, ConsistencyMode::Polling);
        assert_eq!(back.timeout, chrono::Duration::seconds(30));
    }

    #[test]
    fn consistency_mode_roundtrips_all_variants() {
        for (mode, name) in [
            (ConsistencyMode::Strong, "strong"),
            (ConsistencyMode::Eventual, "eventual"),
            (ConsistencyMode::Polling, "polling"),
            (ConsistencyMode::Webhook, "webhook"),
        ] {
            let json = serde_json::to_string(&mode).unwrap();
            assert_eq!(json, format!(r#""{name}""#));
            let back: ConsistencyMode = serde_json::from_str(&json).unwrap();
            assert_eq!(back, mode);
        }
    }

    #[test]
    fn consistency_mode_default_is_strong() {
        assert_eq!(ConsistencyMode::default(), ConsistencyMode::Strong);
    }

    #[test]
    fn recovery_strategy_roundtrips_all_variants() {
        let pairs = [
            (RecoveryStrategy::NoAction, "no_action"),
            (RecoveryStrategy::Retry, "retry"),
            (RecoveryStrategy::VerifyThenRetry, "verify_then_retry"),
            (RecoveryStrategy::Poll, "poll"),
            (RecoveryStrategy::Compensate, "compensate"),
            (RecoveryStrategy::Rollback, "rollback"),
            (RecoveryStrategy::Escalate, "escalate"),
            (RecoveryStrategy::HumanApproval, "human_approval"),
            (RecoveryStrategy::Abort, "abort"),
        ];
        for (strategy, name) in pairs {
            let json = serde_json::to_string(&strategy).unwrap();
            assert_eq!(json, format!(r#""{name}""#));
            let back: RecoveryStrategy = serde_json::from_str(&json).unwrap();
            assert_eq!(back, strategy);
        }
    }

    #[test]
    fn backoff_type_roundtrips() {
        let linear: BackoffType = serde_json::from_str(r#""linear""#).unwrap();
        let exponential: BackoffType = serde_json::from_str(r#""exponential""#).unwrap();
        assert_eq!(linear, BackoffType::Linear);
        assert_eq!(exponential, BackoffType::Exponential);
        assert_eq!(BackoffType::default(), BackoffType::Linear);
    }

    #[test]
    fn recovery_config_applies_defaults_for_optional_fields() {
        // chrono durations serialize as `[secs, nanos]`.
        let json = r#"{
            "strategy": "verify_then_retry",
            "backoff": {"initial": [1, 0], "max": [10, 0]}
        }"#;
        let config: RecoveryConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.strategy, RecoveryStrategy::VerifyThenRetry);
        assert_eq!(config.max_attempts, 3, "max_attempts default is 3");
        assert!(config.on_unknown.is_empty());
        let backoff = config.backoff.expect("backoff present");
        assert_eq!(backoff.backoff_type, BackoffType::Linear);
        assert_eq!(backoff.initial, chrono::Duration::seconds(1));
        assert_eq!(backoff.max, chrono::Duration::seconds(10));
        assert!((backoff.multiplier - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn recovery_config_roundtrips() {
        let config = RecoveryConfig {
            strategy: RecoveryStrategy::Compensate,
            max_attempts: 7,
            backoff: Some(BackoffConfig {
                backoff_type: BackoffType::Exponential,
                initial: chrono::Duration::milliseconds(250),
                max: chrono::Duration::seconds(20),
                multiplier: 3.0,
            }),
            on_unknown: vec![
                RecoveryAction::Verify,
                RecoveryAction::Poll {
                    interval: chrono::Duration::seconds(2),
                    max_attempts: 4,
                },
                RecoveryAction::Alert {
                    severity: AlertSeverity::Critical,
                },
                RecoveryAction::RequireApproval,
            ],
        };

        let json = serde_json::to_string(&config).unwrap();
        let back: RecoveryConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back.strategy, RecoveryStrategy::Compensate);
        assert_eq!(back.max_attempts, 7);
        assert_eq!(json_of(&back.backoff), json_of(&config.backoff));
        assert_eq!(back.on_unknown.len(), 4);
    }

    #[test]
    fn recovery_action_uses_tagged_representation() {
        let verify: RecoveryAction = serde_json::from_str(r#"{"action":"verify"}"#).unwrap();
        assert!(matches!(verify, RecoveryAction::Verify));

        let approval: RecoveryAction =
            serde_json::from_str(r#"{"action":"require_approval"}"#).unwrap();
        assert!(matches!(approval, RecoveryAction::RequireApproval));

        let alert: RecoveryAction =
            serde_json::from_str(r#"{"action":"alert","severity":"warning"}"#).unwrap();
        assert!(matches!(
            alert,
            RecoveryAction::Alert {
                severity: AlertSeverity::Warning
            }
        ));

        let poll: RecoveryAction =
            serde_json::from_str(r#"{"action":"poll","interval":[2,0],"max_attempts":5}"#).unwrap();
        assert!(matches!(
            poll,
            RecoveryAction::Poll { interval, max_attempts }
                if interval == chrono::Duration::seconds(2) && max_attempts == 5
        ));

        let json = serde_json::to_string(&RecoveryAction::Verify).unwrap();
        assert_eq!(json, r#"{"action":"verify"}"#);
    }

    #[test]
    fn alert_severity_roundtrips_all_variants() {
        for (severity, name) in [
            (AlertSeverity::Info, "info"),
            (AlertSeverity::Warning, "warning"),
            (AlertSeverity::Error, "error"),
            (AlertSeverity::Critical, "critical"),
        ] {
            let json = serde_json::to_string(&severity).unwrap();
            assert_eq!(json, format!(r#""{name}""#));
            let back: AlertSeverity = serde_json::from_str(&json).unwrap();
            assert_eq!(back, severity);
        }
    }

    #[test]
    fn postcondition_defaults_to_mandatory() {
        let postcondition: Postcondition =
            serde_json::from_str(r#"{"predicate": {"type": "exists", "path": "a.b"}}"#).unwrap();
        assert!(postcondition.mandatory);
        assert_eq!(postcondition.description, "");
    }

    #[test]
    fn precondition_defaults_to_empty_description() {
        let precondition: Precondition =
            serde_json::from_str(r#"{"predicate": {"type": "exists", "path": "x"}}"#).unwrap();
        assert_eq!(precondition.description, "");
    }

    #[test]
    fn contract_roundtrips_through_json() {
        let contract = valid_contract()
            .with_precondition(Predicate::exists("account.id"), "account exists")
            .with_postcondition(
                Predicate::Count {
                    path: "refund.entries".into(),
                    operator: CountOperator::Ge,
                    value: 1,
                },
                "at least one entry",
            );

        let json = serde_json::to_string_pretty(&contract).unwrap();
        let back: Contract = serde_json::from_str(&json).unwrap();
        assert_eq!(back.schema_version, contract.schema_version);
        assert_eq!(back.id, contract.id);
        assert_eq!(back.action_name, contract.action_name);
        assert_eq!(back.postconditions.len(), 2);
        assert_eq!(back.preconditions.len(), 1);
        assert_eq!(
            json_of(&back.postconditions),
            json_of(&contract.postconditions)
        );
        assert_eq!(
            json_of(&back.preconditions),
            json_of(&contract.preconditions)
        );
        assert!(back.validate().is_ok());
    }

    #[test]
    fn contract_deserializes_with_only_required_fields() {
        let json = r#"{
            "action_name": "minimal",
            "postconditions": [
                {"predicate": {"type": "exists", "path": "result.id"}, "description": "created"}
            ]
        }"#;
        let contract: Contract = serde_json::from_str(json).unwrap();
        assert_eq!(contract.schema_version, "1.0");
        assert_eq!(contract.action_name, "minimal");
        assert!(contract.preconditions.is_empty());
        assert!(contract.recovery.is_none());
        assert_eq!(
            contract.verification.consistency,
            VerificationConfig::default().consistency
        );
        assert_eq!(
            contract.verification.timeout,
            VerificationConfig::default().timeout
        );
        assert!(contract.validate().is_ok());
    }

    #[test]
    fn contract_omitting_verification_section_uses_struct_default() {
        let json = r#"{
            "action_name": "default_verification",
            "postconditions": [
                {"predicate": {"type": "exists", "path": "ok"}, "description": "done"}
            ]
        }"#;
        let contract: Contract = serde_json::from_str(json).unwrap();
        assert_eq!(contract.verification.consistency, ConsistencyMode::Strong);
        assert_eq!(contract.verification.timeout, chrono::Duration::seconds(5));
    }

    #[test]
    fn contract_verification_section_overrides_the_defaults() {
        let json = r#"{
            "action_name": "explicit_verification",
            "postconditions": [
                {"predicate": {"type": "exists", "path": "ok"}, "description": "done"}
            ],
            "verification": {"consistency": "eventual", "timeout": [45, 0]},
            "preconditions": [],
            "recovery": null,
            "schema_version": "1.0"
        }"#;
        let contract: Contract = serde_json::from_str(json).unwrap();
        assert_eq!(contract.verification.consistency, ConsistencyMode::Eventual);
        assert_eq!(contract.verification.timeout, chrono::Duration::seconds(45));
        assert!(contract.recovery.is_none());
    }
}
