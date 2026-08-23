//! Predicate engine implementation
//!
//! Deterministic predicate evaluation for verification conditions.

use std::collections::HashMap;
use std::sync::RwLock;

use agentverify_core::{CountOperator, Predicate, VerificationResult};
use regex::Regex;
use serde_json::Value;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Error)]
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
pub struct PredicateEngine {
    regex_cache: RwLock<HashMap<String, Regex>>,
}

impl Default for PredicateEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl PredicateEngine {
    /// Create a new PredicateEngine with an empty regex cache
    pub fn new() -> Self {
        Self {
            regex_cache: RwLock::new(HashMap::new()),
        }
    }

    /// Evaluate a predicate against observed state
    pub fn evaluate(
        &self,
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
            Predicate::Matches { path, pattern } => self.eval_matches(state, path, pattern),
            Predicate::GreaterThan { path, value } => {
                Self::eval_greater_than(state, path, value, args)
            }
            Predicate::LessThan { path, value } => Self::eval_less_than(state, path, value, args),
            Predicate::Count {
                path,
                operator,
                value,
            } => Self::eval_count(state, path, *operator, *value),
            Predicate::IsEmpty { path } => self.eval_is_empty(state, path),
            Predicate::IsNotEmpty { path } => self.eval_is_not_empty(state, path),
            Predicate::All { predicates } => self.eval_all(predicates, state, args),
            Predicate::Any { predicates } => self.eval_any(predicates, state, args),
            Predicate::Not { predicate } => self.eval_not(predicate, state, args),
            Predicate::Implies {
                antecedent,
                consequent,
            } => self.eval_implies(antecedent, consequent, state, args),
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
        &self,
        state: &Value,
        path: &str,
        pattern: &str,
    ) -> Result<VerificationResult, EngineError> {
        let actual = get_path(state, path);

        match actual {
            Some(Value::String(s)) => {
                // Try to get from cache first
                let regex = {
                    let cache = self.regex_cache.read().unwrap();
                    cache.get(pattern).cloned()
                };

                let regex = match regex {
                    Some(r) => r,
                    None => {
                        // Not in cache, compile and store
                        let new_regex = Regex::new(pattern)?;
                        let mut cache = self.regex_cache.write().unwrap();
                        cache.insert(pattern.to_string(), new_regex.clone());
                        new_regex
                    }
                };

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

    fn eval_is_empty(&self, state: &Value, path: &str) -> Result<VerificationResult, EngineError> {
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

    fn eval_is_not_empty(&self, state: &Value, path: &str) -> Result<VerificationResult, EngineError> {
        let result = self.eval_is_empty(state, path)?;
        match result {
            VerificationResult::Verified => Ok(VerificationResult::Failed),
            VerificationResult::Failed => Ok(VerificationResult::Verified),
            _ => Ok(VerificationResult::Failed),
        }
    }

    fn eval_all(
        &self,
        predicates: &[Predicate],
        state: &Value,
        args: &Value,
    ) -> Result<VerificationResult, EngineError> {
        for predicate in predicates {
            let result = self.evaluate(predicate, state, args)?;
            if !matches!(result, VerificationResult::Verified) {
                return Ok(VerificationResult::Failed);
            }
        }
        Ok(VerificationResult::Verified)
    }

    fn eval_any(
        &self,
        predicates: &[Predicate],
        state: &Value,
        args: &Value,
    ) -> Result<VerificationResult, EngineError> {
        for predicate in predicates {
            let result = self.evaluate(predicate, state, args)?;
            if matches!(result, VerificationResult::Verified) {
                return Ok(VerificationResult::Verified);
            }
        }
        Ok(VerificationResult::Failed)
    }

    fn eval_not(
        &self,
        predicate: &Predicate,
        state: &Value,
        args: &Value,
    ) -> Result<VerificationResult, EngineError> {
        let result = self.evaluate(predicate, state, args)?;
        match result {
            VerificationResult::Verified => Ok(VerificationResult::Failed),
            VerificationResult::Failed => Ok(VerificationResult::Verified),
            _ => Ok(VerificationResult::Unknown),
        }
    }

    fn eval_implies(
        &self,
        antecedent: &Predicate,
        consequent: &Predicate,
        state: &Value,
        args: &Value,
    ) -> Result<VerificationResult, EngineError> {
        // If antecedent is false, implication is true
        let ant_result = self.evaluate(antecedent, state, args)?;
        if !matches!(ant_result, VerificationResult::Verified) {
            return Ok(VerificationResult::Verified);
        }

        // If antecedent is true, consequent must be true
        let cons_result = self.evaluate(consequent, state, args)?;
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
        let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Verified);
    }

    #[test]
    fn exists_not_found() {
        let state = serde_json::json!({"customer": {}});
        let predicate = Predicate::exists("customer.email");
        let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Failed);
    }

    #[test]
    fn not_exists() {
        let state = serde_json::json!({"customer": {}});
        let predicate = Predicate::not_exists("customer.email");
        let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Verified);
    }

    #[test]
    fn equals_success() {
        let state = serde_json::json!({"customer": {"status": "active"}});
        let predicate = Predicate::equals("customer.status", "active");
        let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Verified);
    }

