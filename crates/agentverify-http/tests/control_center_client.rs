#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Integration tests for the Control Center client using wiremock.
//!
//! Exercises the real HTTP path: URL construction, bearer auth, idempotency
//! header, request redaction, response status handling, body parsing, size
//! limits, timeouts, and transport failures.

use agentverify_core::{
    ActionId, ContractId, Evidence, Observation, PostconditionResult, Predicate, Receipt, SourceId,
    VerificationResult,
};
use agentverify_http::{ControlCenterClient, ControlCenterClientConfig, ControlCenterClientError};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::Duration;
use wiremock::matchers::{body_partial_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// A receipt that looks like a real verification result: evidence, postcondition
/// outcomes, a digest, and an idempotency key.
fn signed_off_receipt() -> Receipt {
    let observation = Observation::new(
        SourceId("postgres".to_string()),
        serde_json::json!({ "customers": [{ "id": "cus_1042", "status": "active" }] }),
    )
    .with_evidence(Evidence::new(
        "postgres",
        serde_json::json!({ "rows_affected": 1 }),
    ));

    Receipt::with_contract_version_and_key(
        ActionId::new(),
        ContractId::new(),
        "1.2.0",
        VerificationResult::Verified,
        1,
        Some("create-customer-cus_1042".to_string()),
    )
    .with_observation(observation)
    .with_postcondition_result(PostconditionResult {
        predicate: Predicate::Equals {
            path: "customers.0.status".to_string(),
            value: serde_json::json!("active"),
        },
        description: "customer row exists and is active".to_string(),
        passed: true,
        error: None,
    })
    .with_key_id("vk-2026-08")
}

fn client(base_url: &str) -> ControlCenterClient {
    ControlCenterClient::new(
        ControlCenterClientConfig::new(base_url)
            .with_bearer_token("cc-token-417")
            .with_timeout(2_000),
    )
    .unwrap()
}

fn accepted(correlation_id: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(serde_json::json!({
        "accepted": true,
        "correlation_id": correlation_id,
    }))
}

#[tokio::test]
async fn submits_a_receipt_and_reads_the_correlation_id() {
    let server = MockServer::start().await;
    let receipt = signed_off_receipt();

    Mock::given(method("POST"))
        .and(path("/api/v1/verification-receipts"))
        .and(header("Authorization", "Bearer cc-token-417"))
        .and(header("X-Idempotency-Key", receipt.digest.as_str()))
        .and(header("Content-Type", "application/json"))
        .respond_with(accepted("corr-7712"))
        .mount(&server)
        .await;

    let response = client(&server.uri())
        .submit_receipt(&receipt)
        .await
        .expect("accepted receipts must not error");

    assert!(response.accepted);
    assert_eq!(response.correlation_id.as_deref(), Some("corr-7712"));
    assert!(response.error.is_none());
}

#[tokio::test]
async fn request_body_carries_the_receipt_and_redacted_fields() {
    let server = MockServer::start().await;
    let receipt = signed_off_receipt();

    Mock::given(method("POST"))
        .and(path("/api/v1/verification-receipts"))
        .and(body_partial_json(serde_json::json!({
            "version": receipt.version,
            "action_id": receipt.action_id.to_string(),
            "contract_id": receipt.contract_id.to_string(),
            "result": "verified",
            "digest": receipt.digest,
            "key_id": "[REDACTED]",
        })))
        .respond_with(accepted("corr-0001"))
        .mount(&server)
        .await;

    let submitted = ControlCenterClientConfig::new(server.uri().as_str())
        .with_bearer_token("cc-token-417")
        .with_redact_field("key_id")
        .with_timeout(2_000);
    let submitted_client = ControlCenterClient::new(submitted).unwrap();

    submitted_client
        .submit_receipt(&receipt)
        .await
        .expect("redacted submission must reach the server");

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);

    let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert_eq!(body["key_id"], "[REDACTED]");
    assert_eq!(body["result"], "verified");
    assert_eq!(
        body["observations"][0]["source"], "postgres",
        "only the configured fields may be redacted"
    );

    let idempotency = requests[0]
        .headers
        .get("x-idempotency-key")
        .expect("the receipt digest must travel as an idempotency key")
        .to_str()
        .unwrap()
        .to_string();
    assert_eq!(idempotency, receipt.digest);

    assert!(
        requests[0].headers.get("authorization").is_some(),
        "the configured bearer token must be sent"
    );
}

