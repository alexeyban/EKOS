use anyhow::Result;
use chrono::{DateTime, Utc};
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
    Recover {
        /// Run DAG-independent passes concurrently instead of sequentially
        #[arg(long)]
        parallel: bool,
    },
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
    /// Show what changed in the ledger between two points in time
    Diff {
        #[arg(long)]
        from: DateTime<Utc>,
        #[arg(long)]
        to: DateTime<Utc>,
    },
    /// Manage ledger branches
    Branch {
        #[command(subcommand)]
        subcommand: BranchCommands,
    },
    /// Model Context Protocol server (RFC 0013)
    Mcp {
        #[command(subcommand)]
        subcommand: McpCommands,
    },
    /// Artifact store management (RFC 0015)
    Artifact {
        #[command(subcommand)]
        subcommand: ArtifactCommands,
    },
}

#[derive(Subcommand)]
enum ArtifactCommands {
    /// Migrate loose artifact files into packed segments
    Repack,
}

#[derive(Subcommand)]
enum McpCommands {
    /// Serve MCP over stdio (newline-delimited JSON-RPC 2.0)
    Serve {
        /// Workspace directory containing .ekos/ (default: current directory)
        #[arg(long, value_name = "DIR")]
        workspace: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum BranchCommands {
    /// Create a new branch as a snapshot of the current ledger
    Create { name: String },
    /// List all branches
    List,
    /// Merge a branch's objects/relationships into the main ledger
    Merge { name: String },
    /// Delete a branch
    Delete { name: String },
}

#[derive(Subcommand)]
enum LedgerCommands {
    /// Show ledger entry count and object count
    Status {
        /// Also report per-component storage sizes (RFC 0015)
        #[arg(long)]
        storage: bool,
    },
    /// Migrate the ledger to the v2 compact format (RFC 0015)
    Migrate,
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

    // The MCP server is spawned by agent hosts from arbitrary directories, so
    // its workspace (and the config inside it) may arrive via environment
    // variables instead of flags: EKOS_WORKSPACE, EKOS_CONFIG.
    let env_workspace = std::env::var_os("EKOS_WORKSPACE").map(PathBuf::from);
    let config_path = cli
        .config
        .or_else(|| std::env::var_os("EKOS_CONFIG").map(PathBuf::from))
        .or_else(|| {
            if matches!(cli.command, Commands::Mcp { .. }) {
                env_workspace.as_ref().map(|w| w.join("ekos.toml"))
            } else {
                None
            }
        })
        .unwrap_or_else(|| PathBuf::from("ekos.toml"));
    let config = ekos_compiler_core::EkosConfig::from_file_or_default(&config_path);
    let cwd = std::env::current_dir()?;

    // The MCP server owns stdout for protocol frames; its logs go to stderr.
    if matches!(cli.command, Commands::Mcp { .. }) {
        ekos::commands::init_logging_stderr(&config);
    } else {
        ekos::commands::init_logging(&config);
    }

    match cli.command {
        Commands::Init => ekos::commands::init::run(&config, &cwd),
        Commands::Build => ekos::commands::build::run(&config, &cwd).await,
        Commands::Recover { parallel } => {
            ekos::commands::recover::run(&config, &cwd, parallel).await
        }
        Commands::Resolve => ekos::commands::resolve::run(&config, &cwd),
        Commands::Compile => ekos::commands::compile::run(&config, &cwd).await,
        Commands::Commit => ekos::commands::commit::run(&config, &cwd),
        Commands::Ledger { subcommand } => match subcommand {
            LedgerCommands::Status { storage } => {
                ekos::commands::ledger::status(&config, &cwd, storage)
            }
            LedgerCommands::Migrate => ekos::commands::ledger::migrate(&config, &cwd),
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
        Commands::Diff { from, to } => ekos::commands::diff::run(&config, &cwd, from, to),
        Commands::Branch { subcommand } => match subcommand {
            BranchCommands::Create { name } => ekos::commands::branch::create(&config, &cwd, &name),
            BranchCommands::List => ekos::commands::branch::list(&config, &cwd),
            BranchCommands::Merge { name } => ekos::commands::branch::merge(&config, &cwd, &name),
            BranchCommands::Delete { name } => ekos::commands::branch::delete(&config, &cwd, &name),
        },
        Commands::Mcp { subcommand } => match subcommand {
            McpCommands::Serve { workspace } => {
                let workspace = workspace.or(env_workspace).unwrap_or_else(|| cwd.clone());
                ekos::commands::mcp::run(&config, &workspace)
            }
        },
        Commands::Artifact { subcommand } => match subcommand {
            ArtifactCommands::Repack => ekos::commands::artifact::repack(&config, &cwd),
        },
    }
}
