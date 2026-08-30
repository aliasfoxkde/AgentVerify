//! Observation types

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Source identifier (e.g., "postgres", "rest", "redis")
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceId(pub String);

/// Evidence item from observation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    /// Source of the evidence
    pub source: String,
    /// Raw evidence data
    pub data: Value,
    /// When the evidence was captured
    pub timestamp: DateTime<Utc>,
}

impl Evidence {
    /// Create new evidence
    pub fn new(source: impl Into<String>, data: Value) -> Self {
        Self {
            source: source.into(),
            data,
            timestamp: Utc::now(),
        }
    }
}

/// An observation captures state from a system of record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Observation {
    /// Source identifier (e.g., "postgres", "rest")
    pub source: SourceId,
    /// When the observation was made
    pub timestamp: DateTime<Utc>,
    /// Observed JSON state
    pub state: Value,
    /// Raw evidence items
    #[serde(default)]
    pub evidence: Vec<Evidence>,
}

impl Observation {
    /// Create a new observation
    #[must_use]
    pub fn new(source: SourceId, state: Value) -> Self {
        Self {
            source,
            timestamp: Utc::now(),
            state,
            evidence: Vec::new(),
        }
    }

    /// Add evidence to observation
    #[must_use]
    pub fn with_evidence(mut self, evidence: Evidence) -> Self {
        self.evidence.push(evidence);
        self
    }

    /// Get value at JSON path
    #[must_use]
    pub fn get(&self, path: &str) -> Option<&Value> {
        jsonpath_get(&self.state, path)
    }
}

/// Get value from JSON using dot notation path
fn jsonpath_get<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    let mut current = value;
    for segment in path.split('.') {
        match (current, segment.parse::<usize>()) {
            (Value::Object(obj), _) => {
                current = obj.get(segment)?;
            }
            (Value::Array(arr), Ok(idx)) => {
                current = arr.get(idx)?;
            }
            _ => return None,
        }
    }
    Some(current)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_nested_value() {
        let state = serde_json::json!({
            "customer": {
                "email": "test@example.com",
                "status": "active"
            }
        });

        let obs = Observation::new(SourceId("test".into()), state);

        assert_eq!(
            obs.get("customer.email"),
            Some(&serde_json::json!("test@example.com"))
        );
        assert_eq!(
            obs.get("customer.status"),
            Some(&serde_json::json!("active"))
        );
        assert_eq!(obs.get("customer.nonexistent"), None);
    }

    #[test]
    fn get_array_value() {
        let state = serde_json::json!({
            "items": ["a", "b", "c"]
        });

        let obs = Observation::new(SourceId("test".into()), state);

        assert_eq!(obs.get("items.0"), Some(&serde_json::json!("a")));
        assert_eq!(obs.get("items.2"), Some(&serde_json::json!("c")));
    }

    #[test]
    fn evidence_new_captures_source_and_timestamp() {
        let before = Utc::now();
        let evidence = Evidence::new("postgres", serde_json::json!({"rows": 1}));
        let after = Utc::now();

        assert_eq!(evidence.source, "postgres");
        assert_eq!(evidence.data, serde_json::json!({"rows": 1}));
        assert!(before <= evidence.timestamp && evidence.timestamp <= after);
    }

    #[test]
    fn observation_with_evidence_appends_in_order() {
        let obs = Observation::new(SourceId("rest".into()), serde_json::json!({"ok": true}))
            .with_evidence(Evidence::new("rest", serde_json::json!({"attempt": 1})))
            .with_evidence(Evidence::new(
                "audit-log",
                serde_json::json!({"attempt": 2}),
            ));

        assert_eq!(obs.source, SourceId("rest".into()));
        assert_eq!(obs.evidence.len(), 2);
        assert_eq!(obs.evidence[0].source, "rest");
        assert_eq!(obs.evidence[1].source, "audit-log");
        assert_eq!(obs.get("ok"), Some(&serde_json::json!(true)));
    }

    #[test]
    fn observation_path_not_resolvable_returns_none() {
        // A scalar cannot be traversed, an array cannot be traversed by name,
        // and an out-of-range index has no value.
        let scalar = Observation::new(SourceId("s".into()), serde_json::json!("plain"));
        assert_eq!(scalar.get("anything"), None);

        let state = serde_json::json!({"list": [1, 2, 3], "nested": {"deep": {"x": 1}}});
        let obs = Observation::new(SourceId("s".into()), state);
        assert_eq!(obs.get("list.name"), None);
        assert_eq!(obs.get("list.9"), None);
        assert_eq!(obs.get("missing.entirely"), None);
        assert_eq!(obs.get("nested.deep"), Some(&serde_json::json!({"x": 1})));
        assert_eq!(obs.get("nested.deep.x"), Some(&serde_json::json!(1)));
        assert_eq!(obs.get(""), None);
    }

    #[test]
    fn observation_get_preserves_null_values() {
        let obs = Observation::new(
            SourceId("s".into()),
            serde_json::json!({"refund": {"completed_at": null}}),
        );
        assert_eq!(obs.get("refund.completed_at"), Some(&Value::Null));
    }

    #[test]
    fn observation_roundtrips_through_json() {
        let obs = Observation::new(
            SourceId("postgres".into()),
            serde_json::json!({"status": "captured"}),
        )
        .with_evidence(Evidence::new("postgres", serde_json::json!({"id": 7})));

        let json = serde_json::to_string(&obs).unwrap();
        let back: Observation = serde_json::from_str(&json).unwrap();
        assert_eq!(back.source, obs.source);
        assert_eq!(back.state, obs.state);
        assert_eq!(back.evidence.len(), 1);
        assert_eq!(back.evidence[0].data, obs.evidence[0].data);
    }

    #[test]
    fn observation_deserializes_without_evidence() {
        let json = r#"{
            "source": "rest",
            "timestamp": "2026-01-15T10:30:00Z",
            "state": {"status": "ok"}
        }"#;
        let obs: Observation = serde_json::from_str(json).unwrap();
        assert_eq!(obs.source, SourceId("rest".into()));
        assert!(obs.evidence.is_empty());
        assert_eq!(obs.get("status"), Some(&serde_json::json!("ok")));
    }

    #[test]
    fn observation_and_evidence_are_debug_and_clone() {
        let obs = Observation::new(SourceId("postgres".into()), serde_json::json!({"a": 1}))
            .with_evidence(Evidence::new("postgres", serde_json::json!({"b": 2})));

        let cloned = obs.clone();
        assert_eq!(cloned.source, obs.source);
        assert_eq!(
            serde_json::to_value(&cloned.evidence).unwrap(),
            serde_json::to_value(&obs.evidence).unwrap()
        );

        let debugged = std::format!("{obs:?}");
        assert!(debugged.contains("postgres"));
        assert!(std::format!("{:?}", obs.evidence[0]).contains("Evidence"));
    }
}
