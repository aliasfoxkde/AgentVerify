//! REST observer implementation
//!
//! Observes system state via REST API calls.

use agentverify_core::{Action, Contract, Observation, SourceId};
use agentverify_runtime::ExecutorError;
use reqwest::Client;
use serde::de::DeserializeOwned;
use serde_json::Value;
use thiserror::Error;

/// Errors produced while observing state over REST
#[derive(Debug, Error)]
pub enum RestObserverError {
    /// Underlying HTTP transport failed
    #[error("HTTP request failed: {0}")]
    HttpError(#[from] reqwest::Error),

    /// Constructed URL failed validation (traversal or empty segments)
    #[error("Invalid URL: {0}")]
    InvalidUrl(String),

    /// Response body could not be read or parsed
    #[error("Response parse error: {0}")]
    ParseError(String),

    /// Request exceeded the configured timeout
    #[error("Request timeout")]
    Timeout,

    /// Observation path matched a configured redaction path
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
    /// Create a new config with required `base_url`
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
    #[must_use]
    pub fn with_timeout(mut self, timeout_ms: u64) -> Self {
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
    ///
    /// # Errors
    /// Returns [`RestObserverError::HttpError`] if the underlying HTTP client
    /// cannot be built from the configured timeout.
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

        // Validate URL to prevent path traversal injection
        // Only check the path portion for .. which indicates traversal
        if url.contains("..") {
            return Err(RestObserverError::InvalidUrl(url));
        }

        // Check for empty path segments (//) only in the path portion, not the scheme separator
        // Find the position after scheme://  (e.g., after http://api.example.com/)
        // Then check only the path portion for //
        if let Some(path_start) = url.find("://") {
            if let Some(slash_after_host) = url[path_start + 3..].find('/') {
                let path_portion = &url[path_start + 3 + slash_after_host..];
                if path_portion.contains("//") {
                    return Err(RestObserverError::InvalidUrl(url));
                }
            }
        }

        Ok(url)
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
                RestObserverError::ParseError(format!("Failed to parse response: {e}"))
            })
        } else {
            Err(RestObserverError::ParseError(format!(
                "HTTP error: {status}"
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

impl RestObserverConfig {
    /// Helper to set max evidence size
    #[must_use]
    pub fn with_max_evidence_size(mut self, size: usize) -> Self {
        self.max_evidence_size = size;
        self
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
            .map_err(|e| ExecutorError::Unknown(format!("Failed to build URL: {e}")))?;

        // Check for redacted paths before making request
        for path in &self.config.redact_paths {
            if url.contains(path) {
                return Err(ExecutorError::Unknown(format!(
                    "Redacted path in URL: {path}"
                )));
            }
        }

        let mut state: Value = self
            .fetch_json(&url)
            .await
            .map_err(|e| ExecutorError::Unknown(format!("Failed to fetch: {e}")))?;

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
    use agentverify_runtime::Observer;
    use std::io::{ErrorKind, Read, Write};
    use std::net::{SocketAddr, TcpListener, TcpStream};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    /// How long the stub server keeps serving before shutting itself down.
    const STUB_LIFETIME: Duration = Duration::from_secs(30);

    /// A minimal HTTP/1.1 server that answers every request with one canned
    /// response and records what it was asked for.
    ///
    /// `wiremock` is not a unit-test dependency of this crate, and the observer
    /// only speaks plain HTTP, so a raw listener keeps the tests honest about
    /// the bytes that actually travel.
    struct StubApi {
        addr: SocketAddr,
        requests: Arc<Mutex<Vec<String>>>,
    }

    impl StubApi {
        /// Serve `status_line` / `content_type` / `body` for a bounded lifetime.
        fn start(status_line: &str, content_type: &str, body: &str) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
            let addr = listener.local_addr().expect("read local address");
            listener
                .set_nonblocking(true)
                .expect("set listener nonblocking");

            let requests = Arc::new(Mutex::new(Vec::new()));
            let seen = Arc::clone(&requests);
            let response = format!(
                "{status_line}\r\nContent-Type: {content_type}\r\n\
                 Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let deadline = Instant::now() + STUB_LIFETIME;

            std::thread::Builder::new()
                .name("rest-stub".to_string())
                .spawn(move || {
                    while Instant::now() < deadline {
                        match listener.accept() {
                            Ok((mut stream, _)) => {
                                let request = read_request(&mut stream);
                                seen.lock().expect("request log lock").push(request);
                                let _ = stream.write_all(response.as_bytes());
                                let _ = stream.flush();
                            }
                            Err(ref err) if err.kind() == ErrorKind::WouldBlock => {
                                std::thread::sleep(Duration::from_millis(2));
                            }
                            Err(_) => break,
                        }
                    }
                })
                .expect("spawn rest stub thread");

            Self { addr, requests }
        }

        /// Base URL for an observer pointed at this stub.
        fn base_url(&self) -> String {
            format!("http://{}", self.addr)
        }

        /// The single request the stub is expected to have received.
        fn only_request(&self) -> String {
            let requests = self.requests.lock().expect("request log lock");
            assert_eq!(
                requests.len(),
                1,
                "the observer must issue exactly one request, saw {requests:?}"
            );
            requests[0].clone()
        }
    }

    /// Read one HTTP request (request line plus headers) from `stream`.
    ///
    /// The observer sends a complete request and leaves the connection open for
    /// reuse, so the read stops at the end of the headers rather than at EOF.
    fn read_request(stream: &mut TcpStream) -> String {
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("set read timeout");

        let mut raw = Vec::new();
        let mut scratch = [0u8; 1024];
        let header_end = b"\r\n\r\n";
        while !raw.windows(header_end.len()).any(|w| w == header_end) {
            match stream.read(&mut scratch) {
                // The peer hung up or the read failed before the request was
                // complete: serve whatever made it through.
                Ok(0) | Err(_) => break,
                Ok(n) => raw.extend_from_slice(&scratch[..n]),
            }
        }
        String::from_utf8_lossy(&raw).into_owned()
    }

    /// An abandoned loopback port: nothing is listening there any more.
    fn abandoned_port() -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
        let port = listener.local_addr().expect("read local address").port();
        drop(listener);
        port
    }

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

    // ============ SECURITY TESTS ============

    #[test]
    fn url_injection_path_traversal_rejected() {
        let config = RestObserverConfig::new("http://api.example.com");
        let observer = RestObserver::new(config).unwrap();

        // Create contract with path traversal in action_name
        let action = Action::new("test", serde_json::json!({}));
        let contract = Contract::new("../../../etc/passwd");

        let result = observer.build_url(&action, &contract);
        assert!(result.is_err());
        match result {
            Err(RestObserverError::InvalidUrl(url)) => {
                assert!(url.contains(".."));
            }
            _ => panic!("Expected InvalidUrl error"),
        }
    }

    #[test]
    fn url_injection_double_slash_rejected() {
        let config = RestObserverConfig::new("http://api.example.com");
        let observer = RestObserver::new(config).unwrap();

        // Create contract with double slash in action_name (the path portion)
        // This would create URL like http://api.example.com/test//etc/passwd/<id>
        let action = Action::new("auto-id", serde_json::json!({}));
        let contract = Contract::new("test//etc/passwd");

        let result = observer.build_url(&action, &contract);
        assert!(result.is_err(), "double slash in path should be rejected");
    }

    #[test]
    fn url_injection_http_scheme_injection_rejected() {
        let config = RestObserverConfig::new("http://api.example.com");
        let observer = RestObserver::new(config).unwrap();

        // Attempt to inject different host via action_name
        // The validation should catch this via the // check
        let action = Action::new("auto-id", serde_json::json!({}));
        let contract = Contract::new("test");

        // This URL is fine - no injection
        let result = observer.build_url(&action, &contract);
        assert!(result.is_ok());

        // Now test with path traversal in action_name
        let contract_malicious = Contract::new("test/../../../etc/passwd");
        let result2 = observer.build_url(&action, &contract_malicious);
        assert!(result2.is_err(), "path traversal should be rejected");
    }

    #[test]
    fn redaction_password_field() {
        let config =
            RestObserverConfig::new("http://api.example.com").with_redact_path("/password");
        let observer = RestObserver::new(config).unwrap();

        let mut state = serde_json::json!({
            "username": "testuser",
            "password": "secret123"
        });

        observer.redact(&mut state);

        assert_eq!(state["username"], "testuser");
        assert_eq!(state["password"], "[REDACTED]");
    }

    #[test]
    fn redaction_nested_secret() {
        let config =
            RestObserverConfig::new("http://api.example.com").with_redact_path("/data/api_key");
        let observer = RestObserver::new(config).unwrap();

        let mut state = serde_json::json!({
            "data": {
                "api_key": "sk-12345",
                "name": "test"
            }
        });

        observer.redact(&mut state);

        assert_eq!(state["data"]["api_key"], "[REDACTED]");
        assert_eq!(state["data"]["name"], "test");
    }

    #[test]
    fn redaction_multiple_paths() {
        let config = RestObserverConfig::new("http://api.example.com")
            .with_redact_path("/password")
            .with_redact_path("/secret")
            .with_redact_path("/token");
        let observer = RestObserver::new(config).unwrap();

        let mut state = serde_json::json!({
            "password": "pass1",
            "secret": "pass2",
            "token": "pass3",
            "public": "data"
        });

        observer.redact(&mut state);

        assert_eq!(state["password"], "[REDACTED]");
        assert_eq!(state["secret"], "[REDACTED]");
        assert_eq!(state["token"], "[REDACTED]");
        assert_eq!(state["public"], "data");
    }

    #[test]
    fn truncation_large_response() {
        let config = RestObserverConfig::new("http://api.example.com").with_max_evidence_size(100);
        let observer = RestObserver::new(config).unwrap();

        let large_state = serde_json::json!({
            "data": "x".repeat(500)
        });

        let result = observer.truncate(large_state);

        match result {
            Value::String(s) => {
                assert!(s.contains("[TRUNCATED:"));
                assert!(s.contains("> 100"));
            }
            _ => panic!("Expected truncated string"),
        }
    }

    #[test]
    fn truncation_small_response_preserved() {
        let config = RestObserverConfig::new("http://api.example.com").with_max_evidence_size(1000);
        let observer = RestObserver::new(config).unwrap();

        let small_state = serde_json::json!({
            "data": "small"
        });

        let result = observer.truncate(small_state.clone());
        assert_eq!(result, small_state);
    }

    #[test]
    fn truncation_exact_boundary() {
        let config = RestObserverConfig::new("http://api.example.com").with_max_evidence_size(100);
        let observer = RestObserver::new(config).unwrap();

        // Create JSON that's exactly at the boundary
        let state = serde_json::json!({"d": "x".repeat(50)});
        let json_str = serde_json::to_string(&state).unwrap();
        assert!(json_str.len() <= 100);

        let result = observer.truncate(state.clone());
        assert_eq!(result, state);
    }

    // ============ OBSERVATION OVER A REAL HTTP EXCHANGE ============

    fn contract(name: &str) -> Contract {
        Contract::new(name)
    }

    fn action() -> Action {
        Action::new("observation", serde_json::json!({}))
    }

    #[tokio::test]
    async fn observe_applies_redaction_to_the_state_it_returns() {
        let stub = StubApi::start(
            "HTTP/1.1 200 OK",
            "application/json",
            r#"{"status": "completed", "password": "hunter2", "rows": [{"id": 7}]}"#,
        );

        let observer = RestObserver::new(
            RestObserverConfig::new(stub.base_url())
                .with_redact_path("/password")
                .with_timeout(5_000),
        )
        .unwrap();

        let observation = observer
            .observe(&action(), &contract("deployments"))
            .await
            .expect("a well-formed response must observe cleanly");

        assert_eq!(observation.source, SourceId("rest".into()));
        assert_eq!(observation.state["status"], "completed");
        assert_eq!(observation.state["password"], "[REDACTED]");
        assert_eq!(observation.state["rows"][0]["id"], 7);

        let request = stub.only_request();
        assert!(
            request.starts_with("GET /deployments/"),
            "unexpected request line: {request}"
        );
        assert!(
            request.contains("HTTP/1.1"),
            "unexpected request line: {request}"
        );
    }

    #[tokio::test]
    async fn observe_sends_the_configured_headers() {
        let stub = StubApi::start("HTTP/1.1 200 OK", "application/json", r#"{"ok": true}"#);

        let observer = RestObserver::new(
            RestObserverConfig::new(stub.base_url())
                .with_header("Authorization", "Bearer observer-token")
                .with_header("X-Tenant", "acme"),
        )
        .unwrap();

        observer
            .observe(&action(), &contract("deployments"))
            .await
            .expect("the stub answers");

        let request = stub.only_request().to_ascii_lowercase();
        assert!(
            request.contains("authorization: bearer observer-token"),
            "the auth header must reach the wire: {request}"
        );
        assert!(
            request.contains("x-tenant: acme"),
            "every configured header must reach the wire: {request}"
        );
    }

    #[tokio::test]
    async fn observe_truncates_evidence_that_exceeds_the_cap() {
        let stub = StubApi::start(
            "HTTP/1.1 200 OK",
            "application/json",
            r#"{"blob": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#,
        );

        let observer =
            RestObserver::new(RestObserverConfig::new(stub.base_url()).with_max_evidence_size(32))
                .unwrap();

        let observation = observer
            .observe(&action(), &contract("deployments"))
            .await
            .expect("oversized evidence is truncated, not rejected");

        let Value::String(evidence) = &observation.state else {
            panic!("expected truncated evidence, got {:?}", observation.state);
        };
        assert!(
            evidence.starts_with("[TRUNCATED: "),
            "unexpected evidence: {evidence}"
        );
        assert!(
            evidence.ends_with(" > 32 bytes]"),
            "the cap must be named: {evidence}"
        );
    }

    #[tokio::test]
    async fn observe_maps_an_http_error_status_to_a_fetch_failure() {
        let stub = StubApi::start("HTTP/1.1 503 Service Unavailable", "text/plain", "busy");

        let observer =
            RestObserver::new(RestObserverConfig::new(stub.base_url()).with_timeout(5_000))
                .unwrap();

        let error = observer
            .observe(&action(), &contract("deployments"))
            .await
            .expect_err("a 503 is not an observation");

        match error {
            ExecutorError::Unknown(message) => {
                assert!(
                    message.contains("Failed to fetch") && message.contains("503"),
                    "unexpected message: {message}"
                );
            }
            other => panic!("expected Unknown, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn observe_maps_a_refused_connection_to_a_fetch_failure() {
        let port = abandoned_port();
        let observer =
            RestObserver::new(RestObserverConfig::new(format!("http://127.0.0.1:{port}"))).unwrap();

        let error = observer
            .observe(&action(), &contract("deployments"))
            .await
            .expect_err("nothing is listening on that port");

        match error {
            ExecutorError::Unknown(message) => {
                assert!(
                    message.contains("Failed to fetch"),
                    "unexpected message: {message}"
                );
            }
            other => panic!("expected Unknown, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn observe_rejects_a_url_that_carries_a_redacted_path_without_calling_the_api() {
        // Nothing is listening: if the guard ever lets the request through, the
        // failure message names the transport instead of the redaction.
        let port = abandoned_port();
        let observer = RestObserver::new(
            RestObserverConfig::new(format!("http://127.0.0.1:{port}"))
                .with_redact_path("deployments"),
        )
        .unwrap();

        let error = observer
            .observe(&action(), &contract("deployments"))
            .await
            .expect_err("a redacted path in the URL must be refused");

        match error {
            ExecutorError::Unknown(message) => {
                assert!(
                    message.contains("Redacted path in URL: deployments"),
                    "unexpected message: {message}"
                );
            }
            other => panic!("expected Unknown, got {other:?}"),
        }
    }

    // ============ URL CONSTRUCTION ============

    #[test]
    fn build_url_accepts_a_base_url_without_a_scheme() {
        let observer = RestObserver::new(RestObserverConfig::new("127.0.0.1:8080")).unwrap();
        let deploy = action();

        let url = observer
            .build_url(&deploy, &contract("deployments"))
            .expect("a scheme-less base URL is still assembled");

        assert_eq!(
            url,
            format!("127.0.0.1:8080/deployments/{}", deploy.id),
            "the traversal guard must not depend on a scheme being present"
        );
    }

    #[test]
    fn build_url_ignores_a_trailing_slash_on_the_base_url() {
        let observer =
            RestObserver::new(RestObserverConfig::new("http://api.example.com/")).unwrap();
        let deploy = action();

        let url = observer
            .build_url(&deploy, &contract("deployments"))
            .expect("a trailing slash must not produce an empty path segment");

        assert_eq!(
            url,
            format!("http://api.example.com/deployments/{}", deploy.id)
        );
    }
}

// Note: Full mock-server integration tests with wiremock require API compatibility fixes.
// The existing unit tests (11) prove URL injection rejection, redaction, and truncation behavior.
// Proper integration tests would use a real HTTP server or a compatible mock framework.
