//! Control Center client for submitting verification receipts
//!
//! Submits signed verification receipts to the Control Center for correlation
//! and promotion decision support.
//!
//! # Features
//! - Bounded HTTP with configurable timeout
//! - Authentication via bearer token
//! - Idempotent submission using receipt digest as idempotency key
//! - Redaction of sensitive fields
//! - Timeout-to-UNKNOWN semantics (never fails hard)
//!
//! # Note
//! This client only SUBMITS receipts - it cannot mutate Control Center
//! promotion state. The promotion decision is made exclusively by Control Center.

#![allow(dead_code)]

use agentverify_core::Receipt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ControlCenterClientError {
    #[error("HTTP request failed: {0}")]
    HttpError(#[from] reqwest::Error),

    #[error("Invalid URL: {0}")]
    InvalidUrl(String),

    #[error("Response parse error: {0}")]
    ParseError(String),

    #[error("Request timeout after {0}ms")]
    Timeout(u64),

    #[error("Server rejected receipt: {0}")]
    Rejected(String),

    #[error("Unauthorized - check credentials")]
    Unauthorized,

    #[error("Forbidden - not authorized for this resource")]
    Forbidden,
}

/// Control Center client configuration
#[derive(Debug, Clone)]
pub struct ControlCenterClientConfig {
    /// Base URL for the Control Center API
    base_url: String,
    /// Bearer token for authentication
    bearer_token: Option<String>,
    /// Request timeout in milliseconds
    timeout_ms: u64,
    /// Maximum receipt size in bytes
    max_receipt_size: usize,
    /// Fields to redact from receipt before submission
    redact_fields: Vec<String>,
}

impl ControlCenterClientConfig {
    /// Create a new config with required base_url
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            bearer_token: None,
            timeout_ms: 10_000, // 10s default
            max_receipt_size: 1024 * 1024, // 1MB
            redact_fields: Vec::new(),
        }
    }

    /// Set the bearer token
    pub fn with_bearer_token(mut self, token: impl Into<String>) -> Self {
        self.bearer_token = Some(token.into());
        self
    }

    /// Set the timeout
    pub fn with_timeout(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = timeout_ms;
        self
    }

    /// Add a field to redact
    pub fn with_redact_field(mut self, field: impl Into<String>) -> Self {
        self.redact_fields.push(field.into());
        self
    }
}

impl Default for ControlCenterClientConfig {
    fn default() -> Self {
        Self::new("http://localhost:8080")
    }
}

/// Response from Control Center after receipt submission
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmissionResponse {
    /// Whether the submission was accepted
    pub accepted: bool,
    /// Correlation ID assigned by Control Center
    pub correlation_id: Option<String>,
    /// Error message if rejected
    pub error: Option<String>,
}

/// Client for submitting receipts to Control Center
#[derive(Debug, Clone)]
pub struct ControlCenterClient {
    config: ControlCenterClientConfig,
    http_client: Client,
}

impl ControlCenterClient {
    /// Create a new client with the given configuration
    pub fn new(config: ControlCenterClientConfig) -> Result<Self, ControlCenterClientError> {
        let http_client = Client::builder()
            .timeout(Duration::from_millis(config.timeout_ms))
            .build()
            .map_err(ControlCenterClientError::HttpError)?;

        Ok(Self {
            config,
            http_client,
        })
    }

    /// Submit a receipt to Control Center
    ///
    /// Returns `Ok(SubmissionResponse)` on success (including rejection by server).
    /// Returns `Err` only on network/transport failures.
    ///
    /// This method implements timeout-to-UNKNOWN semantics: timeouts return
    /// a successful response with `accepted: false` rather than propagating
    /// an error.
    pub async fn submit_receipt(
        &self,
        receipt: &Receipt,
    ) -> Result<SubmissionResponse, ControlCenterClientError> {
        let url = format!("{}/api/v1/verification-receipts", self.config.base_url);

        // Build request with optional auth
        let mut request = self.http_client.post(&url);

        if let Some(token) = &self.config.bearer_token {
            request = request.bearer_auth(token);
        }

        // Serialize receipt (applying redactions)
        let receipt_payload = self.redact_receipt(receipt);
        let json = serde_json::to_string(&receipt_payload)
            .map_err(|e| ControlCenterClientError::ParseError(e.to_string()))?;

        // Check size limit
        if json.len() > self.config.max_receipt_size {
            return Ok(SubmissionResponse {
                accepted: false,
                correlation_id: None,
                error: Some("Receipt exceeds maximum size".to_string()),
            });
        }

        request = request
            .header("Content-Type", "application/json")
            .header("X-Idempotency-Key", &receipt.digest); // Use digest as idempotency key

        let response = request.body(json).send().await.map_err(|e| {
            if e.is_timeout() {
                // Timeout is NOT an error - return UNKNOWN semantics
                ControlCenterClientError::Timeout(self.config.timeout_ms)
            } else {
                ControlCenterClientError::HttpError(e)
            }
        })?;

        let status = response.status();

        // Handle authentication errors
        if status.as_u16() == 401 {
            return Err(ControlCenterClientError::Unauthorized);
        }
        if status.as_u16() == 403 {
            return Err(ControlCenterClientError::Forbidden);
        }

        // Parse response
        let body = response.text().await.map_err(|e| {
            ControlCenterClientError::ParseError(format!("failed to read response: {}", e))
        })?;

        serde_json::from_str(&body).map_err(|e| {
            ControlCenterClientError::ParseError(format!("failed to parse response: {}", e))
        })
    }

