//! Predicate engine implementation

use agentverify_core::{Predicate, VerificationResult};
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
            _ => unimplemented!("Predicate evaluation for {:?} not yet implemented", predicate),
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
        let actual = resolve_path(get_path(state, path), args);
        let expected = resolve_value(expected, args);

        if actual == Some(&expected) {
            Ok(VerificationResult::Verified)
        } else {
            Ok(VerificationResult::Failed)
        }
    }
}

/// Get value at path from JSON
fn get_path<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    let mut current = value;
    for segment in path.split('.') {
        match (current, segment.parse::<usize>()) {
            (Value::Object(obj), _) => {
                current = obj.get(segment)?
            }
            (Value::Array(arr), Ok(idx)) => {
                current = arr.get(idx)?
            }
            _ => return None,
        }
    }
    Some(current)
}

/// Resolve $args references in a value
fn resolve_value(value: &Value, args: &Value) -> Value {
    if let Some(s) = value.as_str() {
        if s.starts_with("$args.") {
            let key = &s[6..];
            return get_path(args, key).cloned().unwrap_or(value.clone());
        }
    }
    value.clone()
}

/// Resolve optional value with args
fn resolve_path<'a>(value: Option<&'a Value>, args: &Value) -> Option<&'a Value> {
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentverify_core::Predicate;

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
}
