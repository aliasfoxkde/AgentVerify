//! Contract parsing and validation
//!
//! Supports JSON and YAML contract files.

use agentverify_core::{Contract, ContractId, Predicate};
use std::fs;
use std::path::Path;

/// Source location for error context
#[derive(Debug, Clone)]
pub struct SourceLocation {
    /// File path where the error originated
    pub file: String,
    /// Line number (1-indexed)
    pub line: u32,
    /// Optional column number (1-indexed)
    pub column: Option<u32>,
}

impl std::fmt::Display for SourceLocation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.file, self.line)?;
        if let Some(col) = self.column {
            write!(f, ":{col}")?;
        }
        Ok(())
    }
}

impl SourceLocation {
    /// Create a new source location from file and line
    pub fn new(file: impl Into<String>, line: u32) -> Self {
        Self {
            file: file.into(),
            line,
            column: None,
        }
    }

    /// Create with column info
    pub fn with_column(file: impl Into<String>, line: u32, column: u32) -> Self {
        Self {
            file: file.into(),
            line,
            column: Some(column),
        }
    }
}

/// Context for contract validation errors
#[derive(Debug, Clone)]
pub struct ContractContext {
    /// Contract ID if available
    pub contract_id: Option<ContractId>,
    /// Action name for the contract
    pub action_name: Option<String>,
}

impl ContractContext {
    /// Create with just an action name
    pub fn with_action(action: impl Into<String>) -> Self {
        Self {
            contract_id: None,
            action_name: Some(action.into()),
        }
    }

    /// Create with contract ID
    #[must_use]
    pub fn with_contract_id(contract_id: ContractId) -> Self {
        Self {
            contract_id: Some(contract_id),
            action_name: None,
        }
    }
}

impl std::fmt::Display for ContractContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut parts = Vec::new();
        if let Some(id) = &self.contract_id {
            parts.push(format!("contract_id={id}"));
        }
        if let Some(name) = &self.action_name {
            parts.push(format!("action_name={name}"));
        }
        write!(f, "{}", parts.join(", "))
    }
}

/// Predicate path context for nested predicate errors
#[derive(Debug, Clone)]
pub struct PredicatePath {
    /// The path to the predicate (e.g., "postconditions\[0\]", "preconditions\[2\].not").
    /// Note: brackets are literal here, not array indexing syntax.
    pub path: String,
}

impl std::fmt::Display for PredicatePath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "at {}", self.path)
    }
}

/// Helper function to format context for error messages
fn fmt_ctx(
    f: &mut std::fmt::Formatter<'_>,
    location: Option<&SourceLocation>,
    context: Option<&ContractContext>,
) -> std::fmt::Result {
    let mut ctx_parts = Vec::new();
    if let Some(loc) = location {
        ctx_parts.push(format!(" ({loc})"));
    }
    if let Some(c) = context {
        let ctx_str = c.to_string();
        if !ctx_str.is_empty() {
            ctx_parts.push(format!(" [{ctx_str}]"));
        }
    }
    write!(f, "{}", ctx_parts.join(""))
}

/// Errors raised while parsing, loading, or validating a contract.
#[derive(Debug)]
pub enum ContractError {
    /// The contract JSON could not be deserialized.
    JsonError {
        /// Underlying `serde_json` error.
        source: serde_json::Error,
        /// Input location the error was attributed to, if known.
        location: Option<SourceLocation>,
        /// Contract context (action name, contract id) for the error.
        context: Option<ContractContext>,
    },
    /// The contract YAML could not be deserialized.
    YamlError {
        /// Underlying `serde_yaml` error.
        source: serde_yaml::Error,
        /// Input location the error was attributed to, if known.
        location: Option<SourceLocation>,
        /// Contract context (action name, contract id) for the error.
        context: Option<ContractContext>,
    },
    /// The contract file could not be read.
    IoError {
        /// Underlying I/O error.
        source: std::io::Error,
        /// File location the error was attributed to, if known.
        location: Option<SourceLocation>,
        /// Contract context (action name, contract id) for the error.
        context: Option<ContractContext>,
    },
    /// The contract is structurally valid but violates a validation rule.
    InvalidContract {
        /// Human-readable description of the violation.
        reason: String,
        /// Input location the error was attributed to, if known.
        location: Option<SourceLocation>,
        /// Contract context (action name, contract id) for the error.
        context: Option<ContractContext>,
    },
    /// A predicate inside the contract is not valid.
    InvalidPredicate {
        /// Human-readable description of the violation.
        reason: String,
        /// Path of the offending predicate inside the contract.
        path: PredicatePath,
        /// Contract context (action name, contract id) for the error.
        context: Option<ContractContext>,
    },
    /// The contract file has an extension this parser does not handle.
    UnknownFileExtension {
        /// The unrecognized extension.
        extension: String,
        /// File location the error was attributed to.
        location: Option<SourceLocation>,
        /// Contract context (action name, contract id) for the error.
        context: Option<ContractContext>,
    },
}

