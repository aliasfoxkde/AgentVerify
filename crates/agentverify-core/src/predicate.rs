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

#[cfg(test)]
mod tests {
    use super::*;

    /// Predicates deliberately carry no `PartialEq`: the wire format is the
    /// contract, so equality is asserted through the serialized representation.
    fn json_of(predicate: &Predicate) -> Value {
        serde_json::to_value(predicate).expect("predicate is serializable")
    }

    /// Every constructor must build the documented enum variant.
    #[test]
    fn constructors_build_expected_variants() {
        let cases: Vec<(Predicate, Predicate)> = vec![
            (
                Predicate::exists("refund.status"),
                Predicate::Exists {
                    path: "refund.status".into(),
                },
            ),
            (
                Predicate::not_exists("refund.id"),
                Predicate::NotExists {
                    path: "refund.id".into(),
                },
            ),
            (
                Predicate::equals("charge.status", "captured"),
                Predicate::Equals {
                    path: "charge.status".into(),
                    value: Value::String("captured".into()),
                },
            ),
            (
                Predicate::not_equals("charge.status", "declined"),
                Predicate::NotEquals {
                    path: "charge.status".into(),
                    value: Value::String("declined".into()),
                },
            ),
            (
                Predicate::contains("tags", "priority"),
                Predicate::Contains {
                    path: "tags".into(),
                    value: Value::String("priority".into()),
                },
            ),
            (
                Predicate::matches("email", "^[^@]+@[^@]+$"),
                Predicate::Matches {
                    path: "email".into(),
                    pattern: "^[^@]+@[^@]+$".into(),
                },
            ),
            (
                Predicate::greater_than("amount", 100),
                Predicate::GreaterThan {
                    path: "amount".into(),
                    value: Value::from(100),
                },
            ),
            (
                Predicate::less_than("amount", 10),
                Predicate::LessThan {
                    path: "amount".into(),
                    value: Value::from(10),
                },
            ),
            (
                Predicate::count("items", CountOperator::Gt, 3),
                Predicate::Count {
                    path: "items".into(),
                    operator: CountOperator::Gt,
                    value: 3,
                },
            ),
            (
                Predicate::is_empty("queue"),
                Predicate::IsEmpty {
                    path: "queue".into(),
                },
            ),
            (
                Predicate::is_not_empty("queue"),
                Predicate::IsNotEmpty {
                    path: "queue".into(),
                },
            ),
            (
                Predicate::all(vec![Predicate::exists("a"), Predicate::exists("b")]),
                Predicate::All {
                    predicates: vec![Predicate::exists("a"), Predicate::exists("b")],
                },
            ),
            (
                Predicate::any(vec![Predicate::exists("a")]),
                Predicate::Any {
                    predicates: vec![Predicate::exists("a")],
                },
            ),
            (
                Predicate::negate(Predicate::exists("a")),
                Predicate::Not {
                    predicate: Box::new(Predicate::exists("a")),
                },
            ),
        ];

        for (built, expected) in cases {
            assert_eq!(json_of(&built), json_of(&expected));
        }
    }

