//! Predicate types for verification conditions

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A predicate is a deterministic, evaluable condition
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Predicate {
    // === Basic Predicates ===
    /// Check if a path exists
    Exists {
        /// Dot-separated path into the observed state document.
        path: String,
    },
    /// Check if a path does not exist
    NotExists {
        /// Dot-separated path into the observed state document.
        path: String,
    },
    /// Check if value equals expected
    Equals {
        /// Dot-separated path into the observed state document.
        path: String,
        /// The value the resolved path must compare equal to.
        value: Value,
    },
    /// Check if value does not equal expected
    NotEquals {
        /// Dot-separated path into the observed state document.
        path: String,
        /// The value the resolved path must not compare equal to.
        value: Value,
    },
    /// Check if value contains expected
    Contains {
        /// Dot-separated path into the observed state document.
        path: String,
        /// The substring, element, or key that must be contained.
        value: Value,
    },
    /// Check if value matches regex
    Matches {
        /// Dot-separated path into the observed state document.
        path: String,
        /// Regular expression the resolved string value must match.
        pattern: String,
    },
    /// Check if value is greater than expected
    GreaterThan {
        /// Dot-separated path into the observed state document.
        path: String,
        /// The numeric value the resolved value must be strictly greater than.
        value: Value,
    },
    /// Check if value is less than expected
    LessThan {
        /// Dot-separated path into the observed state document.
        path: String,
        /// The numeric value the resolved value must be strictly less than.
        value: Value,
    },

    // === Collection Predicates ===
    /// Check count of items
    Count {
        /// Dot-separated path into the observed state document.
        path: String,
        /// Comparison applied to the resolved item count.
        operator: CountOperator,
        /// The item count to compare against.
        value: i64,
    },
    /// Check if collection is empty
    IsEmpty {
        /// Dot-separated path to the collection in the observed state.
        path: String,
    },
    /// Check if collection is not empty
    IsNotEmpty {
        /// Dot-separated path to the collection in the observed state.
        path: String,
    },

    // === Compound Predicates ===
    /// All predicates must be true (AND)
    All {
        /// Predicates that must all evaluate to satisfied.
        predicates: Vec<Predicate>,
    },
    /// Any predicate must be true (OR)
    Any {
        /// Predicates of which at least one must evaluate to satisfied.
        predicates: Vec<Predicate>,
    },
    /// Negate predicate (NOT)
    Not {
        /// The predicate whose result is inverted.
        predicate: Box<Predicate>,
    },
    /// If antecedent, then consequent
    Implies {
        /// The antecedent of the implication.
        antecedent: Box<Predicate>,
        /// The consequent, required only when the antecedent holds.
        consequent: Box<Predicate>,
    },
}

impl Predicate {
    /// Create an exists check
    #[must_use]
    pub fn exists(path: impl Into<String>) -> Self {
        Self::Exists { path: path.into() }
    }

    /// Create a not-exists check
    #[must_use]
    pub fn not_exists(path: impl Into<String>) -> Self {
        Self::NotExists { path: path.into() }
    }

    /// Create an equals check
    #[must_use]
    pub fn equals(path: impl Into<String>, value: impl Into<Value>) -> Self {
        Self::Equals {
            path: path.into(),
            value: value.into(),
        }
    }

    /// Create a count check
    #[must_use]
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
    #[must_use]
    pub fn not_equals(path: impl Into<String>, value: impl Into<Value>) -> Self {
        Self::NotEquals {
            path: path.into(),
            value: value.into(),
        }
    }

    /// Create a contains check
    #[must_use]
    pub fn contains(path: impl Into<String>, value: impl Into<Value>) -> Self {
        Self::Contains {
            path: path.into(),
            value: value.into(),
        }
    }

    /// Create a matches regex check
    #[must_use]
    pub fn matches(path: impl Into<String>, pattern: impl Into<String>) -> Self {
        Self::Matches {
            path: path.into(),
            pattern: pattern.into(),
        }
    }

    /// Create a greater-than check
    #[must_use]
    pub fn greater_than(path: impl Into<String>, value: impl Into<Value>) -> Self {
        Self::GreaterThan {
            path: path.into(),
            value: value.into(),
        }
    }

    /// Create a less-than check
    #[must_use]
    pub fn less_than(path: impl Into<String>, value: impl Into<Value>) -> Self {
        Self::LessThan {
            path: path.into(),
            value: value.into(),
        }
    }

    /// Create an is-empty check
    #[must_use]
    pub fn is_empty(path: impl Into<String>) -> Self {
        Self::IsEmpty { path: path.into() }
    }

    /// Create an is-not-empty check
    #[must_use]
    pub fn is_not_empty(path: impl Into<String>) -> Self {
        Self::IsNotEmpty { path: path.into() }
    }
}

/// Count operator
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CountOperator {
    /// Item count must be equal to the expected value.
    Eq,
    /// Item count must not be equal to the expected value.
    Ne,
    /// Item count must be strictly less than the expected value.
    Lt,
    /// Item count must be less than or equal to the expected value.
    Le,
    /// Item count must be strictly greater than the expected value.
    Gt,
    /// Item count must be greater than or equal to the expected value.
    Ge,
}
