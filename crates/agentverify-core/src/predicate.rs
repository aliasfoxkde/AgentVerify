//! Predicate types for verification conditions

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A predicate is a deterministic, evaluable condition
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Predicate {
    // === Basic Predicates ===
    /// Check if a path exists
    Exists { path: String },
    /// Check if a path does not exist
    NotExists { path: String },
    /// Check if value equals expected
    Equals { path: String, value: Value },
    /// Check if value does not equal expected
    NotEquals { path: String, value: Value },
    /// Check if value contains expected
    Contains { path: String, value: Value },
    /// Check if value matches regex
    Matches { path: String, pattern: String },
    /// Check if value is greater than expected
    GreaterThan { path: String, value: Value },
    /// Check if value is less than expected
    LessThan { path: String, value: Value },

    // === Collection Predicates ===
    /// Check count of items
    Count {
        path: String,
        operator: CountOperator,
        value: i64,
    },
    /// Check if collection is empty
    IsEmpty { path: String },
    /// Check if collection is not empty
    IsNotEmpty { path: String },

    // === Compound Predicates ===
    /// All predicates must be true (AND)
    All { predicates: Vec<Predicate> },
    /// Any predicate must be true (OR)
    Any { predicates: Vec<Predicate> },
    /// Negate predicate (NOT)
    Not { predicate: Box<Predicate> },
    /// If antecedent, then consequent
    Implies {
        antecedent: Box<Predicate>,
        consequent: Box<Predicate>,
    },
}

impl Predicate {
    /// Create an exists check
    pub fn exists(path: impl Into<String>) -> Self {
        Self::Exists { path: path.into() }
    }

    /// Create a not-exists check
    pub fn not_exists(path: impl Into<String>) -> Self {
        Self::NotExists { path: path.into() }
    }

    /// Create an equals check
    pub fn equals(path: impl Into<String>, value: impl Into<Value>) -> Self {
        Self::Equals {
            path: path.into(),
            value: value.into(),
        }
    }

    /// Create a count check
    pub fn count(path: impl Into<String>, operator: CountOperator, value: i64) -> Self {
        Self::Count {
            path: path.into(),
            operator,
            value,
        }
    }

    /// Create an all compound predicate
    pub fn all(predicates: Vec<Predicate>) -> Self {
        Self::All { predicates }
    }

    /// Create an any compound predicate
    pub fn any(predicates: Vec<Predicate>) -> Self {
        Self::Any { predicates }
    }

    /// Create a not compound predicate
    pub fn negate(predicate: Predicate) -> Self {
        Self::Not {
            predicate: Box::new(predicate),
        }
    }
}

/// Count operator
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CountOperator {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}
