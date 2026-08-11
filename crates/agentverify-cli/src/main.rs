//! AgentVerify CLI

use anyhow::Result;
use clap::{Parser, Subcommand};

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
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init { path } => {
            println!("Initializing AgentVerify at {:?}...", path);
        }
        Commands::Contract { command } => match command {
            ContractCommands::Validate { file } => {
                println!("Validating contract: {}", file);
            }
        },
        Commands::Verify { contract } => {
            println!("Verifying contract: {}", contract);
        }
        Commands::Serve { port } => {
            println!("Starting server on port {}...", port);
        }
    }

    Ok(())
}
