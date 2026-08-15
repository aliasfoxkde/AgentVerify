//! Contract parsing and validation
//!
//! Supports JSON and YAML contract files.

use agentverify_core::{Contract, Predicate};
use std::fs;
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ContractError {
    #[error("Failed to parse JSON: {0}")]
    JsonError(#[from] serde_json::Error),
    #[error("Failed to parse YAML: {0}")]
    YamlError(#[from] serde_yaml::Error),
    #[error("Failed to read file: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Invalid contract: {0}")]
    InvalidContract(String),
}

/// Parse a contract from JSON string
pub fn parse_json(json: &str) -> Result<Contract, ContractError> {
    let contract: Contract = serde_json::from_str(json)?;
    validate_contract(&contract)?;
    Ok(contract)
}

/// Parse a contract from YAML string
pub fn parse_yaml(yaml: &str) -> Result<Contract, ContractError> {
    let contract: Contract = serde_yaml::from_str(yaml)?;
    validate_contract(&contract)?;
    Ok(contract)
}

/// Load a contract from a file (auto-detects format by extension)
pub fn load_file(path: impl AsRef<Path>) -> Result<Contract, ContractError> {
    let content = fs::read_to_string(path.as_ref())?;
    let path = path.as_ref();

    if let Some(ext) = path.extension() {
        match ext.to_str() {
            Some("json") => parse_json(&content),
            Some("yaml") | Some("yml") => parse_yaml(&content),
            _ => Err(ContractError::InvalidContract(format!(
                "Unknown file extension: {:?}",
                ext
            ))),
        }
    } else {
        // Try JSON first, then YAML
        parse_json(&content).or_else(|_| parse_yaml(&content))
    }
}

/// Validate a contract for basic correctness
pub fn validate_contract(contract: &Contract) -> Result<(), ContractError> {
    if contract.action_name.is_empty() {
        return Err(ContractError::InvalidContract(
            "action_name cannot be empty".into(),
        ));
    }

    // Contract must have at least one postcondition
    if contract.postconditions.is_empty() {
        return Err(ContractError::InvalidContract(
            "Contract must have at least one postcondition".into(),
        ));
    }

    // Validate each postcondition has a valid predicate
    for (i, postcond) in contract.postconditions.iter().enumerate() {
        validate_predicate(&postcond.predicate, &format!("postconditions[{}]", i))?;
    }

    // Validate each precondition
    for (i, precond) in contract.preconditions.iter().enumerate() {
        validate_predicate(&precond.predicate, &format!("preconditions[{}]", i))?;
    }

    Ok(())
}

fn validate_predicate(predicate: &Predicate, path: &str) -> Result<(), ContractError> {
    match predicate {
        Predicate::All { predicates } if predicates.is_empty() => {
            Err(ContractError::InvalidContract(format!(
                "{}: All predicate must have at least one predicate",
                path
            )))
        }
        Predicate::Any { predicates } if predicates.is_empty() => {
            Err(ContractError::InvalidContract(format!(
                "{}: Any predicate must have at least one predicate",
                path
            )))
        }
        Predicate::All { predicates } => {
            for (i, p) in predicates.iter().enumerate() {
                validate_predicate(p, &format!("{}[{}]", path, i))?;
            }
            Ok(())
        }
        Predicate::Any { predicates } => {
            for (i, p) in predicates.iter().enumerate() {
                validate_predicate(p, &format!("{}[{}]", path, i))?;
            }
            Ok(())
        }
        Predicate::Not { predicate } => validate_predicate(predicate, &format!("{}.not", path)),
        Predicate::Implies {
            antecedent,
            consequent,
        } => {
            validate_predicate(antecedent, &format!("{}.antecedent", path))?;
            validate_predicate(consequent, &format!("{}.consequent", path))
        }
        _ => Ok(()),
    }
}

/// Convert a contract to JSON string
pub fn to_json(contract: &Contract) -> Result<String, ContractError> {
    Ok(serde_json::to_string_pretty(contract)?)
}

/// Convert a contract to YAML string
pub fn to_yaml(contract: &Contract) -> Result<String, ContractError> {
    Ok(serde_yaml::to_string(contract)?)
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
        let yaml = r#"
action_name: create_customer
postconditions:
  - predicate:
      type: exists
      path: customer.id
    description: Customer was created
"#;

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
        let result = validate_predicate(&predicate, "test");
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
        let yaml = r#"
action_name: test_action
postconditions:
  - predicate:
      type: exists
      path: x
    description: x exists
"#;
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
        let result = validate_predicate(&predicate, "test");
        assert!(result.is_err());
    }

    #[test]
    fn validate_nested_compound_predicates() {
        let predicate = Predicate::all(vec![
            Predicate::any(vec![Predicate::exists("a"), Predicate::exists("b")]),
            Predicate::not_exists("c"),
        ]);
        let result = validate_predicate(&predicate, "test");
        assert!(result.is_ok());
    }

    #[test]
    fn validate_implies_predicate() {
        let predicate = Predicate::Implies {
            antecedent: Box::new(Predicate::exists("a")),
            consequent: Box::new(Predicate::exists("b")),
        };
        let result = validate_predicate(&predicate, "test");
        assert!(result.is_ok());
    }

    #[test]
    fn validate_nested_empty_all_in_any() {
        // Any with an empty All should fail
        let predicate = Predicate::any(vec![
            Predicate::all(vec![]), // Invalid
            Predicate::exists("a"),
        ]);
        let result = validate_predicate(&predicate, "test");
        assert!(result.is_err());
    }
}
