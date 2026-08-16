//! WASM-native REST observer implementation
//!
//! Uses WasmHttpClient for HTTP operations on wasm32 targets.
//!
//! Note: This observer cannot implement the standard `Observer` trait because
//! JavaScript Promises (JsFuture) are not `Send`. WASM observers require a
//! different execution model using a WASM-specific runtime.

use crate::wasm_client::WasmHttpClient;
use agentverify_core::{Action, Contract, Observation, SourceId};
use serde_json::Value;

/// REST observer configuration for WASM
#[derive(Debug, Clone)]
pub struct WasmRestObserverConfig {
    /// Base URL for the REST API
    pub base_url: String,
    /// Default timeout in milliseconds
    pub timeout_ms: u32,
    /// Maximum evidence size in bytes
    pub max_evidence_size: usize,
    /// Paths to redact from evidence
    pub redact_paths: Vec<String>,
    /// Additional headers
    pub headers: std::collections::HashMap<String, String>,
}

impl WasmRestObserverConfig {
    /// Create a new config with required base_url
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            timeout_ms: 5000,
            max_evidence_size: 1024 * 1024, // 1MB
            redact_paths: Vec::new(),
            headers: std::collections::HashMap::new(),
        }
    }

    /// Set the timeout
    #[must_use]
    pub fn with_timeout(mut self, timeout_ms: u32) -> Self {
        self.timeout_ms = timeout_ms;
        self
    }

    /// Add a path to redact
    #[must_use]
    pub fn with_redact_path(mut self, path: impl Into<String>) -> Self {
        self.redact_paths.push(path.into());
        self
    }

    /// Add a header
    #[must_use]
    pub fn with_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(key.into(), value.into());
        self
    }

    /// Helper to set max evidence size
    #[must_use]
    pub fn with_max_evidence_size(mut self, size: usize) -> Self {
        self.max_evidence_size = size;
        self
    }
}

impl Default for WasmRestObserverConfig {
    fn default() -> Self {
        Self::new("http://localhost:8080")
    }
}

/// WASM-native REST observer using JavaScript fetch API
///
/// This observer cannot implement the standard `Observer` trait because
/// JavaScript Promises (JsFuture) are not `Send`. WASM observers require
/// a different execution model and should be used with WASM-specific runtimes.
///
/// # Example
///
/// ```ignore
/// let config = WasmRestObserverConfig::new("http://api.example.com");
/// let observer = WasmRestObserver::new(config);
/// let observation = observer.observe(&action, &contract).await?;
/// ```
#[derive(Clone)]
pub struct WasmRestObserver {
    client: WasmHttpClient,
    config: WasmRestObserverConfig,
}

impl WasmRestObserver {
    /// Create a new WASM REST observer
    pub fn new(config: WasmRestObserverConfig) -> Self {
        let client = WasmHttpClient::new(&config.base_url)
            .with_headers(config.headers.clone())
            .with_timeout(config.timeout_ms);
        Self { client, config }
    }

    /// Build the URL for observation
    fn build_url(&self, action: &Action, contract: &Contract) -> Result<String, String> {
        let action_name = &contract.action_name;
        let id = action.id;
        let url = format!(
            "{}/{}/{}",
            self.config.base_url.trim_end_matches('/'),
            action_name,
            id
        );

        if url.contains("..") {
            return Err("Invalid URL: path traversal detected".to_string());
        }

        Ok(url)
    }

    /// Fetch JSON from URL
    async fn fetch_json(&self, url: &str) -> Result<Value, String> {
        self.client
            .get_json(url)
            .await
            .map_err(|e| format!("HTTP request failed: {}", e))
    }

    /// Redact paths from state (simple recursive redaction)
    fn redact(&self, state: &mut Value, paths: &[String]) {
        fn redact_recursive(value: &mut Value, path_parts: &[&str]) {
            if path_parts.is_empty() {
                return;
            }
            match (value, path_parts) {
                (Value::Object(map), [key, rest @ ..]) => {
                    if let Some(v) = map.get_mut(*key) {
                        redact_recursive(v, rest);
                    }
                }
                (Value::Array(arr), [idx_str, rest @ ..]) => {
                    if let Ok(idx) = idx_str.parse::<usize>() {
                        if idx < arr.len() {
                            redact_recursive(&mut arr[idx], rest);
                        }
                    }
                }
                _ => {}
            }
        }

        for path in paths {
            let parts: Vec<&str> = path.trim_start_matches('$').split('.').collect();
            redact_recursive(state, &parts);
        }
    }

    /// Truncate state if too large
    fn truncate(&self, state: Value) -> Value {
        let size = serde_json::to_string(&state)
            .map(|s| s.len())
            .unwrap_or(0);

        if size > self.config.max_evidence_size {
            serde_json::json!({
                "error": "evidence_truncated",
                "size": size,
                "max": self.config.max_evidence_size
            })
        } else {
            state
        }
    }

    /// Observe state from the REST endpoint
    ///
    /// Returns an Observation on success, or an error message on failure.
    /// Unlike the standard Observer trait, this method is not `Send` because
    /// it uses JavaScript Promises internally.
    pub async fn observe(
        &self,
        action: &Action,
        contract: &Contract,
    ) -> Result<Observation, String> {
        let url = self
            .build_url(action, contract)
            .map_err(|e| format!("Failed to build URL: {}", e))?;

        for path in &self.config.redact_paths {
            if url.contains(path) {
                return Err(format!("Redacted path in URL: {}", path));
            }
        }

        let mut state: Value = self
            .fetch_json(&url)
            .await?;

        self.redact(&mut state, &self.config.redact_paths);
        state = self.truncate(state);

        Ok(Observation::new(SourceId("rest".into()), state))
    }
}
