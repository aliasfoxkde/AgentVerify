//! Predicate engine implementation
//!
//! Deterministic predicate evaluation for verification conditions.

use agentverify_core::{CountOperator, Predicate, VerificationResult};
use regex::Regex;
use serde_json::Value;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("Invalid path: {0}")]
    InvalidPath(String),
    #[error("Type mismatch: {0}")]
    TypeMismatch(String),
    #[error("Evaluation failed: {0}")]
    EvaluationFailed(String),
    #[error("Regex error: {0}")]
    RegexError(#[from] regex::Error),
}

/// Predicate evaluation engine
pub struct PredicateEngine;

impl PredicateEngine {
    /// Evaluate a predicate against observed state
    pub fn evaluate(
        predicate: &Predicate,
        state: &Value,
        args: &Value,
    ) -> Result<VerificationResult, EngineError> {
        match predicate {
            Predicate::Exists { path } => Self::eval_exists(state, path),
            Predicate::NotExists { path } => Self::eval_not_exists(state, path),
            Predicate::Equals { path, value } => Self::eval_equals(state, path, value, args),
            Predicate::NotEquals { path, value } => Self::eval_not_equals(state, path, value, args),
            Predicate::Contains { path, value } => Self::eval_contains(state, path, value, args),
            Predicate::Matches { path, pattern } => Self::eval_matches(state, path, pattern),
            Predicate::GreaterThan { path, value } => {
                Self::eval_greater_than(state, path, value, args)
            }
            Predicate::LessThan { path, value } => Self::eval_less_than(state, path, value, args),
            Predicate::Count {
                path,
                operator,
                value,
            } => Self::eval_count(state, path, *operator, *value),
            Predicate::IsEmpty { path } => Self::eval_is_empty(state, path),
            Predicate::IsNotEmpty { path } => Self::eval_is_not_empty(state, path),
            Predicate::All { predicates } => Self::eval_all(predicates, state, args),
            Predicate::Any { predicates } => Self::eval_any(predicates, state, args),
            Predicate::Not { predicate } => Self::eval_not(predicate, state, args),
            Predicate::Implies {
                antecedent,
                consequent,
            } => Self::eval_implies(antecedent, consequent, state, args),
        }
    }

    fn eval_exists(state: &Value, path: &str) -> Result<VerificationResult, EngineError> {
        if get_path(state, path).is_some() {
            Ok(VerificationResult::Verified)
        } else {
            Ok(VerificationResult::Failed)
        }
    }

    fn eval_not_exists(state: &Value, path: &str) -> Result<VerificationResult, EngineError> {
        if get_path(state, path).is_none() {
            Ok(VerificationResult::Verified)
        } else {
            Ok(VerificationResult::Failed)
        }
    }

    fn eval_equals(
        state: &Value,
        path: &str,
        expected: &Value,
        args: &Value,
    ) -> Result<VerificationResult, EngineError> {
        let actual = get_path(state, path);
        let expected = resolve_value(expected, args);

        match actual {
            Some(actual_val) if actual_val == &expected => Ok(VerificationResult::Verified),
            Some(_) => Ok(VerificationResult::Failed),
            None => Ok(VerificationResult::Failed),
        }
    }

    fn eval_not_equals(
        state: &Value,
        path: &str,
        value: &Value,
        args: &Value,
    ) -> Result<VerificationResult, EngineError> {
        let actual = get_path(state, path);
        let value = resolve_value(value, args);

        match actual {
            Some(actual_val) if actual_val != &value => Ok(VerificationResult::Verified),
            _ => Ok(VerificationResult::Failed),
        }
    }

    fn eval_contains(
        state: &Value,
        path: &str,
        value: &Value,
        args: &Value,
    ) -> Result<VerificationResult, EngineError> {
        let actual = get_path(state, path);
        let value = resolve_value(value, args);

        match actual {
            Some(Value::String(s)) if s.contains(value.as_str().unwrap_or("")) => {
                Ok(VerificationResult::Verified)
            }
            Some(Value::Array(arr)) if arr.contains(&value) => Ok(VerificationResult::Verified),
            Some(Value::Object(obj)) => {
                if let Ok(obj_str) = serde_json::to_string(obj) {
                    if let Some(val_str) = value.as_str() {
                        if obj_str.contains(val_str) {
                            return Ok(VerificationResult::Verified);
                        }
                    }
                }
                Ok(VerificationResult::Failed)
            }
            _ => Ok(VerificationResult::Failed),
        }
    }