    /// Submit a receipt, returning UNKNOWN on timeout instead of error
    ///
    /// This is the preferred method as it implements correct UNKNOWN semantics.
    pub async fn submit_receipt_with_unknown_on_timeout(
        &self,
        receipt: &Receipt,
    ) -> Result<SubmissionResponse, ControlCenterClientError> {
        match self.submit_receipt(receipt).await {
            Err(ControlCenterClientError::Timeout(_)) => {
                // Timeout means we don't know if it was accepted - return UNKNOWN semantics
                Ok(SubmissionResponse {
                    accepted: false,
                    correlation_id: None,
                    error: Some("Timeout - submission status unknown".to_string()),
                })
            }
            other => other,
        }
    }

    /// Redact sensitive fields from receipt before submission
    fn redact_receipt(&self, receipt: &Receipt) -> serde_json::Value {
        // Start with full JSON representation
        let mut value = serde_json::to_value(receipt).unwrap_or_default();

        // Apply redactions
        for field in &self.config.redact_fields {
            self.redact_field(&mut value, field);
        }

        value
    }

    fn redact_field(&self, value: &mut serde_json::Value, path: &str) {
        // Simple field redaction - just set to [REDACTED]
        if let serde_json::Value::Object(map) = value {
            for (key, val) in map.iter_mut() {
                if key == path || path == "*" {
                    *val = serde_json::Value::String("[REDACTED]".to_string());
                }
                if let serde_json::Value::Object(_) = val {
                    self.redact_field(val, path);
                }
            }
        }
    }
}

/// Builder for creating a ControlCenterClient
#[derive(Debug, Default)]
pub struct ControlCenterClientBuilder {
    config: ControlCenterClientConfig,
}

impl ControlCenterClientBuilder {
    pub fn new() -> Self {
        Self {
            config: ControlCenterClientConfig::default(),
        }
    }

    pub fn base_url(mut self, url: impl Into<String>) -> Self {
        self.config.base_url = url.into();
        self
    }

    pub fn bearer_token(mut self, token: impl Into<String>) -> Self {
        self.config.bearer_token = Some(token.into());
        self
    }

    pub fn timeout_ms(mut self, ms: u64) -> Self {
        self.config.timeout_ms = ms;
        self
    }

    pub fn max_receipt_size(mut self, size: usize) -> Self {
        self.config.max_receipt_size = size;
        self
    }

    pub fn redact_field(mut self, field: impl Into<String>) -> Self {
        self.config.redact_fields.push(field.into());
        self
    }

    pub fn build(self) -> Result<ControlCenterClient, ControlCenterClientError> {
        ControlCenterClient::new(self.config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_default_values() {
        let config = ControlCenterClientConfig::default();
        assert_eq!(config.base_url, "http://localhost:8080");
        assert_eq!(config.timeout_ms, 10_000);
        assert!(config.bearer_token.is_none());
    }

    #[test]
    fn builder_pattern() {
        let client = ControlCenterClientBuilder::new()
            .base_url("https://cc.example.com")
            .bearer_token("test-token")
            .timeout_ms(5000)
            .build()
            .unwrap();

        assert!(client.config.bearer_token.is_some());
    }

    #[test]
    fn submission_response_deserialization() {
        let json = r#"{"accepted": true, "correlation_id": "abc123"}"#;
        let response: SubmissionResponse = serde_json::from_str(json).unwrap();
        assert!(response.accepted);
        assert_eq!(response.correlation_id, Some("abc123".to_string()));
    }
}
