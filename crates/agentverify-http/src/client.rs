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

/// Errors produced while submitting receipts to Control Center
#[derive(Debug, Error)]
pub enum ControlCenterClientError {
    /// Underlying HTTP transport failed
    #[error("HTTP request failed: {0}")]
    HttpError(#[from] reqwest::Error),

    /// Configured or constructed URL is not usable
    #[error("Invalid URL: {0}")]
    InvalidUrl(String),

    /// Request or response body could not be serialized or parsed
    #[error("Response parse error: {0}")]
    ParseError(String),

    /// Request exceeded the configured timeout
    #[error("Request timeout after {0}ms")]
    Timeout(u64),

    /// Server accepted the request but refused the receipt
    #[error("Server rejected receipt: {0}")]
    Rejected(String),

    /// Credentials missing or invalid
    #[error("Unauthorized - check credentials")]
    Unauthorized,

    /// Credentials valid but lacking permission for this resource
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
    /// Create a new config with required `base_url`
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            bearer_token: None,
            timeout_ms: 10_000,            // 10s default
            max_receipt_size: 1024 * 1024, // 1MB
            redact_fields: Vec::new(),
        }
    }

    /// Set the bearer token
    #[must_use]
    pub fn with_bearer_token(mut self, token: impl Into<String>) -> Self {
        self.bearer_token = Some(token.into());
        self
    }

    /// Set the timeout
    #[must_use]
    pub fn with_timeout(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = timeout_ms;
        self
    }

    /// Add a field to redact
    #[must_use]
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
    ///
    /// # Errors
    /// Returns [`ControlCenterClientError::HttpError`] if the underlying HTTP
    /// client cannot be built from the configured timeout.
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
    ///
    /// # Errors
    /// Returns [`ControlCenterClientError::HttpError`] on transport failure,
    /// [`ControlCenterClientError::Timeout`] when the request exceeds the
    /// configured timeout, [`ControlCenterClientError::Unauthorized`] and
    /// [`ControlCenterClientError::Forbidden`] on auth rejection, and
    /// [`ControlCenterClientError::ParseError`] when the receipt or the
    /// response body cannot be serialized or parsed.
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
            ControlCenterClientError::ParseError(format!("failed to read response: {e}"))
        })?;

        serde_json::from_str(&body).map_err(|e| {
            ControlCenterClientError::ParseError(format!("failed to parse response: {e}"))
        })
    }

    /// Submit a receipt, returning UNKNOWN on timeout instead of error
    ///
    /// This is the preferred method as it implements correct UNKNOWN semantics.
    ///
    /// # Errors
    /// Propagates the same errors as [`Self::submit_receipt`], except
    /// [`ControlCenterClientError::Timeout`], which is converted into a
    /// non-accepted response.
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
            Self::redact_field(&mut value, field);
        }

        value
    }

    fn redact_field(value: &mut serde_json::Value, path: &str) {
        // Simple field redaction - just set to [REDACTED]
        if let serde_json::Value::Object(map) = value {
            for (key, val) in map.iter_mut() {
                if key == path || path == "*" {
                    *val = serde_json::Value::String("[REDACTED]".to_string());
                }
                if let serde_json::Value::Object(_) = val {
                    Self::redact_field(val, path);
                }
            }
        }
    }
}