impl std::fmt::Display for ContractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ContractError::JsonError {
                source,
                location,
                context,
            } => {
                write!(f, "Failed to parse JSON")?;
                fmt_ctx(f, location.as_ref(), context.as_ref())?;
                write!(f, ": {source}")
            }
            ContractError::YamlError {
                source,
                location,
                context,
            } => {
                write!(f, "Failed to parse YAML")?;
                fmt_ctx(f, location.as_ref(), context.as_ref())?;
                write!(f, ": {source}")
            }
            ContractError::IoError {
                source,
                location,
                context,
            } => {
                write!(f, "Failed to read file: {source}")?;
                fmt_ctx(f, location.as_ref(), context.as_ref())
            }
            ContractError::InvalidContract {
                reason,
                location,
                context,
            } => {
                write!(f, "Invalid contract")?;
                fmt_ctx(f, location.as_ref(), context.as_ref())?;
                write!(f, ": {reason}")
            }
            ContractError::InvalidPredicate {
                reason,
                path,
                context,
            } => {
                write!(f, "Invalid predicate")?;
                if let Some(c) = context {
                    let ctx_str = c.to_string();
                    if !ctx_str.is_empty() {
                        write!(f, " [{ctx_str}]")?;
                    }
                }
                write!(f, " {path}: {reason}")
            }
            ContractError::UnknownFileExtension {
                extension,
                location,
                context,
            } => {
                write!(f, "Unknown file extension: {extension}")?;
                fmt_ctx(f, location.as_ref(), context.as_ref())
            }
        }
    }
}

