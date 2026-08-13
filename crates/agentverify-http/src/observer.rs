//! REST observer implementation
//!
//! Observes system state via REST API calls.

use agentverify_core::{Action, Contract, Observation, SourceId};
use agentverify_runtime::ExecutorError;
use reqwest::Client;
use serde::de::DeserializeOwned;
use serde_json::Value;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RestObserverError {
    #[error("HTTP request failed: {0}")]
    HttpError(#[from] reqwest::Error),

    #[error("Invalid URL: {0}")]
    InvalidUrl(String),

    #[error("Response parse error: {0}")]
    ParseError(String),

    #[error("Request timeout")]
    Timeout,

    #[error("Redacted value detected in path: {0}")]
    RedactedPath(String),
}

/// REST observer configuration
#[derive(Debug, Clone)]
pub struct RestObserverConfig {
    /// Base URL for the REST API
    pub base_url: String,
    /// Default timeout in milliseconds
    pub timeout_ms: u64,
    /// Maximum evidence size in bytes
    pub max_evidence_size: usize,
    /// Paths to redact from evidence (comma-separated)
    pub redact_paths: Vec<String>,
    /// Additional headers to include (key=value pairs)
    pub headers: Vec<(String, String)>,
}

impl RestObserverConfig {
    /// Create a new config with required base_url
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            timeout_ms: 5000,
            max_evidence_size: 1024 * 1024, // 1MB
            redact_paths: Vec::new(),
            headers: Vec::new(),
        }
    }

    /// Set the timeout
    pub fn with_timeout(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = timeout_ms;
        self
    }

    /// Add a path to redact
    pub fn with_redact_path(mut self, path: impl Into<String>) -> Self {
        self.redact_paths.push(path.into());
        self
    }

    /// Add a header
    pub fn with_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((key.into(), value.into()));
        self
    }
}

impl Default for RestObserverConfig {
    fn default() -> Self {
        Self::new("http://localhost:8080")
    }
}

/// REST observer for making HTTP GET requests to observe system state
pub struct RestObserver {
    client: Client,
    config: RestObserverConfig,
}

impl RestObserver {
    /// Create a new REST observer with the given configuration
    pub fn new(config: RestObserverConfig) -> Result<Self, RestObserverError> {
        let client = Client::builder()
            .timeout(std::time::Duration::from_millis(config.timeout_ms))
            .build()?;

        Ok(Self { client, config })
    }

    /// Build the observation URL from action and contract
    fn build_url(&self, action: &Action, contract: &Contract) -> Result<String, RestObserverError> {
        // Use the action_name from contract to determine the resource
        let action_name = &contract.action_name;

        // Build URL: base_url/{action_name}/{id_path}
        let id = action.id;
        let url = format!(
            "{}/{}/{}",
            self.config.base_url.trim_end_matches('/'),
            action_name,
            id
        );

        // Validate URL to prevent injection
        if !url.contains("..") && !url.contains("//") {
            Ok(url)
        } else {
            Err(RestObserverError::InvalidUrl(url))
        }
    }

    /// Fetch JSON data from the given URL
    async fn fetch_json<T: DeserializeOwned>(&self, url: &str) -> Result<T, RestObserverError> {
        let mut request = self.client.get(url);

        for (key, value) in &self.config.headers {
            request = request.header(key.as_str(), value.as_str());
        }

        let response = request.send().await?;
        let status = response.status();

        if status.is_success() {
            response.json::<T>().await.map_err(|e| {
                RestObserverError::ParseError(format!("Failed to parse response: {}", e))
            })
        } else {
            Err(RestObserverError::ParseError(format!(
                "HTTP error: {}",
                status
            )))
        }
    }

    /// Redact sensitive paths from the JSON value
    fn redact(&self, value: &mut Value) {
        for path in &self.config.redact_paths {
            if let Some(redacted) = value.pointer_mut(path) {
                *redacted = Value::String("[REDACTED]".to_string());
            }
        }
    }

    /// Truncate evidence if it exceeds max size
    fn truncate(&self, value: Value) -> Value {
        let json = serde_json::to_string(&value).unwrap_or_default();
        if json.len() > self.config.max_evidence_size {
            // Return truncated evidence
            Value::String(format!(
                "[TRUNCATED: {} > {} bytes]",
                json.len(),
                self.config.max_evidence_size
            ))
        } else {
            value
        }
    }
}

#[async_trait::async_trait]
impl agentverify_runtime::Observer for RestObserver {
    async fn observe(
        &self,
        action: &Action,
        contract: &Contract,
    ) -> Result<Observation, ExecutorError> {
        let url = self
            .build_url(action, contract)
            .map_err(|e| ExecutorError::Unknown(format!("Failed to build URL: {}", e)))?;

        // Check for redacted paths before making request
        for path in &self.config.redact_paths {
            if url.contains(path) {
                return Err(ExecutorError::Unknown(format!(
                    "Redacted path in URL: {}",
                    path
                )));
            }
        }

        let mut state: Value = self
            .fetch_json(&url)
            .await
            .map_err(|e| ExecutorError::Unknown(format!("Failed to fetch: {}", e)))?;

        // Apply redaction
        self.redact(&mut state);

        // Apply truncation
        state = self.truncate(state);

        Ok(Observation::new(SourceId("rest".into()), state))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_default() {
        let config = RestObserverConfig::default();
        assert_eq!(config.base_url, "http://localhost:8080");
        assert_eq!(config.timeout_ms, 5000);
    }

    #[test]
    fn config_builder() {
        let config = RestObserverConfig::new("http://api.example.com")
            .with_timeout(10000)
            .with_redact_path("password")
            .with_redact_path("secret")
            .with_header("Authorization", "Bearer token");

        assert_eq!(config.base_url, "http://api.example.com");
        assert_eq!(config.timeout_ms, 10000);
        assert_eq!(config.redact_paths.len(), 2);
        assert_eq!(config.headers.len(), 1);
    }
}
