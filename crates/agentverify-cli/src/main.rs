//! AgentVerify CLI

use agentverify_contract::load_file;
use agentverify_core::Action;
use agentverify_http::{RestObserver, RestObserverConfig};
use agentverify_runtime::{Executor, SimulatedActionExecutor};
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::process::ExitCode;
use std::sync::Arc;

#[derive(Parser)]
#[command(name = "agentverify")]
#[command(about = "Outcome verification for AI agents")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize AgentVerify in a project
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

        /// Observer URL (defaults to http://localhost:8080)
        #[arg(short, long, default_value = "http://localhost:8080")]
        observer_url: String,

        /// Maximum retry attempts
        #[arg(short, long, default_value = "3")]
        max_retries: u32,
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
            eprintln!("Error: {}", e);
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<ExitCode> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init { path } => {
            println!("Initializing AgentVerify at {:?}...", path);
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
        } => verify_contract_cmd(&contract, &args, &observer_url, max_retries),
        Commands::Serve { port } => {
            println!("Starting server on port {}...", port);
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn validate_contract_cmd(file: &str, json: bool) -> Result<ExitCode> {
    let path = std::path::Path::new(file);

    let contract =
        load_file(path).with_context(|| format!("Failed to load contract from {}", file))?;

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
        println!("{}", serde_json::to_string_pretty(&output).unwrap());
    } else {
        // Human-readable output
        if output.valid {
            println!("✓ Contract is valid");
            println!("  ID: {}", contract.id);
            println!("  Action: {}", contract.action_name);
        } else {
            println!("✗ Contract is invalid:");
            for error in &errors {
                println!("  - {}", error);
            }
        }
    }

    if output.valid {
        Ok(ExitCode::SUCCESS)
    } else {
        Ok(ExitCode::from(2))
    }
}

fn verify_contract_cmd(
    contract_path: &str,
    args_json: &str,
    observer_url: &str,
    _max_retries: u32,
) -> Result<ExitCode> {
    // Load contract
    let path = std::path::Path::new(contract_path);
    let contract = load_file(path)
        .with_context(|| format!("Failed to load contract from {}", contract_path))?;

    // Parse action arguments
    let args: serde_json::Value = serde_json::from_str(args_json)
        .with_context(|| format!("Invalid JSON in args: {}", args_json))?;

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
        .map_err(|e| anyhow::anyhow!("Failed to create observer: {}", e))?;
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
            println!("Verification result: {}", verification_result);
            println!("Receipt ID: {}", receipt.id);
            println!("Attempts: {}", receipt.attempts);

            match verification_result {
                agentverify_core::VerificationResult::Verified => Ok(ExitCode::SUCCESS),
                agentverify_core::VerificationResult::Duplicate => Ok(ExitCode::SUCCESS),
                agentverify_core::VerificationResult::Failed
                | agentverify_core::VerificationResult::Partial => Ok(ExitCode::from(2)),
                agentverify_core::VerificationResult::Unknown => Ok(ExitCode::from(3)),
            }
        }
        Err(e) => {
            eprintln!("Verification error: {}", e);
            Ok(ExitCode::from(1))
        }
    }
}