impl std::error::Error for ContractError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ContractError::JsonError { source, .. } => Some(source),
            ContractError::YamlError { source, .. } => Some(source),
            ContractError::IoError { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl ContractError {
    /// Add source location to the error
    #[must_use]
    pub fn with_location(self, location: SourceLocation) -> Self {
        match self {
            ContractError::JsonError {
                source,
                location: None,
                context,
            } => ContractError::JsonError {
                source,
                location: Some(location),
                context,
            },
            ContractError::YamlError {
                source,
                location: None,
                context,
            } => ContractError::YamlError {
                source,
                location: Some(location),
                context,
            },
            ContractError::IoError {
                source,
                location: None,
                context,
            } => ContractError::IoError {
                source,
                location: Some(location),
                context,
            },
            ContractError::InvalidContract {
                reason,
                location: None,
                context,
            } => ContractError::InvalidContract {
                reason,
                location: Some(location),
                context,
            },
            ContractError::UnknownFileExtension {
                extension,
                location: None,
                context,
            } => ContractError::UnknownFileExtension {
                extension,
                location: Some(location),
                context,
            },
            other => other,
        }
    }

    /// Add contract context to the error
    #[must_use]
    pub fn with_context(self, context: ContractContext) -> Self {
        match self {
            ContractError::JsonError {
                source,
                location,
                context: None,
            } => ContractError::JsonError {
                source,
                location,
                context: Some(context),
            },
            ContractError::YamlError {
                source,
                location,
                context: None,
            } => ContractError::YamlError {
                source,
                location,
                context: Some(context),
            },
            ContractError::IoError {
                source,
                location,
                context: None,
            } => ContractError::IoError {
                source,
                location,
                context: Some(context),
            },
            ContractError::InvalidContract {
                reason,
                location,
                context: None,
            } => ContractError::InvalidContract {
                reason,
                location,
                context: Some(context),
            },
            ContractError::InvalidPredicate {
                reason,
                path,
                context: None,
            } => ContractError::InvalidPredicate {
                reason,
                path,
                context: Some(context),
            },
            ContractError::UnknownFileExtension {
                extension,
                location,
                context: None,
            } => ContractError::UnknownFileExtension {
                extension,
                location,
                context: Some(context),
            },
            other => other,
        }
    }

    /// Get the contract ID if available
    #[must_use]
    pub fn contract_id(&self) -> Option<&ContractId> {
        match self {
            ContractError::JsonError { context, .. }
            | ContractError::YamlError { context, .. }
            | ContractError::IoError { context, .. }
            | ContractError::InvalidContract { context, .. }
            | ContractError::InvalidPredicate { context, .. }
            | ContractError::UnknownFileExtension { context, .. } => {
                context.as_ref().and_then(|c| c.contract_id.as_ref())
            }
        }
    }

    /// Get the action name if available
    #[must_use]
    pub fn action_name(&self) -> Option<&str> {
        match self {
            ContractError::JsonError { context, .. }
            | ContractError::YamlError { context, .. }
            | ContractError::IoError { context, .. }
            | ContractError::InvalidContract { context, .. }
            | ContractError::InvalidPredicate { context, .. }
            | ContractError::UnknownFileExtension { context, .. } => {
                context.as_ref().and_then(|c| c.action_name.as_deref())
            }
        }
    }
}

/// Parse a contract from JSON string
///
/// # Errors
///
/// Returns [`ContractError::JsonError`] if the input is not valid JSON and
/// [`ContractError::InvalidContract`] / [`ContractError::InvalidPredicate`] if
/// validation fails.
pub fn parse_json(json: &str) -> Result<Contract, ContractError> {
    let contract: Contract = serde_json::from_str(json).map_err(|e| ContractError::JsonError {
        source: e,
        location: None,
        context: None,
    })?;
    validate_contract(&contract)?;
    Ok(contract)
}

/// Parse a contract from YAML string
///
/// # Errors
///
/// Returns [`ContractError::YamlError`] if the input is not valid YAML and
/// [`ContractError::InvalidContract`] / [`ContractError::InvalidPredicate`] if
/// validation fails.
pub fn parse_yaml(yaml: &str) -> Result<Contract, ContractError> {
    let contract: Contract = serde_yaml::from_str(yaml).map_err(|e| ContractError::YamlError {
        source: e,
        location: None,
        context: None,
    })?;
    validate_contract(&contract)?;
    Ok(contract)
}

/// Load a contract from a file (auto-detects format by extension)
///
/// # Errors
///
/// Returns [`ContractError::IoError`] if the file cannot be read,
/// [`ContractError::UnknownFileExtension`] if the extension is not `.json`,
/// `.yaml`, or `.yml`, or the parse/validation errors from
/// [`parse_json`] / [`parse_yaml`] otherwise.
pub fn load_file(path: impl AsRef<Path>) -> Result<Contract, ContractError> {
    let content = fs::read_to_string(path.as_ref()).map_err(|e| ContractError::IoError {
        source: e,
        location: Some(SourceLocation::new(path.as_ref().display().to_string(), 1)),
        context: None,
    })?;
    let path = path.as_ref();

    if let Some(ext) = path.extension() {
        match ext.to_str() {
            Some("json") => parse_json(&content)
                .map_err(|e| e.with_location(SourceLocation::new(path.display().to_string(), 1))),
            Some("yaml" | "yml") => parse_yaml(&content)
                .map_err(|e| e.with_location(SourceLocation::new(path.display().to_string(), 1))),
            _ => Err(ContractError::UnknownFileExtension {
                extension: ext.to_string_lossy().into_owned(),
                location: Some(SourceLocation::new(path.display().to_string(), 1)),
                context: None,
            }),
        }
    } else {
        // Try JSON first, then YAML
        parse_json(&content)
            .or_else(|_| parse_yaml(&content))
            .map_err(|e| e.with_location(SourceLocation::new(path.display().to_string(), 1)))
    }
}

/// Validate a contract for basic correctness
///
/// # Errors
///
/// Returns [`ContractError::InvalidContract`] when required fields are missing
/// and [`ContractError::InvalidPredicate`] when any predicate is malformed.
pub fn validate_contract(contract: &Contract) -> Result<(), ContractError> {
    let ctx = ContractContext::with_action(&contract.action_name);

    if contract.action_name.is_empty() {
        return Err(ContractError::InvalidContract {
            reason: "action_name cannot be empty".into(),
            location: None,
            context: Some(ctx),
        });
    }

    // Contract must have at least one postcondition
    if contract.postconditions.is_empty() {
        return Err(ContractError::InvalidContract {
            reason: "Contract must have at least one postcondition".into(),
            location: None,
            context: Some(ctx),
        });
    }

    // Validate each postcondition has a valid predicate
    for (i, postcond) in contract.postconditions.iter().enumerate() {
        validate_predicate(
            &postcond.predicate,
            &PredicatePath {
                path: format!("postconditions[{i}]"),
            },
            Some(ctx.clone()),
        )?;
    }

    // Validate each precondition
    for (i, precond) in contract.preconditions.iter().enumerate() {
        validate_predicate(
            &precond.predicate,
            &PredicatePath {
                path: format!("preconditions[{i}]"),
            },
            Some(ctx.clone()),
        )?;
    }

    Ok(())
}

fn validate_predicate(
    predicate: &Predicate,
    path: &PredicatePath,
    ctx: Option<ContractContext>,
) -> Result<(), ContractError> {
    match predicate {
        Predicate::All { predicates } if predicates.is_empty() => {
            Err(ContractError::InvalidPredicate {
                reason: "All predicate must have at least one predicate".into(),
                path: path.clone(),
                context: ctx,
            })
        }
        Predicate::Any { predicates } if predicates.is_empty() => {
            Err(ContractError::InvalidPredicate {
                reason: "Any predicate must have at least one predicate".into(),
                path: path.clone(),
                context: ctx,
            })
        }
        Predicate::All { predicates } | Predicate::Any { predicates } => {
            for (i, p) in predicates.iter().enumerate() {
                validate_predicate(
                    p,
                    &PredicatePath {
                        path: format!("{}[{}]", path.path, i),
                    },
                    ctx.clone(),
                )?;
            }
            Ok(())
        }
        Predicate::Not { predicate } => validate_predicate(
            predicate,
            &PredicatePath {
                path: format!("{}.not", path.path),
            },
            ctx,
        ),
        Predicate::Implies {
            antecedent,
            consequent,
        } => {
            validate_predicate(
                antecedent,
                &PredicatePath {
                    path: format!("{}.antecedent", path.path),
                },
                ctx.clone(),
            )?;
            validate_predicate(
                consequent,
                &PredicatePath {
                    path: format!("{}.consequent", path.path),
                },
                ctx,
            )
        }
        _ => Ok(()),
    }
}

/// Convert a contract to JSON string
///
/// # Errors
///
/// Returns [`ContractError::JsonError`] if the contract cannot be serialized.
pub fn to_json(contract: &Contract) -> Result<String, ContractError> {
    serde_json::to_string_pretty(contract).map_err(|e| ContractError::JsonError {
        source: e,
        location: None,
        context: None,
    })
}

/// Convert a contract to YAML string
///
/// # Errors
///
/// Returns [`ContractError::YamlError`] if the contract cannot be serialized.
pub fn to_yaml(contract: &Contract) -> Result<String, ContractError> {
    serde_yaml::to_string(contract).map_err(|e| ContractError::YamlError {
        source: e,
        location: None,
        context: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentverify_core::Predicate;

    #[test]
    fn parse_json_contract() {
        let json = r#"{
            "action_name": "create_customer",
            "postconditions": [
                {
                    "predicate": {"type": "exists", "path": "customer.id"},
                    "description": "Customer was created"
                }
            ]
        }"#;

        let contract = parse_json(json).unwrap();
        assert_eq!(contract.action_name, "create_customer");
        assert_eq!(contract.postconditions.len(), 1);
    }

    #[test]
    fn parse_yaml_contract() {
        let yaml = r"
action_name: create_customer
postconditions:
  - predicate:
      type: exists
      path: customer.id
    description: Customer was created
";

        let contract = parse_yaml(yaml).unwrap();
        assert_eq!(contract.action_name, "create_customer");
        assert_eq!(contract.postconditions.len(), 1);
    }

    #[test]
    fn validate_contract_empty_action_name() {
        let contract = Contract::new("");
        let result = validate_contract(&contract);
        assert!(result.is_err());
    }

    #[test]
    fn validate_contract_no_postconditions() {
        let contract = Contract::new("test");
        let result = validate_contract(&contract);
        assert!(result.is_err());
    }

    #[test]
    fn validate_contract_valid() {
        let contract = Contract::new("test").with_postcondition(Predicate::exists("x"), "x exists");
        let result = validate_contract(&contract);
        assert!(result.is_ok());
    }

    #[test]
    fn validate_empty_all_predicate() {
        let predicate = Predicate::all(vec![]);
        let result = validate_predicate(
            &predicate,
            &PredicatePath {
                path: "test".to_string(),
            },
            None,
        );
        assert!(result.is_err());
    }

    #[test]
    fn roundtrip_json() {
        let contract = Contract::new("test").with_postcondition(Predicate::exists("x"), "x exists");
        let json = to_json(&contract).unwrap();
        let parsed = parse_json(&json).unwrap();
        assert_eq!(parsed.action_name, contract.action_name);
    }

    // === Schema version tests ===

    #[test]
    fn parse_contract_default_schema_version() {
        let json = r#"{
            "action_name": "test_action",
            "postconditions": [
                {"predicate": {"type": "exists", "path": "x"}, "description": "x exists"}
            ]
        }"#;
        let contract = parse_json(json).unwrap();
        assert_eq!(contract.schema_version, "1.0");
    }

    #[test]
    fn parse_contract_explicit_schema_version() {
        let json = r#"{
            "schema_version": "1.0",
            "action_name": "test_action",
            "postconditions": [
                {"predicate": {"type": "exists", "path": "x"}, "description": "x exists"}
            ]
        }"#;
        let contract = parse_json(json).unwrap();
        assert_eq!(contract.schema_version, "1.0");
    }

    #[test]
    fn parse_yaml_contract_with_schema() {
        let yaml = r"
action_name: test_action
postconditions:
  - predicate:
      type: exists
      path: x
    description: x exists
";
        let contract = parse_yaml(yaml).unwrap();
        assert_eq!(contract.action_name, "test_action");
        assert_eq!(contract.postconditions.len(), 1);
    }

    // === Contract validation edge cases ===

    #[test]
    fn validate_contract_duplicate_postcondition_paths() {
        let json = r#"{
            "action_name": "test",
            "postconditions": [
                {"predicate": {"type": "exists", "path": "x"}, "description": "first"},
                {"predicate": {"type": "exists", "path": "x"}, "description": "duplicate"}
            ]
        }"#;
        let contract = parse_json(json).unwrap();
        let result = contract.validate();
        assert!(result.is_err());
    }

    #[test]
    fn validate_contract_recovery_max_attempts_zero() {
        // Parse a contract with max_attempts: 0 via JSON
        let json = r#"{
            "action_name": "test",
            "postconditions": [
                {"predicate": {"type": "exists", "path": "x"}, "description": "x exists"}
            ],
            "recovery": {
                "strategy": "verify_then_retry",
                "max_attempts": 0
            }
        }"#;
        let contract = parse_json(json).unwrap();
        let result = contract.validate();
        assert!(result.is_err());
    }

    #[test]
    fn validate_contract_recovery_backoff_max_less_than_initial() {
        // Parse via JSON with backoff.max < backoff.initial
        let json = r#"{
            "action_name": "test",
            "postconditions": [
                {"predicate": {"type": "exists", "path": "x"}, "description": "x exists"}
            ],
            "recovery": {
                "strategy": "verify_then_retry",
                "max_attempts": 3,
                "backoff": {
                    "backoff_type": "exponential",
                    "initial": [10, 0],
                    "max": [5, 0],
                    "multiplier": 2.0
                }
            }
        }"#;
        let contract = parse_json(json).unwrap();
        let result = contract.validate();
        // This should fail validation because max < initial
        assert!(result.is_err());
    }

    #[test]
    fn validate_contract_recovery_backoff_negative_multiplier() {
        let json = r#"{
            "action_name": "test",
            "postconditions": [
                {"predicate": {"type": "exists", "path": "x"}, "description": "x exists"}
            ],
            "recovery": {
                "strategy": "verify_then_retry",
                "max_attempts": 3,
                "backoff": {
                    "backoff_type": "exponential",
                    "initial": [1, 0],
                    "max": [60, 0],
                    "multiplier": -1.0
                }
            }
        }"#;
        let contract = parse_json(json).unwrap();
        let result = contract.validate();
        assert!(result.is_err());
    }

    #[test]
    fn validate_contract_valid_with_recovery() {
        let json = r#"{
            "action_name": "test",
            "postconditions": [
                {"predicate": {"type": "exists", "path": "x"}, "description": "x exists"}
            ],
            "recovery": {
                "strategy": "verify_then_retry",
                "max_attempts": 3,
                "backoff": {
                    "backoff_type": "exponential",
                    "initial": [1, 0],
                    "max": [60, 0],
                    "multiplier": 2.0
                }
            }
        }"#;
        let contract = parse_json(json).unwrap();
        let result = contract.validate();
        assert!(result.is_ok());
    }

    #[test]
    fn validate_contract_incompatible_schema_version() {
        let json = r#"{
            "schema_version": "2.0",
            "action_name": "test",
            "postconditions": [
                {"predicate": {"type": "exists", "path": "x"}, "description": "x exists"}
            ]
        }"#;
        let contract = parse_json(json).unwrap();
        let result = contract.validate();
        assert!(result.is_err());
    }

    #[test]
    fn validate_contract_invalid_schema_version() {
        let json = r#"{
            "schema_version": "invalid",
            "action_name": "test",
            "postconditions": [
                {"predicate": {"type": "exists", "path": "x"}, "description": "x exists"}
            ]
        }"#;
        let contract = parse_json(json).unwrap();
        let result = contract.validate();
        assert!(result.is_err());
    }

    // === Compound predicate validation ===

    #[test]
    fn validate_empty_any_predicate() {
        let predicate = Predicate::any(vec![]);
        let result = validate_predicate(
            &predicate,
            &PredicatePath {
                path: "test".to_string(),
            },
            None,
        );
        assert!(result.is_err());
    }

    #[test]
    fn validate_nested_compound_predicates() {
        let predicate = Predicate::all(vec![
            Predicate::any(vec![Predicate::exists("a"), Predicate::exists("b")]),
            Predicate::not_exists("c"),
        ]);
        let result = validate_predicate(
            &predicate,
            &PredicatePath {
                path: "test".to_string(),
            },
            None,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn validate_implies_predicate() {
        let predicate = Predicate::Implies {
            antecedent: Box::new(Predicate::exists("a")),
            consequent: Box::new(Predicate::exists("b")),
        };
        let result = validate_predicate(
            &predicate,
            &PredicatePath {
                path: "test".to_string(),
            },
            None,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn validate_nested_empty_all_in_any() {
        // Any with an empty All should fail
        let predicate = Predicate::any(vec![
            Predicate::all(vec![]), // Invalid
            Predicate::exists("a"),
        ]);
        let result = validate_predicate(
            &predicate,
            &PredicatePath {
                path: "test".to_string(),
            },
            None,
        );
        assert!(result.is_err());
    }
}
