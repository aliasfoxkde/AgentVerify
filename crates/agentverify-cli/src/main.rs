//! `AgentVerify` CLI

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
// A CLI's contract is its stdout/stderr output; routing those writes
// through `tracing` would change user-visible behavior.
#![allow(clippy::print_stdout, clippy::print_stderr)]
use agentverify_contract::{contract::ContractError, load_file};
use agentverify_core::Action;
use agentverify_http::{RestObserver, RestObserverConfig};
use agentverify_runtime::{Executor, SimulatedActionExecutor};
use anyhow::{Context, Result};
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use clap::{Parser, Subcommand};
use std::net::SocketAddr;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::oneshot;
use tower::ServiceBuilder;
use tower_http::trace::TraceLayer;
use tracing::info;
use url::Url;

/// Shared state for the HTTP server
#[derive(Clone)]
struct ServeState {
    shutdown_flag: Arc<AtomicBool>,
}

/// Health check endpoint
async fn health() -> Response {
    (StatusCode::OK, "OK").into_response()
}

/// Shutdown endpoint - triggers graceful shutdown
async fn shutdown(State(state): State<ServeState>) -> Response {
    state.shutdown_flag.store(true, Ordering::SeqCst);
    (StatusCode::OK, "Shutting down...").into_response()
}

#[derive(Parser)]
#[command(name = "agentverify")]
#[command(about = "Outcome verification for AI agents")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize `AgentVerify` in a project
    Init {
        /// Project directory
        #[arg(short, long)]
        path: Option<String>,
    },
    /// Manage verification contracts
    Contract {
        #[command(subcommand)]
        command: ContractCommands,
    },
    /// Run verification
    Verify {
        /// Contract file
        #[arg(short, long)]
        contract: String,

        /// Action arguments as JSON
        #[arg(short, long, default_value = "{}")]
        args: String,

        /// Observer URL (defaults to <http://localhost:8080>, or `AGENTVERIFY_OBSERVER_URL` env var)
        #[arg(short, long, default_value = "http://localhost:8080")]
        observer_url: String,

        /// Maximum retry attempts
        #[arg(short, long, default_value = "3")]
        max_retries: u32,

        /// Output in JSON format
        #[arg(short, long)]
        json: bool,
    },
    /// Start the HTTP gateway
    Serve {
        /// Listen port
        #[arg(short, long, default_value = "8080")]
        port: u16,
    },
}

#[derive(Subcommand)]
enum ContractCommands {
    /// Validate a contract file
    Validate {
        /// Contract file path
        file: String,

        /// Output JSON format
        #[arg(short, long)]
        json: bool,
    },
}

#[derive(serde::Serialize)]
struct ValidateOutput {
    valid: bool,
    errors: Vec<String>,
    contract_id: Option<String>,
    action_name: Option<String>,
}