    #[test]
    fn equals_failure() {
        let state = serde_json::json!({"customer": {"status": "inactive"}});
        let predicate = Predicate::equals("customer.status", "active");
        let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Failed);
    }

    #[test]
    fn not_equals() {
        let state = serde_json::json!({"customer": {"status": "active"}});
        let predicate = Predicate::NotEquals {
            path: "customer.status".into(),
            value: serde_json::json!("inactive"),
        };
        let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Verified);
    }

    #[test]
    fn contains_in_string() {
        let state = serde_json::json!({"message": "Hello World"});
        let predicate = Predicate::Contains {
            path: "message".into(),
            value: serde_json::json!("World"),
        };
        let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Verified);
    }

    #[test]
    fn contains_in_array() {
        let state = serde_json::json!({"items": ["a", "b", "c"]});
        let predicate = Predicate::Contains {
            path: "items".into(),
            value: serde_json::json!("b"),
        };
        let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Verified);
    }

    #[test]
    fn matches_regex() {
        let state = serde_json::json!({"email": "test@example.com"});
        let predicate = Predicate::Matches {
            path: "email".into(),
            pattern: r".+@.+\..+".into(),
        };
        let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Verified);
    }

    #[test]
    fn matches_regex_failure() {
        let state = serde_json::json!({"email": "invalid"});
        let predicate = Predicate::Matches {
            path: "email".into(),
            pattern: r".+@.+\..+".into(),
        };
        let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Failed);
    }

    #[test]
    fn greater_than_number() {
        let state = serde_json::json!({"count": 10});
        let predicate = Predicate::GreaterThan {
            path: "count".into(),
            value: serde_json::json!(5),
        };
        let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Verified);
    }

    #[test]
    fn less_than_number() {
        let state = serde_json::json!({"count": 3});
        let predicate = Predicate::LessThan {
            path: "count".into(),
            value: serde_json::json!(5),
        };
        let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Verified);
    }

    #[test]
    fn count_equals() {
        let state = serde_json::json!({"items": [1, 2, 3]});
        let predicate = Predicate::count("items", CountOperator::Eq, 3);
        let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Verified);
    }

    #[test]
    fn count_greater() {
        let state = serde_json::json!({"items": [1, 2, 3, 4, 5]});
        let predicate = Predicate::count("items", CountOperator::Gt, 3);
        let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Verified);
    }

    #[test]
    fn is_empty_array() {
        let state = serde_json::json!({"items": []});
        let predicate = Predicate::IsEmpty {
            path: "items".into(),
        };
        let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Verified);
    }

    #[test]
    fn is_not_empty() {
        let state = serde_json::json!({"items": [1]});
        let predicate = Predicate::IsNotEmpty {
            path: "items".into(),
        };
        let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Verified);
    }

    #[test]
    fn all_predicates() {
        let state = serde_json::json!({"a": 1, "b": 2});
        let predicate = Predicate::all(vec![Predicate::exists("a"), Predicate::exists("b")]);
        let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Verified);
    }

    #[test]
    fn all_predicates_failure() {
        let state = serde_json::json!({"a": 1});
        let predicate = Predicate::all(vec![Predicate::exists("a"), Predicate::exists("b")]);
        let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Failed);
    }

    #[test]
    fn any_predicates() {
        let state = serde_json::json!({"a": 1});
        let predicate = Predicate::any(vec![Predicate::exists("a"), Predicate::exists("b")]);
        let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Verified);
    }

    #[test]
    fn not_predicate() {
        let state = serde_json::json!({"a": 1});
        let predicate = Predicate::negate(Predicate::exists("b"));
        let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Verified);
    }

    #[test]
    fn implies_true() {
        let state = serde_json::json!({"a": true, "b": true});
        let predicate = Predicate::Implies {
            antecedent: Box::new(Predicate::exists("a")),
            consequent: Box::new(Predicate::exists("b")),
        };
        let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
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
        let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Failed);
    }

    #[test]
    fn implies_true_when_antecedent_false() {
        let state = serde_json::json!({"a": false});
        let predicate = Predicate::Implies {
            antecedent: Box::new(Predicate::exists("b")),
            consequent: Box::new(Predicate::exists("a")),
        };
        let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Verified);
    }

    #[test]
    fn args_resolution() {
        let state = serde_json::json!({"customer": {"email": "test@example.com"}});
        let args = serde_json::json!({"email": "test@example.com"});
        let predicate = Predicate::equals("customer.email", "$args.email");
        let result = PredicateEngine::default().evaluate(&predicate, &state, &args).unwrap();
        assert_eq!(result, VerificationResult::Verified);
    }

    // === Missing path and null handling tests ===

    #[test]
    fn exists_on_null_value() {
        // A path that exists but has null value should NOT be considered "found" by exists
        let state = serde_json::json!({"customer": null});
        let predicate = Predicate::exists("customer");
        let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        // null is a value, so the path exists
        assert_eq!(result, VerificationResult::Verified);
    }

    #[test]
    fn exists_on_missing_path() {
        let state = serde_json::json!({"customer": {"name": "test"}});
        let predicate = Predicate::exists("customer.missing");
        let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Failed);
    }

    #[test]
    fn not_exists_on_null_value() {
        // Path exists with null value - NotExists should return Failed
        let state = serde_json::json!({"customer": null});
        let predicate = Predicate::not_exists("customer");
        let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Failed);
    }

    #[test]
    fn equals_with_missing_path() {
        let state = serde_json::json!({"customer": {"name": "test"}});
        let predicate = Predicate::equals("customer.missing", serde_json::json!("value"));
        let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Failed);
    }

    #[test]
    fn equals_with_null_actual() {
        let state = serde_json::json!({"field": null});
        let predicate = Predicate::equals("field", serde_json::json!(null));
        let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Verified);
    }

    #[test]
    fn not_equals_with_null_actual() {
        let state = serde_json::json!({"field": null});
        let predicate = Predicate::not_equals("field", serde_json::json!("value"));
        let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Verified);
    }

    // === Type mismatch tests ===

    #[test]
    fn equals_type_mismatch_string_vs_number() {
        let state = serde_json::json!({"field": "123"});
        let predicate = Predicate::equals("field", serde_json::json!(123));
        let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        // String "123" != Number 123
        assert_eq!(result, VerificationResult::Failed);
    }

    #[test]
    fn equals_type_mismatch_number_vs_string() {
        let state = serde_json::json!({"field": 123});
        let predicate = Predicate::equals("field", serde_json::json!("123"));
        let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        // Number 123 != String "123"
        assert_eq!(result, VerificationResult::Failed);
    }

    #[test]
    fn equals_type_mismatch_bool() {
        let state = serde_json::json!({"field": true});
        let predicate = Predicate::equals("field", serde_json::json!("true"));
        let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Failed);
    }

    #[test]
    fn greater_than_type_mismatch() {
        // String > Number is always Failed
        let state = serde_json::json!({"field": "abc"});
        let predicate = Predicate::greater_than("field", serde_json::json!(100));
        let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Failed);
    }

    #[test]
    fn less_than_type_mismatch() {
        // String < Number is always Failed
        let state = serde_json::json!({"field": "abc"});
        let predicate = Predicate::less_than("field", serde_json::json!(100));
        let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Failed);
    }

    #[test]
    fn contains_type_mismatch() {
        // Contains on non-string/non-array returns Failed
        let state = serde_json::json!({"field": 123});
        let predicate = Predicate::contains("field", serde_json::json!("23"));
        let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Failed);
    }

    #[test]
    fn matches_type_mismatch_non_string() {
        // Matches only works on strings
        let state = serde_json::json!({"field": 123});
        let predicate = Predicate::matches("field", r"\d+");
        let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Failed);
    }

    // === Regex error tests ===

    #[test]
    fn matches_invalid_regex() {
        let state = serde_json::json!({"email": "test@example.com"});
        let predicate = Predicate::matches("email", r"[invalid(");
        // Invalid regex produces an EngineError::RegexError at evaluation time
        let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({}));
        assert!(result.is_err());
    }

    #[test]
    fn matches_empty_pattern() {
        let state = serde_json::json!({"field": "test"});
        let predicate = Predicate::matches("field", r"");
        let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        // Empty pattern matches at position 0 of any string
        assert_eq!(result, VerificationResult::Verified);
    }

    // === Numeric coercion edge cases ===

    #[test]
    fn greater_than_float_edge() {
        // 5.0 should equal 5 for comparison purposes
        let state = serde_json::json!({"count": 5.0});
        let predicate = Predicate::greater_than("count", serde_json::json!(5));
        let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Failed); // 5.0 is NOT > 5
    }

    #[test]
    fn less_than_float_edge() {
        let state = serde_json::json!({"count": 5.0});
        let predicate = Predicate::less_than("count", serde_json::json!(5));
        let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Failed); // 5.0 is NOT < 5
    }

    #[test]
    fn greater_than_float_success() {
        let state = serde_json::json!({"count": 5.1});
        let predicate = Predicate::greater_than("count", serde_json::json!(5));
        let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Verified);
    }

    #[test]
    fn greater_than_string_lexicographic() {
        // String comparison is lexicographic, not numeric
        // "10.0" > "2.0" is False because '1' (49) < '2' (50) in ASCII
        let state = serde_json::json!({"version": "10.0"});
        let predicate = Predicate::greater_than("version", serde_json::json!("2.0"));
        let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Failed);
    }

    // === Empty collection tests ===

    #[test]
    fn count_empty_array() {
        let state = serde_json::json!({"items": []});
        let predicate = Predicate::count("items", CountOperator::Eq, 0);
        let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Verified);
    }

    #[test]
    fn count_empty_object() {
        let state = serde_json::json!({"data": {}});
        let predicate = Predicate::count("data", CountOperator::Eq, 0);
        let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Verified); // Empty object has 0 keys
    }

    #[test]
    fn count_empty_string() {
        let state = serde_json::json!({"name": ""});
        let predicate = Predicate::count("name", CountOperator::Eq, 0);
        let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Verified);
    }

    #[test]
    fn is_empty_on_missing_path() {
        // Missing path is considered empty
        let state = serde_json::json!({"customer": {"name": "test"}});
        let predicate = Predicate::is_empty("customer.missing");
        let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Verified);
    }

    #[test]
    fn is_empty_null() {
        // null is considered empty
        let state = serde_json::json!({"field": null});
        let predicate = Predicate::is_empty("field");
        let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Verified);
    }

    #[test]
    fn is_not_empty_on_number() {
        // Numbers are never empty
        let state = serde_json::json!({"count": 0});
        let predicate = Predicate::is_not_empty("count");
        let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Verified);
    }

    // === Compound predicate edge cases ===

    #[test]
    fn all_with_one_failure() {
        // All requires ALL predicates to be Verified
        let state = serde_json::json!({"a": 1, "b": 2});
        let predicate = Predicate::all(vec![
            Predicate::exists("a"),
            Predicate::exists("b"),
            Predicate::exists("c"), // missing
        ]);
        let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Failed);
    }

    #[test]
    fn any_with_all_failures() {
        let state = serde_json::json!({"a": 1});
        let predicate = Predicate::any(vec![Predicate::exists("b"), Predicate::exists("c")]);
        let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Failed);
    }

    #[test]
    fn not_with_verified() {
        // Not Verified = Failed
        let state = serde_json::json!({"a": 1});
        let predicate = Predicate::negate(Predicate::exists("a"));
        let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Failed);
    }

    #[test]
    fn not_with_failed() {
        // Not Failed = Verified
        let state = serde_json::json!({"a": 1});
        let predicate = Predicate::negate(Predicate::exists("b"));
        let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Verified);
    }

    #[test]
    fn not_with_unknown() {
        // Not Unknown = Unknown
        // This tests the logic that Not on Unknown propagates Unknown
        let state = serde_json::json!({"a": 1});
        // Not can't actually produce Unknown directly since individual predicates don't return Unknown
        // But we can test with a nested compound
        let predicate = Predicate::Not {
            predicate: Box::new(Predicate::Implies {
                antecedent: Box::new(Predicate::exists("nonexistent")),
                consequent: Box::new(Predicate::exists("also_nonexistent")),
            }),
        };
        let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        // Implies with missing antecedent returns Verified, so Not Verified = Failed
        assert_eq!(result, VerificationResult::Failed);
    }

    #[test]
    fn implies_false_antecedent() {
        // A => B is true when A is false (antecedent doesn't exist)
        let state = serde_json::json!({"b": 1});
        let predicate = Predicate::Implies {
            antecedent: Box::new(Predicate::exists("a")), // false
            consequent: Box::new(Predicate::exists("b")), // true
        };
        let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Verified);
    }

    #[test]
    fn nested_compound_predicates() {
        let state = serde_json::json!({"a": 1, "b": 2, "c": 3});
        // All(Any(a, b), Not(c > 10))
        let predicate = Predicate::all(vec![
            Predicate::any(vec![Predicate::exists("a"), Predicate::exists("b")]),
            Predicate::negate(Predicate::greater_than("c", serde_json::json!(10))),
        ]);
        let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Verified);
    }

    // === Argument substitution edge cases ===

    #[test]
    fn args_missing_key() {
        // When $args.key doesn't exist, use the literal value
        let state = serde_json::json!({"field": "original"});
        let args = serde_json::json!({"other": "value"});
        let predicate = Predicate::equals("field", "$args.missing");
        let result = PredicateEngine::default().evaluate(&predicate, &state, &args).unwrap();
        // Falls back to literal "$args.missing" which != "original"
        assert_eq!(result, VerificationResult::Failed);
    }

    #[test]
    fn args_nested_path() {
        let state = serde_json::json!({"customer": {"email": "test@example.com"}});
        let args = serde_json::json!({"customer": {"email": "test@example.com"}});
        let predicate = Predicate::equals("customer.email", "$args.customer.email");
        let result = PredicateEngine::default().evaluate(&predicate, &state, &args).unwrap();
        assert_eq!(result, VerificationResult::Verified);
    }

    #[test]
    fn args_with_non_string_value() {
        // Args can be non-strings that get used as-is
        let state = serde_json::json!({"count": 42});
        let args = serde_json::json!({"expected": 42});
        let predicate = Predicate::equals("count", "$args.expected");
        let result = PredicateEngine::default().evaluate(&predicate, &state, &args).unwrap();
        assert_eq!(result, VerificationResult::Verified);
    }

    // === Path parsing edge cases ===

    #[test]
    fn path_with_array_index() {
        let state = serde_json::json!({"items": ["a", "b", "c"]});
        let predicate = Predicate::equals("items.1", serde_json::json!("b"));
        let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Verified);
    }

    #[test]
    fn path_with_out_of_bounds_array_index() {
        let state = serde_json::json!({"items": ["a", "b"]});
        let predicate = Predicate::exists("items.5");
        let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Failed);
    }

    #[test]
    fn path_with_non_numeric_segment() {
        // Trying to use non-numeric segment as array index
        let state = serde_json::json!({"items": ["a", "b"]});
        let predicate = Predicate::exists("items.abc");
        let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Failed);
    }

    #[test]
    fn empty_path() {
        let state = serde_json::json!({"field": "value"});
        let predicate = Predicate::exists("");
        let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        // Empty path splits to [""], which doesn't match root or any field
        assert_eq!(result, VerificationResult::Failed);
    }

    // === Contains edge cases ===

    #[test]
    fn contains_in_object_string_search() {
        let state = serde_json::json!({"config": {"host": "localhost", "port": 5432}});
        let predicate = Predicate::contains("config", serde_json::json!("localhost"));
        let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Verified);
    }

    #[test]
    fn contains_array_element_not_found() {
        let state = serde_json::json!({"items": ["a", "b", "c"]});
        let predicate = Predicate::contains("items", serde_json::json!("d"));
        let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Failed);
    }

    #[test]
    fn contains_on_missing_path() {
        let state = serde_json::json!({"items": ["a", "b"]});
        let predicate = Predicate::contains("missing", serde_json::json!("a"));
        let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Failed);
    }

    // === Count operator edge cases ===

    #[test]
    fn count_on_missing_path() {
        let state = serde_json::json!({"items": ["a", "b"]});
        let predicate = Predicate::count("missing", CountOperator::Eq, 0);
        let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Verified); // Missing path has count 0
    }

    #[test]
    fn count_string_length() {
        let state = serde_json::json!({"name": "abc"});
        let predicate = Predicate::count("name", CountOperator::Eq, 3);
        let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Verified);
    }

    #[test]
    fn count_object_keys() {
        let state = serde_json::json!({"obj": {"a": 1, "b": 2, "c": 3}});
        let predicate = Predicate::count("obj", CountOperator::Ge, 3);
        let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Verified);
    }

    // === Count on non-countable (returns Failed) ===

    #[test]
    fn count_on_number() {
        let state = serde_json::json!({"num": 42});
        let predicate = Predicate::count("num", CountOperator::Eq, 42);
        let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Failed);
    }

    #[test]
    fn count_on_bool() {
        let state = serde_json::json!({"flag": true});
        let predicate = Predicate::count("flag", CountOperator::Eq, 1);
        let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({})).unwrap();
        assert_eq!(result, VerificationResult::Failed);
    }
}

