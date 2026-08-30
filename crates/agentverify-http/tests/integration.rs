#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Integration tests for REST observer using wiremock
//!
//! Tests authenticated mock server interactions: success, unauthorized access,
//! malformed response, oversized response, and timeout handling.

use agentverify_core::{Action, Contract, SourceId};
use agentverify_http::{RestObserver, RestObserverConfig};
use agentverify_runtime::Observer;
use std::time::Duration;
use wiremock::matchers::{method, path_regex};
use wiremock::{Mock, MockServer, ResponseTemplate};

// Test helper to create observer with auth header
fn make_observer(base_url: &str, auth_header: Option<(&str, &str)>) -> RestObserver {
    let mut config = RestObserverConfig::new(base_url);
    if let Some((k, v)) = auth_header {
        config = config.with_header(k, v);
    }
    // Small timeout for test responsiveness
    config = config.with_timeout(2000);
    RestObserver::new(config).unwrap()
}

#[tokio::test]
async fn mock_server_success_with_valid_auth() {
    // Start mock server
    let mock_server = MockServer::start().await;

    // Match any path under /test/ (UUID is auto-generated)
    Mock::given(method("GET"))
        .and(path_regex(r"^/test/.+$"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": {
                "status": "completed",
                "value": 42
            }
        })))
        .mount(&mock_server)
        .await;

    // Create observer with auth header
    let observer = make_observer(
        &mock_server.uri(),
        Some(("Authorization", "Bearer test-token")),
    );

    // action.name goes into contract.action_name, action.id is auto-generated UUID
    let action = Action::new("auto-generated-id", serde_json::json!({}));
    let contract = Contract::new("test");

    let result = observer.observe(&action, &contract).await;
    assert!(result.is_ok(), "expected success, got {result:?}");

    let observation = result.unwrap();
    assert_eq!(observation.source, SourceId("rest".into()));
    assert_eq!(observation.state["result"]["status"], "completed");
}

#[tokio::test]
async fn mock_server_unauthorized_missing_auth() {
    let mock_server = MockServer::start().await;

    // Server requires auth but client doesn't send it
    Mock::given(method("GET"))
        .and(path_regex(r"^/test/.+$"))
        .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
            "error": "Unauthorized"
        })))
        .mount(&mock_server)
        .await;

    let observer = make_observer(&mock_server.uri(), None); // No auth header

    let action = Action::new("auto-generated-id", serde_json::json!({}));
    let contract = Contract::new("test");

    // HTTP 401 returns error (wrapped as Unknown) - this is correct behavior
    let result = observer.observe(&action, &contract).await;
    assert!(result.is_err(), "401 should return error, got {result:?}");
    let err_str = format!("{result:?}");
    assert!(
        err_str.contains("401") || err_str.contains("Unauthorized"),
        "Expected 401 error, got: {err_str}"
    );
}

#[tokio::test]
async fn mock_server_unauthorized_invalid_token() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path_regex(r"^/test/.+$"))
        .respond_with(ResponseTemplate::new(403).set_body_json(serde_json::json!({
            "error": "Forbidden"
        })))
        .mount(&mock_server)
        .await;

    // Wrong token
    let observer = make_observer(
        &mock_server.uri(),
        Some(("Authorization", "Bearer wrong-token")),
    );

    let action = Action::new("auto-generated-id", serde_json::json!({}));
    let contract = Contract::new("test");

    // HTTP 403 returns error - auth failure is treated as error
    let result = observer.observe(&action, &contract).await;
    assert!(result.is_err(), "403 should return error, got {result:?}");
    let err_str = format!("{result:?}");
    assert!(
        err_str.contains("403") || err_str.contains("Forbidden"),
        "Expected 403 error, got: {err_str}"
    );
}