fn main() -> ExitCode {
    match run() {
        Ok(exit_code) => exit_code,
        Err(e) => {
            eprintln!("Error: {e}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<ExitCode> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init { path } => {
            println!("Initializing AgentVerify at {path:?}...");
            Ok(ExitCode::SUCCESS)
        }
        Commands::Contract { command } => match command {
            ContractCommands::Validate { file, json } => validate_contract_cmd(&file, json),
        },
        Commands::Verify {
            contract,
            args,
            observer_url,
            max_retries,
            json,
        } => verify_contract_cmd(&contract, &args, &observer_url, max_retries, json),
        Commands::Serve { port } => {
            let rt = tokio::runtime::Runtime::new().context("Failed to create Tokio runtime")?;
            rt.block_on(serve(port))
        }
    }
}

/// Start the HTTP server with graceful shutdown support
async fn serve(port: u16) -> Result<ExitCode> {
    // Initialize tracing subscriber
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    let shutdown_flag = Arc::new(AtomicBool::new(false));
    let shutdown_flag_clone = shutdown_flag.clone();

    // Build the router
    let state = ServeState {
        shutdown_flag: shutdown_flag.clone(),
    };

    let app = Router::new()
        .route("/health", get(health))
        .route("/shutdown", get(shutdown))
        .layer(
            ServiceBuilder::new()
                .layer(TraceLayer::new_for_http())
                .into_inner(),
        )
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("Failed to bind to port {port}"))?;

    info!("AgentVerify server listening on {}", addr);

    // Spawn signal handler task
    let (shutdown_tx, mut shutdown_rx) = oneshot::channel::<()>();
    tokio::spawn(async move {
        // SIGTERM streams are unix-only; other platforms shut down via
        // Ctrl+C (SIGINT) and the internal shutdown channel alone.
        #[cfg(unix)]
        {
            // Setup SIGTERM signal stream
            let mut sigterm =
                match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                    Ok(stream) => stream,
                    Err(error) => {
                        tracing::warn!("Failed to create SIGTERM signal handler: {error}");
                        return;
                    }
                };

            // Wait for shutdown signal (SIGINT or SIGTERM)
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {
                    info!("Received SIGINT (Ctrl+C)");
                }
                _ = sigterm.recv() => {
                    info!("Received SIGTERM");
                }
                _ = &mut shutdown_rx => {
                    info!("Received internal shutdown signal");
                }
            }
        }

        #[cfg(not(unix))]
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                info!("Received Ctrl+C");
            }
            _ = &mut shutdown_rx => {
                info!("Received internal shutdown signal");
            }
        }

        shutdown_flag_clone.store(true, Ordering::SeqCst);
    });

    // Start the server
    let server_handle = tokio::spawn(async move { axum::serve(listener, app).await });

    // Wait for either the server to stop or shutdown signal
    tokio::select! {
        outcome = server_handle => {
            match outcome {
                Ok(Ok(())) => info!("Server task completed"),
                Ok(Err(error)) => return Err(anyhow::anyhow!("Server error: {error}")),
                Err(error) => return Err(anyhow::anyhow!("Server task failed: {error}")),
            }
        }
        () = tokio::time::sleep(std::time::Duration::from_secs(u64::MAX)) => {
            // This branch should never be taken, but prevents the select from completing
        }
    }

    // If we get here due to signal, wait for graceful shutdown
    if shutdown_flag.load(Ordering::SeqCst) {
        info!("Initiating graceful shutdown...");
        // Give time for in-flight requests to complete
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        info!("Graceful shutdown complete");
    }

    // Send shutdown signal to signal handler
    let _ = shutdown_tx.send(());

    Ok(ExitCode::SUCCESS)
}

fn validate_contract_cmd(file: &str, json: bool) -> Result<ExitCode> {
    let path = std::path::Path::new(file);

    let contract = match load_file(path) {
        Ok(contract) => contract,
        Err(error) => {
            let output = ValidateOutput {
                valid: false,
                errors: vec![error.to_string()],
                contract_id: None,
                action_name: None,
            };
            if json {
                println!("{}", serde_json::to_string_pretty(&output)?);
            } else {
                println!("✗ Contract is invalid:");
                for message in &output.errors {
                    println!("  - {message}");
                }
            }
            let exit_code = if matches!(error, ContractError::IoError { .. }) {
                ExitCode::from(1)
            } else {
                ExitCode::from(2)
            };
            return Ok(exit_code);
        }
    };

    // Run validation
    let errors: Vec<String> = match contract.validate() {
        Ok(()) => Vec::new(),
        Err(e) => vec![e.to_string()],
    };

    let output = ValidateOutput {
        valid: errors.is_empty(),
        errors: errors.clone(),
        contract_id: Some(contract.id.to_string()),
        action_name: Some(contract.action_name.clone()),
    };

    if json {
        // Machine-readable JSON output
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        // Human-readable output
        if output.valid {
            println!("✓ Contract is valid");
            println!("  ID: {}", contract.id);
            println!("  Action: {}", contract.action_name);
        } else {
            println!("✗ Contract is invalid:");
            for error in &errors {
                println!("  - {error}");
            }
        }
    }

    if output.valid {
        Ok(ExitCode::SUCCESS)
    } else {
        Ok(ExitCode::from(2))
    }
}

