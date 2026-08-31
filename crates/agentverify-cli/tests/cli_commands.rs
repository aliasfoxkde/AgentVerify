#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! CLI command tests
//!
//! Exercises the full `contract validate`, `verify`, and `serve` command
//! paths, including JSON output modes and the graceful-shutdown behaviour of
//! the HTTP gateway. The `verify` tests run against a minimal in-process HTTP
//! server standing in for the REST observer.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const VALID_CONTRACT: &str = r#"{
    "action_name": "create_order",
    "postconditions": [
        {
            "predicate": {"type": "exists", "path": "order.id"},
            "description": "Order was created"
        }
    ]
}"#;

/// Write a contract file to a temp directory and return its path.
fn contract_file(body: &str) -> std::path::PathBuf {
    let dir = tempfile::tempdir().expect("create temp dir");
    // Leak the tempdir so the file outlives this helper; the OS cleans
    // /tmp on reboot and the tests are short-lived.
    let path = dir.keep().join("contract.json");
    std::fs::write(&path, body).expect("write contract file");
    path
}

// ---------------------------------------------------------------------------
// Minimal REST observer stand-in
// ---------------------------------------------------------------------------

/// Serve the given status line and body for every request until the test
/// process exits. Returns the port the server listens on.
fn spawn_http_server(status_line: String, body: String) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind test observer server");
    let port = listener.local_addr().expect("local addr").port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            // Drain the request headers; the response is static.
            let mut buf = [0u8; 4096];
            loop {
                match stream.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if buf[..n].windows(4).any(|w| w == b"\r\n\r\n") {
                            break;
                        }
                    }
                }
            }
            let response = format!(
                "{status_line}\r\nContent-Type: application/json\r\n\
                 Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        }
    });
    port
}

/// Serve a 200 JSON state document for every request.
fn spawn_state_server(body: String) -> u16 {
    spawn_http_server("HTTP/1.1 200 OK".to_string(), body)
}

/// Issue an HTTP GET and return the status line plus body.
fn http_get(port: u16, path: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect to serve");
    let request = format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .expect("write HTTP request");
    let mut response = String::new();
    let _ = stream.read_to_string(&mut response);
    response
}

/// Wait until the port accepts connections (or fail after 10 s).
fn wait_for_port(port: u16) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("port {port} never started accepting connections");
}