    fn eval_matches(
        state: &Value,
        path: &str,
        pattern: &str,
    ) -> Result<VerificationResult, EngineError> {
        let actual = get_path(state, path);

        match actual {
            Some(Value::String(s)) => {
                let regex = Regex::new(pattern)?;
                if regex.is_match(s) {
                    Ok(VerificationResult::Verified)
                } else {
                    Ok(VerificationResult::Failed)
                }
            }
            _ => Ok(VerificationResult::Failed),
        }
    }

    fn eval_greater_than(
        state: &Value,
        path: &str,
        value: &Value,
        args: &Value,
    ) -> Result<VerificationResult, EngineError> {
        let actual = get_path(state, path);
        let value = resolve_value(value, args);

        match (actual, value) {
            (Some(Value::Number(actual_num)), Value::Number(expected_num)) => {
                match (actual_num.as_f64(), expected_num.as_f64()) {
                    (Some(a), Some(e)) if a > e => Ok(VerificationResult::Verified),
                    _ => Ok(VerificationResult::Failed),
                }
            }
            (Some(Value::String(actual_str)), Value::String(expected_str)) => {
                if actual_str.as_str() > expected_str.as_str() {
                    Ok(VerificationResult::Verified)
                } else {
                    Ok(VerificationResult::Failed)
                }
            }
            _ => Ok(VerificationResult::Failed),
        }
    }

    fn eval_less_than(
        state: &Value,
        path: &str,
        value: &Value,
        args: &Value,
    ) -> Result<VerificationResult, EngineError> {
        let actual = get_path(state, path);
        let value = resolve_value(value, args);

        match (actual, value) {
            (Some(Value::Number(actual_num)), Value::Number(expected_num)) => {
                match (actual_num.as_f64(), expected_num.as_f64()) {
                    (Some(a), Some(e)) if a < e => Ok(VerificationResult::Verified),
                    _ => Ok(VerificationResult::Failed),
                }
            }
            (Some(Value::String(actual_str)), Value::String(expected_str)) => {
                if actual_str.as_str() < expected_str.as_str() {
                    Ok(VerificationResult::Verified)
                } else {
                    Ok(VerificationResult::Failed)
                }
            }
            _ => Ok(VerificationResult::Failed),
        }
    }

    fn eval_count(
        state: &Value,
        path: &str,
        operator: CountOperator,
        expected: i64,
    ) -> Result<VerificationResult, EngineError> {
        let actual = get_path(state, path);

        let count = match actual {
            Some(Value::Array(arr)) => arr.len() as i64,
            Some(Value::Object(obj)) => obj.len() as i64,
            Some(Value::String(s)) => s.len() as i64,
            None => 0,
            _ => return Ok(VerificationResult::Failed),
        };

        let result = match operator {
            CountOperator::Eq => count == expected,
            CountOperator::Ne => count != expected,
            CountOperator::Lt => count < expected,
            CountOperator::Le => count <= expected,
            CountOperator::Gt => count > expected,
            CountOperator::Ge => count >= expected,
        };

        if result {
            Ok(VerificationResult::Verified)
        } else {
            Ok(VerificationResult::Failed)
        }
    }

    fn eval_is_empty(state: &Value, path: &str) -> Result<VerificationResult, EngineError> {
        let actual = get_path(state, path);

        let is_empty = match actual {
            Some(Value::Array(arr)) => arr.is_empty(),
            Some(Value::Object(obj)) => obj.is_empty(),
            Some(Value::String(s)) => s.is_empty(),
            Some(Value::Null) => true,
            None => true,
            _ => false,
        };

        if is_empty {
            Ok(VerificationResult::Verified)
        } else {
            Ok(VerificationResult::Failed)
        }
    }

    fn eval_is_not_empty(state: &Value, path: &str) -> Result<VerificationResult, EngineError> {
        let result = Self::eval_is_empty(state, path)?;
        match result {
            VerificationResult::Verified => Ok(VerificationResult::Failed),
            VerificationResult::Failed => Ok(VerificationResult::Verified),
            _ => Ok(VerificationResult::Failed),
        }
    }

    fn eval_all(
        predicates: &[Predicate],
        state: &Value,
        args: &Value,
    ) -> Result<VerificationResult, EngineError> {
        for predicate in predicates {
            let result = Self::evaluate(predicate, state, args)?;
            if !matches!(result, VerificationResult::Verified) {
                return Ok(VerificationResult::Failed);
            }
        }
        Ok(VerificationResult::Verified)
    }

