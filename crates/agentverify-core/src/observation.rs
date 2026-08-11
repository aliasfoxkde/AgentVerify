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
    pub fn new(source: SourceId, state: Value) -> Self {
        Self {
            source,
            timestamp: Utc::now(),
            state,
            evidence: Vec::new(),
        }
    }

    /// Add evidence to observation
    pub fn with_evidence(mut self, evidence: Evidence) -> Self {
        self.evidence.push(evidence);
        self
    }

    /// Get value at JSON path
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
}