/// Machine-readable output for verify command
#[derive(serde::Serialize)]
struct VerifyOutput {
    verification_result: String,
    receipt_id: String,
    attempts: u32,
    contract_id: String,
    action_id: String,
}

fn verify_contract_cmd(
    contract_path: &str,
    args_json: &str,
    observer_url: &str,
    _max_retries: u32,
    json_output: bool,
) -> Result<ExitCode> {
    // Use environment variable if set, otherwise use provided URL
    let observer_url =
        std::env::var("AGENTVERIFY_OBSERVER_URL").unwrap_or_else(|_| observer_url.to_string());

    // Validate observer URL
    let observer_base_url = Url::parse(&observer_url).with_context(|| {
        format!("Invalid observer URL '{observer_url}': must be a valid HTTP/HTTPS URL")
    })?;
    if !matches!(observer_base_url.scheme(), "http" | "https") {
        anyhow::bail!(
            "Invalid observer URL scheme '{}': must be http or https",
            observer_base_url.scheme()
        );
    }

    // Load contract
    let path = std::path::Path::new(contract_path);
    let contract =
        load_file(path).with_context(|| format!("Failed to load contract from {contract_path}"))?;

    // Parse action arguments
    let args: serde_json::Value = serde_json::from_str(args_json)
        .with_context(|| format!("Invalid JSON in args: {args_json}"))?;

    // Create action with idempotency key based on contract and args
    let idempotency_key = format!("{}-{}", contract.id, args_json);
    let action = Action::with_idempotency(
        &contract.action_name,
        args,
        agentverify_core::IdempotencyKey::new(idempotency_key),
    );

    // Setup REST observer
    let observer_config = RestObserverConfig::new(observer_url);
    let observer = RestObserver::new(observer_config)
        .map_err(|e| anyhow::anyhow!("Failed to create observer: {e}"))?;
    let observer: Option<Arc<dyn agentverify_runtime::Observer>> = Some(Arc::new(observer));

    // Setup executor with simulated action executor
    let action_executor: Arc<dyn agentverify_runtime::ActionExecutor> =
        Arc::new(SimulatedActionExecutor::new());
    let executor = Executor::new();

    // Execute verification using execute_with_executor (real executor path)
    let rt = tokio::runtime::Runtime::new().context("Failed to create Tokio runtime")?;

    let result = rt.block_on(async {
        executor
            .execute_with_executor(action.clone(), contract.clone(), action_executor, observer)
            .await
    });

    match result {
        Ok((verification_result, receipt)) => {
            if json_output {
                let output = VerifyOutput {
                    verification_result: verification_result.to_string(),
                    receipt_id: receipt.id.to_string(),
                    attempts: receipt.attempts,
                    contract_id: contract.id.to_string(),
                    action_id: action.id.to_string(),
                };
                println!("{}", serde_json::to_string_pretty(&output)?);
            } else {
                println!("Verification result: {verification_result}");
                println!("Receipt ID: {}", receipt.id);
                println!("Attempts: {}", receipt.attempts);
            }

            match verification_result {
                agentverify_core::VerificationResult::Verified
                | agentverify_core::VerificationResult::Duplicate => Ok(ExitCode::SUCCESS),
                agentverify_core::VerificationResult::Failed
                | agentverify_core::VerificationResult::Partial => Ok(ExitCode::from(2)),
                agentverify_core::VerificationResult::Unknown => Ok(ExitCode::from(3)),
            }
        }
        Err(e) => {
            if json_output {
                let output = serde_json::json!({
                    "error": e.to_string(),
                });
                eprintln!("{}", serde_json::to_string_pretty(&output)?);
            } else {
                eprintln!("Verification error: {e}");
            }
            Ok(ExitCode::from(1))
        }
    }
}