/// Wait for the child to exit and return its status (or fail after 15 s).
fn wait_for_exit(child: &mut std::process::Child) -> std::process::ExitStatus {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Some(status) = child.try_wait().expect("poll child process") {
            return status;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("child process did not exit within 15 s");
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn spawn_serve(port: u16) -> std::process::Child {
    Command::new(env!("CARGO_BIN_EXE_agentverify"))
        .args(["serve", "--port", &port.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn serve command")
}

// ---------------------------------------------------------------------------
// contract validate
// ---------------------------------------------------------------------------

#[test]
fn contract_validate_success_prints_details() {
    let path = contract_file(VALID_CONTRACT);
    let output = Command::new(env!("CARGO_BIN_EXE_agentverify"))
        .args(["contract", "validate", path.to_str().unwrap()])
        .output()
        .expect("run contract validate");

    assert_eq!(
        output.status.code(),
        Some(0),
        "valid contract should exit 0"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Contract is valid"), "stdout: {stdout}");
    assert!(stdout.contains("Action: create_order"), "stdout: {stdout}");
    assert!(stdout.contains("ID:"), "stdout: {stdout}");
}

#[test]
fn contract_validate_success_json_output() {
    let path = contract_file(VALID_CONTRACT);
    let output = Command::new(env!("CARGO_BIN_EXE_agentverify"))
        .args(["contract", "validate", "--json", path.to_str().unwrap()])
        .output()
        .expect("run contract validate");

    assert_eq!(
        output.status.code(),
        Some(0),
        "valid contract should exit 0"
    );
    let parsed: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("parse validate JSON output");
    assert_eq!(parsed["valid"], serde_json::Value::Bool(true));
    assert!(parsed["errors"]
        .as_array()
        .expect("errors array")
        .is_empty());
    assert_eq!(parsed["action_name"], "create_order");
    assert!(parsed["contract_id"].is_string());
}

#[test]
fn contract_validate_semantic_failure_json_output() {
    // Parses cleanly but declares an incompatible schema version, so the
    // post-load validation step fails.
    let path = contract_file(
        r#"{
        "schema_version": "9.9",
        "action_name": "create_order",
        "postconditions": [
            {"predicate": {"type": "exists", "path": "order.id"}, "description": "ok"}
        ]
    }"#,
    );
    let output = Command::new(env!("CARGO_BIN_EXE_agentverify"))
        .args(["contract", "validate", "--json", path.to_str().unwrap()])
        .output()
        .expect("run contract validate");

    assert_eq!(
        output.status.code(),
        Some(2),
        "semantically invalid contract should exit 2"
    );
    let parsed: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("parse validate JSON output");
    assert_eq!(parsed["valid"], serde_json::Value::Bool(false));
    let errors = parsed["errors"].as_array().expect("errors array");
    assert!(!errors.is_empty(), "expected a validation error");
}

#[test]
fn contract_validate_parse_error_prints_message() {
    let path = contract_file("{ not json");
    let output = Command::new(env!("CARGO_BIN_EXE_agentverify"))
        .args(["contract", "validate", path.to_str().unwrap()])
        .output()
        .expect("run contract validate");

    assert_eq!(output.status.code(), Some(2), "parse error should exit 2");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Contract is invalid"), "stdout: {stdout}");
    assert!(stdout.contains("Failed to parse JSON"), "stdout: {stdout}");
}

// ---------------------------------------------------------------------------
// verify
// ---------------------------------------------------------------------------

#[test]
fn verify_reports_verified_against_live_observer() {
    let path = contract_file(VALID_CONTRACT);
    let port = spawn_state_server(r#"{"order": {"id": "ord-1"}}"#.to_string());

    let output = Command::new(env!("CARGO_BIN_EXE_agentverify"))
        .args(["verify", "--contract", path.to_str().unwrap(), "--json"])
        // The environment override must win over the CLI default.
        .env(
            "AGENTVERIFY_OBSERVER_URL",
            format!("http://127.0.0.1:{port}"),
        )
        .output()
        .expect("run verify");

    assert_eq!(output.status.code(), Some(0), "Verified should exit 0");
    let parsed: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("parse verify JSON output");
    assert_eq!(parsed["verification_result"], "verified");
    assert!(parsed["receipt_id"].is_string());
    assert!(parsed["contract_id"].is_string());
    assert!(parsed["action_id"].is_string());
    assert!(parsed["attempts"].is_number());
}

#[test]
fn verify_reports_failed_when_state_is_missing() {
    let path = contract_file(VALID_CONTRACT);
    let port = spawn_state_server("{}".to_string());

    let output = Command::new(env!("CARGO_BIN_EXE_agentverify"))
        .args(["verify", "--contract", path.to_str().unwrap(), "--json"])
        .env(
            "AGENTVERIFY_OBSERVER_URL",
            format!("http://127.0.0.1:{port}"),
        )
        .output()
        .expect("run verify");

    assert_eq!(output.status.code(), Some(2), "Failed should exit 2");
    let parsed: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("parse verify JSON output");
    assert_eq!(parsed["verification_result"], "failed");
}

#[test]
fn verify_human_output_reports_each_outcome() {
    // Verified in human-readable mode.
    let path = contract_file(VALID_CONTRACT);
    let port = spawn_state_server(r#"{"order": {"id": "ord-1"}}"#.to_string());
    let output = Command::new(env!("CARGO_BIN_EXE_agentverify"))
        .args(["verify", "--contract", path.to_str().unwrap()])
        .env(
            "AGENTVERIFY_OBSERVER_URL",
            format!("http://127.0.0.1:{port}"),
        )
        .output()
        .expect("run verify");
    assert_eq!(output.status.code(), Some(0), "Verified should exit 0");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Verification result: verified"),
        "stdout: {stdout}"
    );
    assert!(stdout.contains("Receipt ID:"), "stdout: {stdout}");
    assert!(stdout.contains("Attempts:"), "stdout: {stdout}");

    // Failed in human-readable mode.
    let path = contract_file(VALID_CONTRACT);
    let port = spawn_state_server("{}".to_string());
    let output = Command::new(env!("CARGO_BIN_EXE_agentverify"))
        .args(["verify", "--contract", path.to_str().unwrap()])
        .env(
            "AGENTVERIFY_OBSERVER_URL",
            format!("http://127.0.0.1:{port}"),
        )
        .output()
        .expect("run verify");
    assert_eq!(output.status.code(), Some(2), "Failed should exit 2");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Verification result: failed"),
        "stdout: {stdout}"
    );
}

#[test]
fn verify_reports_unknown_when_the_observer_errors() {
    let path = contract_file(VALID_CONTRACT);
    let port = spawn_http_server(
        "HTTP/1.1 500 Internal Server Error".to_string(),
        "{}".to_string(),
    );

    let output = Command::new(env!("CARGO_BIN_EXE_agentverify"))
        .args(["verify", "--contract", path.to_str().unwrap()])
        .env(
            "AGENTVERIFY_OBSERVER_URL",
            format!("http://127.0.0.1:{port}"),
        )
        .output()
        .expect("run verify");

    assert_eq!(output.status.code(), Some(3), "Unknown should exit 3");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Verification result: unknown"),
        "stdout: {stdout}"
    );
}

#[test]
fn verify_rejects_the_action_without_dispatch_when_preconditions_fail() {
    let path = contract_file(
        r#"{
        "action_name": "create_order",
        "preconditions": [
            {"predicate": {"type": "exists", "path": "input.request"}, "description": "request present"}
        ],
        "postconditions": [
            {"predicate": {"type": "exists", "path": "order.id"}, "description": "Order was created"}
        ]
    }"#,
    );
    // The observed state has no `input.request`, so the executor rejects the
    // action before dispatch — a terminal Failed, never an error.
    let port = spawn_state_server("{}".to_string());

    let output = Command::new(env!("CARGO_BIN_EXE_agentverify"))
        .args(["verify", "--contract", path.to_str().unwrap()])
        .env(
            "AGENTVERIFY_OBSERVER_URL",
            format!("http://127.0.0.1:{port}"),
        )
        .output()
        .expect("run verify");

    assert_eq!(
        output.status.code(),
        Some(2),
        "a precondition rejection is a terminal Failed"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Verification result: failed"),
        "stdout: {stdout}"
    );
}

#[test]
fn verify_reports_an_error_when_a_postcondition_cannot_be_evaluated() {
    // An invalid regex makes the postcondition unevaluable, which the
    // executor reports as an error rather than a verdict.
    let path = contract_file(
        r#"{
        "action_name": "create_order",
        "postconditions": [
            {"predicate": {"type": "matches", "path": "order.status", "pattern": "*invalid("}, "description": "status sane"}
        ]
    }"#,
    );
    let port = spawn_state_server(r#"{"order": {"status": "open"}}"#.to_string());

    let output = Command::new(env!("CARGO_BIN_EXE_agentverify"))
        .args(["verify", "--contract", path.to_str().unwrap()])
        .env(
            "AGENTVERIFY_OBSERVER_URL",
            format!("http://127.0.0.1:{port}"),
        )
        .output()
        .expect("run verify");

    assert_eq!(
        output.status.code(),
        Some(1),
        "unevaluable postconditions should exit 1"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Verification error"), "stderr: {stderr}");
}

#[test]
fn verify_rejects_invalid_args_json() {
    let path = contract_file(VALID_CONTRACT);
    let output = Command::new(env!("CARGO_BIN_EXE_agentverify"))
        .args([
            "verify",
            "--contract",
            path.to_str().unwrap(),
            "--args",
            "not json",
        ])
        .output()
        .expect("run verify");

    assert_eq!(output.status.code(), Some(1), "bad args should exit 1");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Invalid JSON in args"), "stderr: {stderr}");
}

#[test]
fn verify_rejects_non_http_observer_scheme() {
    let path = contract_file(VALID_CONTRACT);
    let output = Command::new(env!("CARGO_BIN_EXE_agentverify"))
        .args([
            "verify",
            "--contract",
            path.to_str().unwrap(),
            "--observer-url",
            "ftp://example.com/state",
        ])
        .output()
        .expect("run verify");

    assert_eq!(output.status.code(), Some(1), "bad scheme should exit 1");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("must be http or https"), "stderr: {stderr}");
}

// ---------------------------------------------------------------------------
// serve
// ---------------------------------------------------------------------------

#[test]
fn serve_shuts_down_gracefully_via_endpoint() {
    let port = 12441;
    let mut child = spawn_serve(port);
    wait_for_port(port);

    let health = http_get(port, "/health");
    assert!(health.contains("200"), "health response: {health}");
    assert!(health.ends_with("OK"), "health body: {health}");

    let shutdown = http_get(port, "/shutdown");
    assert!(shutdown.contains("200"), "shutdown response: {shutdown}");

    let status = wait_for_exit(&mut child);
    assert!(status.success(), "graceful shutdown should exit 0");
}

#[test]
fn serve_shuts_down_on_sigterm() {
    let port = 12443;
    let mut child = spawn_serve(port);
    wait_for_port(port);

    Command::new("kill")
        .arg("-TERM")
        .arg(child.id().to_string())
        .status()
        .expect("send SIGTERM");

    let status = wait_for_exit(&mut child);
    assert!(status.success(), "SIGTERM should trigger a clean exit");
}

#[test]
fn serve_reports_bind_failure() {
    // Hold the port in this process so the CLI's bind must fail.
    let holder = TcpListener::bind("127.0.0.1:12442").expect("hold port");

    let output = Command::new(env!("CARGO_BIN_EXE_agentverify"))
        .args(["serve", "--port", "12442"])
        .output()
        .expect("run serve");

    assert_eq!(output.status.code(), Some(1), "bind failure should exit 1");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Failed to bind to port 12442"),
        "stderr: {stderr}"
    );

    drop(holder);
}

#[test]
fn serve_shuts_down_on_sigint() {
    let port = 12444;
    let mut child = spawn_serve(port);
    wait_for_port(port);

    Command::new("kill")
        .arg("-INT")
        .arg(child.id().to_string())
        .status()
        .expect("send SIGINT");

    let status = wait_for_exit(&mut child);
    assert!(status.success(), "SIGINT should trigger a clean exit");
}

#[test]
fn contract_validate_missing_file_json_output() {
    let output = Command::new(env!("CARGO_BIN_EXE_agentverify"))
        .args([
            "contract",
            "validate",
            "--json",
            "/nonexistent/path/contract.json",
        ])
        .output()
        .expect("run contract validate");

    assert_eq!(output.status.code(), Some(1), "missing file should exit 1");
    let parsed: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("parse JSON error output");
    assert_eq!(parsed["valid"], serde_json::Value::Bool(false));
    assert_eq!(parsed["contract_id"], serde_json::Value::Null);
    assert_eq!(parsed["action_name"], serde_json::Value::Null);
    assert!(parsed["errors"][0]
        .as_str()
        .expect("error message")
        .contains("Failed to read file"));
}

#[test]
fn contract_validate_semantic_failure_prints_messages() {
    let path = contract_file(
        r#"{
        "schema_version": "9.9",
        "action_name": "create_order",
        "postconditions": [
            {"predicate": {"type": "exists", "path": "order.id"}, "description": "ok"}
        ]
    }"#,
    );
    let output = Command::new(env!("CARGO_BIN_EXE_agentverify"))
        .args(["contract", "validate", path.to_str().unwrap()])
        .output()
        .expect("run contract validate");

    assert_eq!(output.status.code(), Some(2));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Contract is invalid"), "stdout: {stdout}");
    assert!(
        stdout.contains("schema version"),
        "validation error should be listed: {stdout}"
    );
}