    fn eval_any(
        predicates: &[Predicate],
        state: &Value,
        args: &Value,
    ) -> Result<VerificationResult, EngineError> {
        for predicate in predicates {
            let result = Self::evaluate(predicate, state, args)?;
            if matches!(result, VerificationResult::Verified) {
                return Ok(VerificationResult::Verified);
            }
        }
        Ok(VerificationResult::Failed)
    }

    fn eval_not(
        predicate: &Predicate,
        state: &Value,
        args: &Value,
    ) -> Result<VerificationResult, EngineError> {
        let result = Self::evaluate(predicate, state, args)?;
        match result {
            VerificationResult::Verified => Ok(VerificationResult::Failed),
            VerificationResult::Failed => Ok(VerificationResult::Verified),
            _ => Ok(VerificationResult::Unknown),
        }
    }

    fn eval_implies(
        antecedent: &Predicate,
        consequent: &Predicate,
        state: &Value,
        args: &Value,
    ) -> Result<VerificationResult, EngineError> {
        // If antecedent is false, implication is true
        let ant_result = Self::evaluate(antecedent, state, args)?;
        if !matches!(ant_result, VerificationResult::Verified) {
            return Ok(VerificationResult::Verified);
        }

        // If antecedent is true, consequent must be true
        let cons_result = Self::evaluate(consequent, state, args)?;
        if matches!(cons_result, VerificationResult::Verified) {
            Ok(VerificationResult::Verified)
        } else {
            Ok(VerificationResult::Failed)
        }
    }
}

/// Get value at path from JSON using dot notation
fn get_path<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    let mut current = value;
    for segment in path.split('.') {
        match (current, segment.parse::<usize>()) {
            (Value::Object(obj), _) => current = obj.get(segment)?,
            (Value::Array(arr), Ok(idx)) => current = arr.get(idx)?,
            _ => return None,
        }
    }
    Some(current)
}