/// Builder for creating a `ControlCenterClient`
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
    use agentverify_core::{
        ActionId, ContractId, Observation, PostconditionResult, Predicate, SourceId,
        VerificationResult,
    };

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

    #[test]
    fn submission_response_rejected() {
        let json = r#"{"accepted": false, "error": "Stale contract"}"#;
        let response: SubmissionResponse = serde_json::from_str(json).unwrap();
        assert!(!response.accepted);
        assert_eq!(response.error, Some("Stale contract".to_string()));
    }

    #[test]
    fn config_with_bearer_token() {
        let config = ControlCenterClientConfig::new("https://cc.example.com")
            .with_bearer_token("my-token")
            .with_timeout(5000)
            .with_redact_field("signature");

        assert!(config.bearer_token.is_some());
        assert_eq!(config.bearer_token.unwrap(), "my-token");
        assert_eq!(config.timeout_ms, 5000);
        assert_eq!(config.redact_fields.len(), 1);
    }

    #[test]
    fn client_creation() {
        let client =
            ControlCenterClient::new(ControlCenterClientConfig::new("https://cc.example.com"));
        assert!(client.is_ok());
    }

    #[test]
    fn submission_response_no_correlation_id() {
        let json = r#"{"accepted": true}"#;
        let response: SubmissionResponse = serde_json::from_str(json).unwrap();
        assert!(response.accepted);
        assert!(response.correlation_id.is_none());
    }

    #[test]
    fn error_types_contain_meaningful_messages() {
        // Verify error variants have proper display implementations
        let err = ControlCenterClientError::Timeout(5000);
        assert!(err.to_string().contains("5000"));

        let err = ControlCenterClientError::Unauthorized;
        assert!(err.to_string().contains("Unauthorized"));

        let err = ControlCenterClientError::Forbidden;
        assert!(err.to_string().contains("Forbidden"));

        let err = ControlCenterClientError::Rejected("Stale contract".to_string());
        assert!(err.to_string().contains("Stale contract"));
    }

    #[test]
    fn config_chain_methods() {
        // Test that builder methods chain correctly
        let config = ControlCenterClientConfig::new("http://localhost:8080")
            .with_bearer_token("token123")
            .with_timeout(3000)
            .with_redact_field("password")
            .with_redact_field("secret");

        assert_eq!(config.base_url, "http://localhost:8080");
        assert_eq!(config.bearer_token, Some("token123".to_string()));
        assert_eq!(config.timeout_ms, 3000);
        assert_eq!(config.redact_fields.len(), 2);
    }

    #[test]
    fn redact_field_chaining() {
        let config = ControlCenterClientConfig::new("http://localhost:8080")
            .with_redact_field("field1")
            .with_redact_field("field2")
            .with_redact_field("field3");

        assert_eq!(config.redact_fields.len(), 3);
    }

    #[test]
    fn builder_sets_every_config_knob() {
        let client = ControlCenterClientBuilder::new()
            .base_url("https://cc.eu-west.example.com")
            .bearer_token("cc-token-9")
            .timeout_ms(1_500)
            .max_receipt_size(2_048)
            .redact_field("signature")
            .redact_field("key_id")
            .build()
            .expect("a fully specified config must build a client");

        assert_eq!(client.config.base_url, "https://cc.eu-west.example.com");
        assert_eq!(client.config.bearer_token.as_deref(), Some("cc-token-9"));
        assert_eq!(client.config.timeout_ms, 1_500);
        assert_eq!(client.config.max_receipt_size, 2_048);
        assert_eq!(
            client.config.redact_fields,
            vec!["signature".to_string(), "key_id".to_string()]
        );
    }

    #[test]
    fn builder_defaults_to_the_default_config() {
        let client = ControlCenterClientBuilder::new()
            .build()
            .expect("the default config must build a client");

        assert_eq!(client.config.base_url, "http://localhost:8080");
        assert_eq!(client.config.max_receipt_size, 1024 * 1024);
    }

    #[tokio::test]
    async fn oversized_receipt_is_refused_without_leaving_the_process() {
        let server = wiremock::MockServer::start().await;

        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/api/v1/verification-receipts"))
            .respond_with(wiremock::ResponseTemplate::new(200))
            .expect(0)
            .mount(&server)
            .await;

        // A 64-byte cap is far below any real receipt, which carries a
        // 64-character digest plus evidence.
        let client = ControlCenterClientBuilder::new()
            .base_url(server.uri())
            .max_receipt_size(64)
            .build()
            .expect("capped client builds");

        let response = client
            .submit_receipt(&sample_receipt())
            .await
            .expect("the size guard refuses in-band rather than erroring");

        assert!(!response.accepted);
        assert_eq!(response.correlation_id, None);
        assert_eq!(
            response.error.as_deref(),
            Some("Receipt exceeds maximum size")
        );
        assert!(
            server.received_requests().await.unwrap().is_empty(),
            "an oversized receipt must not reach the server"
        );
    }

    #[test]
    fn redact_receipt_applies_each_configured_field() {
        let client = ControlCenterClient::new(
            ControlCenterClientConfig::new("http://localhost:8080")
                .with_redact_field("key_id")
                .with_redact_field("idempotency_key"),
        )
        .expect("client builds");

        let redacted = client.redact_receipt(&sample_receipt());

        assert_eq!(redacted["key_id"], "[REDACTED]");
        assert_eq!(redacted["idempotency_key"], "[REDACTED]");
        // Everything outside the redaction list stays intact.
        assert_eq!(redacted["result"], "verified");
        assert_eq!(redacted["observations"][0]["source"], "postgres");
        assert_eq!(redacted["attempts"], 2);
    }

    #[test]
    fn redact_field_descends_into_nested_objects() {
        let mut value = serde_json::json!({
            "receipt": {
                "key_id": "vk-2026-08",
                "issued_by": { "key_id": "vk-2026-08", "region": "eu-west-1" },
            },
            "key_id": "vk-2026-08",
        });

        ControlCenterClient::redact_field(&mut value, "key_id");

        assert_eq!(value["key_id"], "[REDACTED]");
        assert_eq!(value["receipt"]["key_id"], "[REDACTED]");
        assert_eq!(value["receipt"]["issued_by"]["key_id"], "[REDACTED]");
        assert_eq!(value["receipt"]["issued_by"]["region"], "eu-west-1");
    }

    #[test]
    fn redact_field_wildcard_scrubs_every_value() {
        let mut value = serde_json::json!({
            "result": "verified",
            "attempts": 2,
            "nested": { "digest": "abc", "keep": true },
        });

        ControlCenterClient::redact_field(&mut value, "*");

        assert_eq!(value["result"], "[REDACTED]");
        assert_eq!(value["attempts"], "[REDACTED]");
        // A wildcard replaces a whole subtree in one step, so nested content
        // never survives.
        assert_eq!(value["nested"], "[REDACTED]");
    }

    #[test]
    fn redact_field_ignores_non_object_payloads() {
        let mut scalar = serde_json::Value::String("verified".to_string());
        ControlCenterClient::redact_field(&mut scalar, "result");
        assert_eq!(scalar, serde_json::Value::String("verified".to_string()));

        let mut array = serde_json::json!(["verified", 2]);
        ControlCenterClient::redact_field(&mut array, "0");
        assert_eq!(array, serde_json::json!(["verified", 2]));
    }

    #[test]
    fn invalid_url_error_is_descriptive() {
        let error = ControlCenterClientError::InvalidUrl("no scheme in url".to_string());
        assert_eq!(error.to_string(), "Invalid URL: no scheme in url");
    }

    /// A receipt with evidence and a postcondition outcome, close to what the
    /// runtime produces after a successful verification.
    fn sample_receipt() -> Receipt {
        Receipt::with_contract_version_and_key(
            ActionId::new(),
            ContractId::new(),
            "1.0.0",
            VerificationResult::Verified,
            2,
            Some("create-customer-42".to_string()),
        )
        .with_observation(Observation::new(
            SourceId("postgres".to_string()),
            serde_json::json!({ "customers": [{ "id": 42 }] }),
        ))
        .with_postcondition_result(PostconditionResult {
            predicate: Predicate::Exists {
                path: "customers.0.id".to_string(),
            },
            description: "customer row exists".to_string(),
            passed: true,
            error: None,
        })
        .with_key_id("vk-2026-08")
    }
}