    /// Serde tags are part of the public contract format: contracts are
    /// exchanged as JSON between services, so tag spelling must not drift.
    #[test]
    fn every_variant_serializes_with_expected_tag() {
        let cases: Vec<(Predicate, &str)> = vec![
            (Predicate::exists("p"), r#"{"type":"exists","path":"p"}"#),
            (
                Predicate::not_exists("p"),
                r#"{"type":"not_exists","path":"p"}"#,
            ),
            (
                Predicate::equals("p", 1),
                r#"{"type":"equals","path":"p","value":1}"#,
            ),
            (
                Predicate::not_equals("p", 1),
                r#"{"type":"not_equals","path":"p","value":1}"#,
            ),
            (
                Predicate::contains("p", "x"),
                r#"{"type":"contains","path":"p","value":"x"}"#,
            ),
            (
                Predicate::matches("p", "^a"),
                r#"{"type":"matches","path":"p","pattern":"^a"}"#,
            ),
            (
                Predicate::greater_than("p", 2),
                r#"{"type":"greater_than","path":"p","value":2}"#,
            ),
            (
                Predicate::less_than("p", 2),
                r#"{"type":"less_than","path":"p","value":2}"#,
            ),
            (
                Predicate::count("p", CountOperator::Le, 4),
                r#"{"type":"count","path":"p","operator":"le","value":4}"#,
            ),
            (
                Predicate::is_empty("p"),
                r#"{"type":"is_empty","path":"p"}"#,
            ),
            (
                Predicate::is_not_empty("p"),
                r#"{"type":"is_not_empty","path":"p"}"#,
            ),
            (
                Predicate::all(vec![Predicate::exists("p")]),
                r#"{"type":"all","predicates":[{"type":"exists","path":"p"}]}"#,
            ),
            (
                Predicate::any(vec![Predicate::exists("p")]),
                r#"{"type":"any","predicates":[{"type":"exists","path":"p"}]}"#,
            ),
            (
                Predicate::negate(Predicate::exists("p")),
                r#"{"type":"not","predicate":{"type":"exists","path":"p"}}"#,
            ),
            (
                Predicate::Implies {
                    antecedent: Box::new(Predicate::exists("a")),
                    consequent: Box::new(Predicate::exists("b")),
                },
                r#"{"type":"implies","antecedent":{"type":"exists","path":"a"},"consequent":{"type":"exists","path":"b"}}"#,
            ),
        ];

        for (predicate, expected) in cases {
            let json = serde_json::to_string(&predicate).unwrap();
            assert_eq!(json, expected, "unexpected encoding for {expected}");
            let back: Predicate = serde_json::from_str(&json).unwrap();
            assert_eq!(json_of(&back), json_of(&predicate));
        }
    }

    #[test]
    fn count_operator_serializes_all_variants() {
        let pairs = [
            (CountOperator::Eq, "eq"),
            (CountOperator::Ne, "ne"),
            (CountOperator::Lt, "lt"),
            (CountOperator::Le, "le"),
            (CountOperator::Gt, "gt"),
            (CountOperator::Ge, "ge"),
        ];
        for (operator, name) in pairs {
            let json = serde_json::to_string(&operator).unwrap();
            assert_eq!(json, format!(r#""{name}""#));
            let back: CountOperator = serde_json::from_str(&json).unwrap();
            assert_eq!(back, operator);
        }
    }

    #[test]
    fn compound_predicates_nest_arbitrarily_deeply() {
        let predicate = Predicate::all(vec![
            Predicate::any(vec![
                Predicate::equals("status", "settled"),
                Predicate::greater_than("settled_at.epoch", 0),
            ]),
            Predicate::negate(Predicate::exists("voided_at")),
            Predicate::Implies {
                antecedent: Box::new(Predicate::is_not_empty("disputes")),
                consequent: Box::new(Predicate::count("disputes", CountOperator::Lt, 3)),
            },
        ]);

        let json = serde_json::to_string(&predicate).unwrap();
        let back: Predicate = serde_json::from_str(&json).unwrap();
        assert_eq!(json_of(&back), json_of(&predicate));
        assert!(json.contains(r#""type":"implies""#));
    }

    #[test]
    fn unknown_type_tag_is_rejected() {
        let result = serde_json::from_str::<Predicate>(r#"{"type":"nonsense","path":"p"}"#);
        assert!(result.is_err(), "unknown predicate tags must not parse");
    }

    #[test]
    fn missing_type_tag_is_rejected() {
        let result = serde_json::from_str::<Predicate>(r#"{"path":"p"}"#);
        assert!(result.is_err(), "the type tag is required");
    }

    #[test]
    fn predicates_are_debug_and_cloneable() {
        let predicate = Predicate::all(vec![Predicate::exists("a")]);
        let cloned = predicate.clone();
        assert_eq!(json_of(&cloned), json_of(&predicate));
        assert!(std::format!("{predicate:?}").contains("All"));
    }

    proptest::proptest! {
        /// Predicates survive a JSON roundtrip for arbitrary paths and counts,
        /// so no encoding path drops or reorders fields.
        #[test]
        fn json_roundtrip_preserves_equals(
            path in r"[a-z]{1,12}(\.[a-z]{1,12}){0,3}",
            value in 0i64..1_000_000,
        ) {
            let predicate = Predicate::equals(path.as_str(), value);
            let json = serde_json::to_string(&predicate).unwrap();
            let back: Predicate = serde_json::from_str(&json).unwrap();
            proptest::prop_assert_eq!(json_of(&back), json_of(&predicate));
        }
    }
}