/// Resolve $args references in a value
fn resolve_value(value: &Value, args: &Value) -> Value {
    if let Some(s) = value.as_str() {
        if let Some(key) = s.strip_prefix("$args.") {
            return get_path(args, key).cloned().unwrap_or(value.clone());
        }
    }
    value.clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentverify_core::{CountOperator, Predicate};

    #[test]
    fn exists_found() {
        let state = serde_json::json!({"customer": {"email": "test@example.com"}});
        let predicate = Predicate::exists("customer.email");
        let result = PredicateEngine::evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Verified);
    }

    #[test]
    fn exists_not_found() {
        let state = serde_json::json!({"customer": {}});
        let predicate = Predicate::exists("customer.email");
        let result = PredicateEngine::evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Failed);
    }

    #[test]
    fn not_exists() {
        let state = serde_json::json!({"customer": {}});
        let predicate = Predicate::not_exists("customer.email");
        let result = PredicateEngine::evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Verified);
    }

    #[test]
    fn equals_success() {
        let state = serde_json::json!({"customer": {"status": "active"}});
        let predicate = Predicate::equals("customer.status", "active");
        let result = PredicateEngine::evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Verified);
    }

    #[test]
    fn equals_failure() {
        let state = serde_json::json!({"customer": {"status": "inactive"}});
        let predicate = Predicate::equals("customer.status", "active");
        let result = PredicateEngine::evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Failed);
    }

    #[test]
    fn not_equals() {
        let state = serde_json::json!({"customer": {"status": "active"}});
        let predicate = Predicate::NotEquals {
            path: "customer.status".into(),
            value: serde_json::json!("inactive"),
        };
        let result = PredicateEngine::evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Verified);
    }

    #[test]
    fn contains_in_string() {
        let state = serde_json::json!({"message": "Hello World"});
        let predicate = Predicate::Contains {
            path: "message".into(),
            value: serde_json::json!("World"),
        };
        let result = PredicateEngine::evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Verified);
    }

    #[test]
    fn contains_in_array() {
        let state = serde_json::json!({"items": ["a", "b", "c"]});
        let predicate = Predicate::Contains {
            path: "items".into(),
            value: serde_json::json!("b"),
        };
        let result = PredicateEngine::evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Verified);
    }

    #[test]
    fn matches_regex() {
        let state = serde_json::json!({"email": "test@example.com"});
        let predicate = Predicate::Matches {
            path: "email".into(),
            pattern: r".+@.+\..+".into(),
        };
        let result = PredicateEngine::evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Verified);
    }

    #[test]
    fn matches_regex_failure() {
        let state = serde_json::json!({"email": "invalid"});
        let predicate = Predicate::Matches {
            path: "email".into(),
            pattern: r".+@.+\..+".into(),
        };
        let result = PredicateEngine::evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Failed);
    }

    #[test]
    fn greater_than_number() {
        let state = serde_json::json!({"count": 10});
        let predicate = Predicate::GreaterThan {
            path: "count".into(),
            value: serde_json::json!(5),
        };
        let result = PredicateEngine::evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Verified);
    }

    #[test]
    fn less_than_number() {
        let state = serde_json::json!({"count": 3});
        let predicate = Predicate::LessThan {
            path: "count".into(),
            value: serde_json::json!(5),
        };
        let result = PredicateEngine::evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Verified);
    }

    #[test]
    fn count_equals() {
        let state = serde_json::json!({"items": [1, 2, 3]});
        let predicate = Predicate::count("items", CountOperator::Eq, 3);
        let result = PredicateEngine::evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Verified);
    }

    #[test]
    fn count_greater() {
        let state = serde_json::json!({"items": [1, 2, 3, 4, 5]});
        let predicate = Predicate::count("items", CountOperator::Gt, 3);
        let result = PredicateEngine::evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Verified);
    }

    #[test]
    fn is_empty_array() {
        let state = serde_json::json!({"items": []});
        let predicate = Predicate::IsEmpty {
            path: "items".into(),
        };
        let result = PredicateEngine::evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Verified);
    }

    #[test]
    fn is_not_empty() {
        let state = serde_json::json!({"items": [1]});
        let predicate = Predicate::IsNotEmpty {
            path: "items".into(),
        };
        let result = PredicateEngine::evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Verified);
    }

    #[test]
    fn all_predicates() {
        let state = serde_json::json!({"a": 1, "b": 2});
        let predicate = Predicate::all(vec![Predicate::exists("a"), Predicate::exists("b")]);
        let result = PredicateEngine::evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Verified);
    }

    #[test]
    fn all_predicates_failure() {
        let state = serde_json::json!({"a": 1});
        let predicate = Predicate::all(vec![Predicate::exists("a"), Predicate::exists("b")]);
        let result = PredicateEngine::evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Failed);
    }

    #[test]
    fn any_predicates() {
        let state = serde_json::json!({"a": 1});
        let predicate = Predicate::any(vec![Predicate::exists("a"), Predicate::exists("b")]);
        let result = PredicateEngine::evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Verified);
    }

    #[test]
    fn not_predicate() {
        let state = serde_json::json!({"a": 1});
        let predicate = Predicate::negate(Predicate::exists("b"));
        let result = PredicateEngine::evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Verified);
    }

    #[test]
    fn implies_true() {
        let state = serde_json::json!({"a": true, "b": true});
        let predicate = Predicate::Implies {
            antecedent: Box::new(Predicate::exists("a")),
            consequent: Box::new(Predicate::exists("b")),
        };
        let result = PredicateEngine::evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Verified);
    }

    #[test]
    fn implies_false_when_antecedent_true_consequent_false() {
        // Antecedent exists (Verified), consequent does not exist (Failed)
        // A => B should be false when A is true and B is false
        let state = serde_json::json!({"a": 1, "c": 1});
        let predicate = Predicate::Implies {
            antecedent: Box::new(Predicate::exists("a")),
            consequent: Box::new(Predicate::exists("b")), // b does not exist
        };
        let result = PredicateEngine::evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Failed);
    }

    #[test]
    fn implies_true_when_antecedent_false() {
        let state = serde_json::json!({"a": false});
        let predicate = Predicate::Implies {
            antecedent: Box::new(Predicate::exists("b")),
            consequent: Box::new(Predicate::exists("a")),
        };
        let result = PredicateEngine::evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Verified);
    }

    #[test]
    fn args_resolution() {
        let state = serde_json::json!({"customer": {"email": "test@example.com"}});
        let args = serde_json::json!({"email": "test@example.com"});
        let predicate = Predicate::equals("customer.email", "$args.email");
        let result = PredicateEngine::evaluate(&predicate, &state, &args).unwrap();
        assert_eq!(result, VerificationResult::Verified);
    }
}