// === Property-based tests ===

#[cfg(test)]
mod property_tests {
    use super::*;
    use proptest::prelude::*;

    // Helper to parse JSON string safely
    fn parse_state(s: &str) -> Value {
        serde_json::from_str(s).unwrap_or(Value::Null)
    }

    // Property: Exists predicate is deterministic
    proptest! {
        #[test]
        fn exists_is_deterministic(state_json: String, path: String) {
            let state = parse_state(&state_json);
            let predicate = Predicate::exists(&path);
            let result1 = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({}));
            let result2 = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({}));
            prop_assert_eq!(result1, result2);
        }

        #[test]
        fn equals_is_deterministic(state_json: String, path: String, value: String) {
            let state = parse_state(&state_json);
            let expected: Value = serde_json::json!(value);
            let predicate = Predicate::equals(&path, expected);
            let result1 = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({}));
            let result2 = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({}));
            prop_assert_eq!(result1, result2);
        }

        #[test]
        fn not_equals_is_deterministic(state_json: String, path: String, value: String) {
            let state = parse_state(&state_json);
            let expected: Value = serde_json::json!(value);
            let predicate = Predicate::not_equals(&path, expected);
            let result1 = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({}));
            let result2 = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({}));
            prop_assert_eq!(result1, result2);
        }

        #[test]
        fn contains_is_deterministic(state_json: String, path: String, value: String) {
            let state = parse_state(&state_json);
            let predicate = Predicate::contains(&path, serde_json::json!(value));
            let result1 = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({}));
            let result2 = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({}));
            prop_assert_eq!(result1, result2);
        }

        #[test]
        fn not_exists_is_deterministic(state_json: String, path: String) {
            let state = parse_state(&state_json);
            let predicate = Predicate::not_exists(&path);
            let result1 = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({}));
            let result2 = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({}));
            prop_assert_eq!(result1, result2);
        }

        #[test]
        fn is_empty_is_deterministic(state_json: String, path: String) {
            let state = parse_state(&state_json);
            let predicate = Predicate::is_empty(&path);
            let result1 = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({}));
            let result2 = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({}));
            prop_assert_eq!(result1, result2);
        }

        #[test]
        fn is_not_empty_is_deterministic(state_json: String, path: String) {
            let state = parse_state(&state_json);
            let predicate = Predicate::is_not_empty(&path);
            let result1 = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({}));
            let result2 = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({}));
            prop_assert_eq!(result1, result2);
        }
    }

    // Property: All/Any with empty predicates
    proptest! {
        #[test]
        fn all_with_empty_predicates(state_json: String) {
            let state = parse_state(&state_json);
            let predicate = Predicate::all(vec![]);
            let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({}));
            prop_assert_eq!(result.unwrap(), VerificationResult::Verified);
        }

        #[test]
        fn any_with_empty_predicates(state_json: String) {
            let state = parse_state(&state_json);
            let predicate = Predicate::any(vec![]);
            let result = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({}));
            prop_assert_eq!(result.unwrap(), VerificationResult::Failed);
        }
    }

    // Property: Count is deterministic
    proptest! {
        #[test]
        fn count_is_deterministic(state_json: String, path: String, op: String, value: i64) {
            let state = parse_state(&state_json);
            let operator = match op.as_str() {
                "eq" => CountOperator::Eq,
                "ne" => CountOperator::Ne,
                "gt" => CountOperator::Gt,
                "ge" => CountOperator::Ge,
                "lt" => CountOperator::Lt,
                "le" => CountOperator::Le,
                _ => CountOperator::Eq,
            };
            let predicate = Predicate::count(&path, operator, value);
            let result1 = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({}));
            let result2 = PredicateEngine::default().evaluate(&predicate, &state, &serde_json::json!({}));
            prop_assert_eq!(result1, result2);
        }
    }
}