#[test]
fn verify_rejects_an_unparseable_observer_url() {
    let path = contract_file(VALID_CONTRACT);
    let output = Command::new(env!("CARGO_BIN_EXE_agentverify"))
        .args([
            "verify",
            "--contract",
            path.to_str().unwrap(),
            "--observer-url",
            "not a url at all",
        ])
        .output()
        .expect("run verify");

    assert_eq!(output.status.code(), Some(1), "bad URL should exit 1");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("must be a valid HTTP/HTTPS URL"),
        "stderr: {stderr}"
    );
}

#[test]
fn verify_reports_errors_as_json_when_requested() {
    // An invalid regex makes the postcondition unevaluable; in JSON mode the
    // error is reported as a JSON object on stderr.
    let path = contract_file(
        r#"{
        "action_name": "create_order",
        "postconditions": [
            {"predicate": {"type": "matches", "path": "order.status", "pattern": "*invalid("}, "description": "status sane"}
        ]
    }"#,
    );
    let port = spawn_state_server(r#"{"order": {"status": "open"}}"#.to_string());

    let output = Command::new(env!("CARGO_BIN_EXE_agentverify"))
        .args(["verify", "--contract", path.to_str().unwrap(), "--json"])
        .env(
            "AGENTVERIFY_OBSERVER_URL",
            format!("http://127.0.0.1:{port}"),
        )
        .output()
        .expect("run verify");

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    let parsed: serde_json::Value = serde_json::from_str(stderr.trim()).expect("JSON error output");
    assert!(
        parsed["error"]
            .as_str()
            .expect("error field")
            .contains("Verification failed"),
        "stderr: {stderr}"
    );
}
