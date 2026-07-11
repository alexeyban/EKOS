use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "ekos",
    about = "Enterprise Knowledge Operating System",
    version,
    propagate_version = true
)]
struct Cli {
    /// Path to ekos.toml (default: ./ekos.toml)
    #[arg(long, global = true, value_name = "FILE")]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize .ekos/ workspace directory
    Init,
    /// Run observation passes and write knowledge to the ledger
    Build,
    /// Run knowledge-recovery compiler passes (SQL + Git analysis)
    Recover,
    /// Resolve synonymous concepts across sources into canonical identities
    Resolve,
    /// Run the semantic compiler: KIR → Canonical Knowledge Model
    Compile,
    /// Commit the CKM to the append-only knowledge ledger
    Commit,
    /// Ledger management subcommands
    Ledger {
        #[command(subcommand)]
        subcommand: LedgerCommands,
    },
    /// Clear the artifact cache (.ekos/artifacts/)
    Clean,
    /// Check the environment and configuration
    Doctor,
    /// Query the knowledge ledger
    Query {
        #[command(subcommand)]
        subcommand: QueryCommands,
    },
    /// Ask a natural-language question, answered from grounded, evidenced knowledge
    Ask {
        question: String,
        #[arg(long)]
        json: bool,
    },
    /// Run an Enterprise Knowledge Language query against the ledger
    Ekl {
        query: String,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum LedgerCommands {
    /// Show ledger entry count and object count
    Status,
}

#[derive(Subcommand)]
enum QueryCommands {
    /// Retrieve an object by ID
    Object {
        id: String,
        #[arg(long, default_value = "pretty")]
        format: String,
    },
    /// Full-text search over object names
    Find { query: String },
    /// BFS neighbourhood graph up to --depth hops
    Neighbourhood {
        id: String,
        #[arg(long, default_value = "1")]
        depth: u32,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let config_path = cli.config.unwrap_or_else(|| PathBuf::from("ekos.toml"));
    let config = ekos_compiler_core::EkosConfig::from_file_or_default(&config_path);
    let cwd = std::env::current_dir()?;

    ekos::commands::init_logging(&config);

    match cli.command {
        Commands::Init => ekos::commands::init::run(&config, &cwd),
        Commands::Build => ekos::commands::build::run(&config, &cwd).await,
        Commands::Recover => ekos::commands::recover::run(&config, &cwd).await,
        Commands::Resolve => ekos::commands::resolve::run(&config, &cwd),
        Commands::Compile => ekos::commands::compile::run(&config, &cwd).await,
        Commands::Commit => ekos::commands::commit::run(&config, &cwd),
        Commands::Ledger { subcommand } => match subcommand {
            LedgerCommands::Status => ekos::commands::ledger::status(&config, &cwd),
        },
        Commands::Clean => ekos::commands::clean::run(&config, &cwd),
        Commands::Doctor => ekos::commands::doctor::run(&config, &cwd, &config_path),
        Commands::Query { subcommand } => match subcommand {
            QueryCommands::Object { id, format } => {
                ekos::commands::query::object(&config, &cwd, &id, &format)
            }
            QueryCommands::Find { query } => ekos::commands::query::find(&config, &cwd, &query),
            QueryCommands::Neighbourhood { id, depth } => {
                ekos::commands::query::neighbourhood(&config, &cwd, &id, depth)
            }
        },
        Commands::Ask { question, json } => {
            ekos::commands::ask::run(&config, &cwd, &question, json).await
        }
        Commands::Ekl { query, json } => ekos::commands::ekl::run(&config, &cwd, &query, json),
    }
}
