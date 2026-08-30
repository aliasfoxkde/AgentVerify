//! CLI exit-code tests
//!
//! Verifies that the CLI returns correct exit codes for each verification outcome.

use std::process::Command;

/// Test that contract validate returns 1 for file not found.
#[test]
fn contract_validate_file_not_found() {
    let output = Command::new(env!("CARGO_BIN_EXE_agentverify"))
        .args(["contract", "validate", "/nonexistent/path/contract.json"])
        .output()
        .expect("Failed to execute CLI");

    assert_eq!(
        output.status.code(),
        Some(1),
        "File not found should return exit code 1"
    );
}

/// Test that verify command returns 1 on error (missing file).
#[test]
fn verify_command_error_missing_contract() {
    let output = Command::new(env!("CARGO_BIN_EXE_agentverify"))
        .args(["verify", "--contract", "/nonexistent/path/contract.json"])
        .output()
        .expect("Failed to execute CLI");

    // Error cases return exit code 1
    assert_eq!(
        output.status.code(),
        Some(1),
        "Missing contract file should return exit code 1"
    );
}

/// Test init command returns 0.
#[test]
fn init_command_success() {
    let temp_dir = std::env::temp_dir();
    let init_path = temp_dir.join("test_init_dir_cli");

    let output = Command::new(env!("CARGO_BIN_EXE_agentverify"))
        .args(["init", "--path", init_path.to_str().unwrap()])
        .output()
        .expect("Failed to execute CLI");

    assert_eq!(
        output.status.code(),
        Some(0),
        "init should return exit code 0"
    );
}

/// Test serve command can start and respond to health check.
///
/// Note: serve is a long-running server, so we poll the port with retries
/// instead of relying on a fixed sleep, which is flaky on slow runners.
#[test]
fn serve_command_starts_and_responds() {
    use std::net::{SocketAddr, TcpStream};
    use std::time::{Duration, Instant};

    let addr: SocketAddr = "127.0.0.1:12346".parse().expect("valid socket address");

    let mut child = Command::new(env!("CARGO_BIN_EXE_agentverify"))
        .args(["serve", "--port", "12346"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("Failed to spawn serve command");

    // Poll until the server accepts connections (up to 10s).
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut connected = false;
    while Instant::now() < deadline {
        // If the server process died, fail fast with its status.
        if let Some(status) = child.try_wait().expect("poll serve process") {
            panic!("serve exited early with status: {status}");
        }
        if TcpStream::connect(addr).is_ok() {
            connected = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    // Clean up: kill the server
    let _ = child.kill();
    let _ = child.wait();

    assert!(connected, "serve should start and listen on port");
}

/// Test help command returns 0.
#[test]
fn help_command_success() {
    let output = Command::new(env!("CARGO_BIN_EXE_agentverify"))
        .args(["--help"])
        .output()
        .expect("Failed to execute CLI");

    assert_eq!(
        output.status.code(),
        Some(0),
        "--help should return exit code 0"
    );
}

/// Test contract validate shows usage for invalid args.
#[test]
fn contract_validate_invalid_args() {
    let output = Command::new(env!("CARGO_BIN_EXE_agentverify"))
        .args(["contract", "validate", "--json", "--invalid-flag"])
        .output()
        .expect("Failed to execute CLI");

    assert_eq!(
        output.status.code(),
        Some(2),
        "Invalid flags should return exit code 2"
    );
}

/// Test verify command --help shows JSON output flag.
#[test]
fn verify_help_shows_json_flag() {
    let output = Command::new(env!("CARGO_BIN_EXE_agentverify"))
        .args(["verify", "--help"])
        .output()
        .expect("Failed to execute CLI");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--json"), "Help should show --json flag");
}