#[tokio::test]
async fn without_a_token_no_authorization_header_is_sent() {
    let server = MockServer::start().await;
    let receipt = signed_off_receipt();

    Mock::given(method("POST"))
        .and(path("/api/v1/verification-receipts"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "accepted": false,
            "error": "receipt for action already superseded",
        })))
        .mount(&server)
        .await;

    let anonymous = ControlCenterClient::new(
        ControlCenterClientConfig::new(server.uri().as_str()).with_timeout(2_000),
    )
    .unwrap();

    let response = anonymous
        .submit_receipt(&receipt)
        .await
        .expect("a rejection is still a successful exchange");

    assert!(!response.accepted);
    assert_eq!(
        response.error.as_deref(),
        Some("receipt for action already superseded")
    );

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].headers.get("authorization").is_none());
}

#[tokio::test]
async fn unauthorized_response_maps_to_unauthorized_error() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/v1/verification-receipts"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;

    let error = client(&server.uri())
        .submit_receipt(&signed_off_receipt())
        .await
        .expect_err("401 must surface as Unauthorized");

    assert!(
        matches!(error, ControlCenterClientError::Unauthorized),
        "unexpected error: {error}"
    );
    assert!(error.to_string().contains("Unauthorized"));
}

#[tokio::test]
async fn forbidden_response_maps_to_forbidden_error() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/v1/verification-receipts"))
        .respond_with(ResponseTemplate::new(403))
        .mount(&server)
        .await;

    let error = client(&server.uri())
        .submit_receipt(&signed_off_receipt())
        .await
        .expect_err("403 must surface as Forbidden");

    assert!(matches!(error, ControlCenterClientError::Forbidden));
}

#[tokio::test]
async fn server_side_error_with_valid_json_is_reported_not_raised() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/v1/verification-receipts"))
        .respond_with(ResponseTemplate::new(500).set_body_json(serde_json::json!({
            "accepted": false,
            "error": "receipt store unavailable",
        })))
        .mount(&server)
        .await;

    let response = client(&server.uri())
        .submit_receipt(&signed_off_receipt())
        .await
        .expect("a 5xx with a parseable body is still a valid response");

    assert!(!response.accepted);
    assert_eq!(response.error.as_deref(), Some("receipt store unavailable"));
}

#[tokio::test]
async fn non_json_response_body_is_a_parse_error() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/v1/verification-receipts"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("<html>gateway timeout</html>")
                .insert_header("Content-Type", "text/html"),
        )
        .mount(&server)
        .await;

    let error = client(&server.uri())
        .submit_receipt(&signed_off_receipt())
        .await
        .expect_err("a non-JSON body cannot be decoded");

    match &error {
        ControlCenterClientError::ParseError(message) => {
            assert!(
                message.contains("failed to parse response"),
                "unexpected message: {message}"
            );
        }
        other => panic!("expected ParseError, got {other}"),
    }
}

#[tokio::test]
async fn timeout_is_reported_as_timeout_and_as_unknown() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/v1/verification-receipts"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({ "accepted": true }))
                .set_delay(Duration::from_secs(5)),
        )
        .mount(&server)
        .await;

    // Client gives up after 150ms; the server answers after 5s.
    let impatient = ControlCenterClient::new(
        ControlCenterClientConfig::new(server.uri().as_str())
            .with_bearer_token("cc-token-417")
            .with_timeout(150),
    )
    .unwrap();

    let error = impatient
        .submit_receipt(&signed_off_receipt())
        .await
        .expect_err("an expired request must be reported");

    match error {
        ControlCenterClientError::Timeout(ms) => assert_eq!(ms, 150),
        other => panic!("expected Timeout, got {other}"),
    }

    let unknown = impatient
        .submit_receipt_with_unknown_on_timeout(&signed_off_receipt())
        .await
        .expect("UNKNOWN is a response, not an error");

    assert!(!unknown.accepted);
    assert_eq!(unknown.correlation_id, None);
    assert_eq!(
        unknown.error.as_deref(),
        Some("Timeout - submission status unknown")
    );
}

