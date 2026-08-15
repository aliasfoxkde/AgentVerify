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

/// Test serve command returns 0.
#[test]
fn serve_command_success() {
    let output = Command::new(env!("CARGO_BIN_EXE_agentverify"))
        .args(["serve", "--port", "12345"])
        .output()
        .expect("Failed to execute CLI");

    assert_eq!(
        output.status.code(),
        Some(0),
        "serve should return exit code 0"
    );
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
