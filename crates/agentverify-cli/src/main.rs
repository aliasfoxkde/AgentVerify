//! AgentVerify CLI

use agentverify_contract::load_file;
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::process::ExitCode;

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
    /// Run verification (dry-run)
    Verify {
        /// Contract file
        contract: String,
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
    if let Err(e) = run() {
        eprintln!("Error: {}", e);
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}

fn run() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init { path } => {
            println!("Initializing AgentVerify at {:?}...", path);
        }
        Commands::Contract { command } => match command {
            ContractCommands::Validate { file, json } => {
                validate_contract_cmd(&file, json)?;
            }
        },
        Commands::Verify { contract } => {
            println!("Verifying contract (dry-run): {}", contract);
        }
        Commands::Serve { port } => {
            println!("Starting server on port {}...", port);
        }
    }

    Ok(())
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
