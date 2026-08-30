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
    #[must_use]
    pub fn all(predicates: Vec<Predicate>) -> Self {
        Self::All { predicates }
    }

    /// Create an any compound predicate
    #[must_use]
    pub fn any(predicates: Vec<Predicate>) -> Self {
        Self::Any { predicates }
    }

    /// Create a not compound predicate
    #[must_use]
    pub fn negate(predicate: Predicate) -> Self {
        Self::Not {
            predicate: Box::new(predicate),
        }
    }

    /// Create a not-equals check
    pub fn not_equals(path: impl Into<String>, value: impl Into<Value>) -> Self {
        Self::NotEquals {
            path: path.into(),
            value: value.into(),
        }
    }

    /// Create a contains check
    pub fn contains(path: impl Into<String>, value: impl Into<Value>) -> Self {
        Self::Contains {
            path: path.into(),
            value: value.into(),
        }
    }

    /// Create a matches regex check
    pub fn matches(path: impl Into<String>, pattern: impl Into<String>) -> Self {
        Self::Matches {
            path: path.into(),
            pattern: pattern.into(),
        }
    }

    /// Create a greater-than check
    pub fn greater_than(path: impl Into<String>, value: impl Into<Value>) -> Self {
        Self::GreaterThan {
            path: path.into(),
            value: value.into(),
        }
    }

    /// Create a less-than check
    pub fn less_than(path: impl Into<String>, value: impl Into<Value>) -> Self {
        Self::LessThan {
            path: path.into(),
            value: value.into(),
        }
    }

    /// Create an is-empty check
    pub fn is_empty(path: impl Into<String>) -> Self {
        Self::IsEmpty { path: path.into() }
    }

    /// Create an is-not-empty check
    pub fn is_not_empty(path: impl Into<String>) -> Self {
        Self::IsNotEmpty { path: path.into() }
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