#[tokio::test]
async fn unknown_on_timeout_wrapper_passes_other_outcomes_through() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/v1/verification-receipts"))
        .respond_with(accepted("corr-5150"))
        .mount(&server)
        .await;

    let response = client(&server.uri())
        .submit_receipt_with_unknown_on_timeout(&signed_off_receipt())
        .await
        .expect("an accepted receipt passes through unchanged");

    assert!(response.accepted);
    assert_eq!(response.correlation_id.as_deref(), Some("corr-5150"));
}

#[tokio::test]
async fn unknown_on_timeout_wrapper_propagates_auth_failures() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/v1/verification-receipts"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;

    let error = client(&server.uri())
        .submit_receipt_with_unknown_on_timeout(&signed_off_receipt())
        .await
        .expect_err("auth failures must not be masked as UNKNOWN");

    assert!(matches!(error, ControlCenterClientError::Unauthorized));
}

#[tokio::test]
async fn unreachable_collector_is_a_transport_error() {
    // Claim a port, then release it so the connection is refused.
    let abandoned = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = abandoned.local_addr().unwrap().port();
    drop(abandoned);

    let error = client(&format!("http://127.0.0.1:{port}"))
        .submit_receipt(&signed_off_receipt())
        .await
        .expect_err("a refused connection must surface as a transport error");

    assert!(error.to_string().contains("HTTP request failed"));
    match &error {
        ControlCenterClientError::HttpError(inner) => {
            assert!(!inner.is_timeout(), "connection refusal is not a timeout");
        }
        other => panic!("expected HttpError, got {other}"),
    }
}

#[tokio::test]
async fn wildcard_redaction_scrubs_every_field_of_the_receipt() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/v1/verification-receipts"))
        .and(body_partial_json(serde_json::json!({
            "version": "[REDACTED]",
            "result": "[REDACTED]",
            "digest": "[REDACTED]",
        })))
        .respond_with(accepted("corr-scrubbed"))
        .mount(&server)
        .await;

    let scrubber = ControlCenterClient::new(
        ControlCenterClientConfig::new(server.uri().as_str())
            .with_bearer_token("cc-token-417")
            .with_redact_field("*")
            .with_timeout(2_000),
    )
    .unwrap();

    scrubber
        .submit_receipt(&signed_off_receipt())
        .await
        .expect("a scrubbed receipt is still submittable");

    let requests = server.received_requests().await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert_eq!(body["version"], "[REDACTED]");
    assert_eq!(body["result"], "[REDACTED]");
    assert_eq!(body["digest"], "[REDACTED]");
    assert_eq!(body["observations"], "[REDACTED]");
}

#[tokio::test]
async fn truncated_response_body_is_a_parse_error() {
    // A peer that promises a JSON body and then hangs up mid-stream.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let peer = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut scratch = [0u8; 2048];
        let _ = stream.read(&mut scratch);
        let _ = stream.write_all(
            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 4096\r\n\r\n",
        );
        let _ = stream.write_all(b"{\"accepted\":");
        // Deliberately drops the socket before the promised 4096 bytes arrive.
    });

    let error = client(&format!("http://{addr}"))
        .submit_receipt(&signed_off_receipt())
        .await
        .expect_err("a truncated body cannot be decoded");

    peer.join().unwrap();

    match &error {
        ControlCenterClientError::ParseError(message) => {
            assert!(
                message.contains("failed to read response"),
                "unexpected message: {message}"
            );
        }
        other => panic!("expected ParseError, got {other}"),
    }
}