#[tokio::test]
async fn mock_server_malformed_response() {
    let mock_server = MockServer::start().await;

    // Return non-JSON response
    Mock::given(method("GET"))
        .and(path_regex(r"^/test/.+$"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("this is not json")
                .insert_header("Content-Type", "text/plain"),
        )
        .mount(&mock_server)
        .await;

    let observer = make_observer(&mock_server.uri(), None);

    let action = Action::new("auto-generated-id", serde_json::json!({}));
    let contract = Contract::new("test");

    // Parse error returns error (wrapped as Unknown)
    let result = observer.observe(&action, &contract).await;
    assert!(
        result.is_err(),
        "malformed JSON should return error, got {result:?}"
    );
}

#[tokio::test]
async fn mock_server_oversized_response() {
    let mock_server = MockServer::start().await;

    // Generate large JSON response
    let large_data = serde_json::json!({
        "data": "x".repeat(200_000) // 200KB of data
    });

    Mock::given(method("GET"))
        .and(path_regex(r"^/test/.+$"))
        .respond_with(ResponseTemplate::new(200).set_body_json(large_data))
        .mount(&mock_server)
        .await;

    // Observer with 1KB max evidence size
    let config = RestObserverConfig::new(mock_server.uri().as_str())
        .with_max_evidence_size(1024) // 1KB limit
        .with_timeout(5000);
    let observer = RestObserver::new(config).unwrap();

    let action = Action::new("auto-generated-id", serde_json::json!({}));
    let contract = Contract::new("test");

    let result = observer.observe(&action, &contract).await;
    assert!(result.is_ok());
    let observation = result.unwrap();

    // State should be truncated (a string starting with [TRUNCATED)
    let state_str = serde_json::to_string(&observation.state).unwrap();
    assert!(
        state_str.contains("[TRUNCATED"),
        "Expected truncated state, got: {}",
        &state_str[..state_str.len().min(200)]
    );
}

#[tokio::test]
async fn mock_server_timeout() {
    let mock_server = MockServer::start().await;

    // Delayed response exceeding timeout (10 second delay, observer times out at 500ms)
    Mock::given(method("GET"))
        .and(path_regex(r"^/test/.+$"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"ok": true}))
                .set_delay(Duration::from_secs(10)),
        )
        .mount(&mock_server)
        .await;

    // Observer with 500ms timeout
    let config = RestObserverConfig::new(mock_server.uri().as_str()).with_timeout(500);
    let observer = RestObserver::new(config).unwrap();

    let action = Action::new("auto-generated-id", serde_json::json!({}));
    let contract = Contract::new("test");

    // Timeout returns error (wrapped as Unknown)
    let result = observer.observe(&action, &contract).await;
    assert!(
        result.is_err(),
        "timeout should return error, got {result:?}"
    );
    let err_str = format!("{result:?}");
    assert!(
        err_str.contains("timeout") || err_str.contains("Timeout") || err_str.contains("fetch"),
        "Expected timeout error, got: {err_str}"
    );
}

#[tokio::test]
async fn mock_server_stale_read_returns_pending_status() {
    let mock_server = MockServer::start().await;

    // Return stale data (pending instead of completed)
    Mock::given(method("GET"))
        .and(path_regex(r"^/test/.+$"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": {
                "status": "pending",
                "updated_at": "2020-01-01T00:00:00Z"
            }
        })))
        .mount(&mock_server)
        .await;

    let observer = make_observer(&mock_server.uri(), Some(("Authorization", "Bearer token")));

    let action = Action::new("auto-generated-id", serde_json::json!({}));
    let contract = Contract::new("test");

    let result = observer.observe(&action, &contract).await;
    assert!(result.is_ok());
    let observation = result.unwrap();

    // Observer correctly captures the stale state
    assert_eq!(observation.state["result"]["status"], "pending");
}

#[tokio::test]
async fn mock_server_redaction_in_response() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path_regex(r"^/test/.+$"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "username": "testuser",
            "password": "supersecret",
            "api_key": "sk-12345"
        })))
        .mount(&mock_server)
        .await;

    // Observer configured to redact password and api_key
    let config = RestObserverConfig::new(mock_server.uri().as_str())
        .with_redact_path("/password")
        .with_redact_path("/api_key")
        .with_timeout(5000);
    let observer = RestObserver::new(config).unwrap();

    let action = Action::new("auto-generated-id", serde_json::json!({}));
    let contract = Contract::new("test");

    let result = observer.observe(&action, &contract).await;
    assert!(result.is_ok());
    let observation = result.unwrap();

    // Sensitive fields should be redacted
    assert_eq!(observation.state["username"], "testuser");
    assert_eq!(observation.state["password"], "[REDACTED]");
    assert_eq!(observation.state["api_key"], "[REDACTED]");
}

#[tokio::test]
async fn mock_server_action_name_that_breaks_the_url_is_unknown() {
    // No server is needed: the observer refuses to build the URL at all.
    let observer = make_observer("http://127.0.0.1:9", None);

    let action = Action::new("auto-generated-id", serde_json::json!({}));
    let contract = Contract::new("../../../etc/passwd");

    let result = observer.observe(&action, &contract).await;
    assert!(
        result.is_err(),
        "a broken URL must not produce an observation, got {result:?}"
    );
    let err_str = format!("{result:?}");
    assert!(
        err_str.contains("Failed to build URL"),
        "expected the URL failure to be reported as UNKNOWN, got: {err_str}"
    );
}
