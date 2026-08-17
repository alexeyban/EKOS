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
    /// Live NL-to-SQL query engine over a compiled ClickHouse schema (RFC 0056)
    #[command(name = "clickhouse")]
    ClickHouse {
        #[command(subcommand)]
        subcommand: ClickHouseCommands,
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
    /// Cross-system identity resolution subcommands (RFC 0029)
    Identity {
        #[command(subcommand)]
        subcommand: IdentityCommands,
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
    /// Marketing agent: devlog -> tweet draft -> approval -> X publish (RFC 0030)
    Marketing {
        #[command(subcommand)]
        subcommand: MarketingCommands,
    },
    /// Generated documentation from the compiled ledger (RFC 0035)
    Docs {
        #[command(subcommand)]
        subcommand: DocsCommands,
    },
    /// Pentaho -> dbt model export from the compiled Transformation IR (RFC 0036)
    Dbt {
        #[command(subcommand)]
        subcommand: DbtCommands,
    },
    /// Load and run a World Engine scenario (RFC 0051)
    Simulate {
        /// Path to the scenario YAML file
        scenario: PathBuf,
        /// Override the scenario's own simulation.rounds
        #[arg(long)]
        rounds: Option<u32>,
        /// Write to this ledger instead of the default scenario-scoped one
        /// at .ekos/simulations/<scenario-id>/ledger.db — WARNING: passing
        /// the real workspace ledger here permanently commingles fictional
        /// simulation entities with real compiled knowledge (no delete/
        /// tombstone mechanism exists anywhere in this codebase, RFC 0043).
        #[arg(long)]
        ledger: Option<PathBuf>,
        /// Override the scenario's own simulation.seed (RFC 0052) — governs
        /// reproducible priority tie-breaking and resource-contention
        /// ordering, never what an agent decides to do.
        #[arg(long)]
        seed: Option<u64>,
    },
    /// Read back a previously recorded simulation (RFC 0054) — read-only,
    /// does not run any new rounds
    Replay {
        /// Path to the scenario YAML file (used only to resolve the
        /// scenario-scoped ledger path and names, never re-run)
        scenario: PathBuf,
        /// Show only this round instead of every recorded round
        #[arg(long)]
        round: Option<u32>,
        /// Read from this ledger instead of the default scenario-scoped one
        #[arg(long)]
        ledger: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum DbtCommands {
    /// Render dbt SQL models + schema.yml from already-committed Custom("TransformNode")
    /// objects, ref()-chained via real FeedsInto edges. No LLM calls, no cost. Filter/Calculate
    /// expressions and Unmapped nodes render as flagged raw text/stubs, never silently
    /// transpiled.
    Generate {
        /// Output directory (default: <workspace>/dbt-generated)
        #[arg(long, value_name = "DIR")]
        output: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum DocsCommands {
    /// Render deterministic Markdown or HTML pages from already-committed ledger objects,
    /// with Mermaid diagrams. No LLM calls, no cost — unless --prose is given.
    Generate {
        /// Output directory (default: <workspace>/docs-generated)
        #[arg(long, value_name = "DIR")]
        output: Option<PathBuf>,
        /// Output format: "md" (default) or "html"
        #[arg(long, default_value = "md")]
        format: String,
        /// Output layout: "objects" (default, one page per compiled object) or "curated"
        /// (README.md/Architecture.md/API.md/SequenceDiagrams.md — RFC 0037, Markdown only)
        #[arg(long, default_value = "objects")]
        layout: String,
        /// Opt-in: add an LLM-written "Overview" to each page, grounded and citation-validated
        /// via the same pipeline `ekos ask` uses. Shows a token-cost estimate and asks for
        /// confirmation first, unless --yes is also given.
        #[arg(long)]
        prose: bool,
        /// Skip the --prose confirmation prompt
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Subcommand)]
enum MarketingCommands {
    /// Draft (and, after approval, publish) a tweet for a devlog
    Publish {
        /// Path, bare devlog number (e.g. "28"), or "latest" (default: latest devlog_*.md)
        devlog: Option<String>,
        /// Skip the interactive approval prompt and publish as drafted
        #[arg(long)]
        yes: bool,
        /// Never call the real Publisher or record a posted entry
        #[arg(long)]
        dry_run: bool,
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
enum ClickHouseCommands {
    /// Ask a natural-language question, answered by an LLM-built SQL query run live against
    /// ClickHouse (SELECT-only, validated before execution — RFC 0056)
    Ask {
        question: String,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum IdentityCommands {
    /// Scan the ledger for candidate cross-system matches (e.g. Informix
    /// `cust_mstr` vs. Postgres `customers`); written as unconfirmed until
    /// reviewed via the ekos_identity_review MCP tool.
    Scan,
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
    /// Migrate the ledger: v2 compact format (RFC 0015), or --v3 for the
    /// fact engine (RFC 0016)
    Migrate {
        /// Migrate to the RFC 0016 fact-segment engine
        #[arg(long)]
        v3: bool,
    },
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
        Commands::Identity { subcommand } => match subcommand {
            IdentityCommands::Scan => ekos::commands::identity::scan(&config, &cwd),
        },
        Commands::Compile => ekos::commands::compile::run(&config, &cwd).await,
        Commands::Commit => ekos::commands::commit::run(&config, &cwd),
        Commands::Ledger { subcommand } => match subcommand {
            LedgerCommands::Status { storage } => {
                ekos::commands::ledger::status(&config, &cwd, storage)
            }
            LedgerCommands::Migrate { v3 } => ekos::commands::ledger::migrate(&config, &cwd, v3),
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
        Commands::ClickHouse { subcommand } => match subcommand {
            ClickHouseCommands::Ask { question, json } => {
                ekos::commands::clickhouse::ask(&config, &cwd, &question, json).await
            }
        },
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
        Commands::Marketing { subcommand } => match subcommand {
            MarketingCommands::Publish {
                devlog,
                yes,
                dry_run,
            } => ekos::commands::marketing::publish(&config, &cwd, devlog, yes, dry_run).await,
        },
        Commands::Docs { subcommand } => match subcommand {
            DocsCommands::Generate {
                output,
                format,
                layout,
                prose,
                yes,
            } => {
                let output = ekos::commands::docs::resolve_output_dir(&cwd, output);
                let format = ekos::commands::docs::Format::parse(&format)?;
                let layout = ekos::commands::docs::Layout::parse(&layout)?;
                ekos::commands::docs::generate(&config, &cwd, &output, format, layout, prose, yes)
                    .await
            }
        },
        Commands::Dbt { subcommand } => match subcommand {
            DbtCommands::Generate { output } => {
                let output = ekos::commands::dbt::resolve_output_dir(&cwd, output);
                ekos::commands::dbt::generate(&cwd, &output, &config).await
            }
        },
        Commands::Simulate {
            scenario,
            rounds,
            ledger,
            seed,
        } => ekos::commands::simulate::run(&config, &cwd, &scenario, rounds, ledger, seed),
        Commands::Replay {
            scenario,
            round,
            ledger,
        } => ekos::commands::replay::run(&config, &cwd, &scenario, round, ledger),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 0056: clap auto-kebab-cases `ClickHouse` to `click-house` by default (splitting on
    /// the internal case boundary) — every doc/README/RFC reference uses the one-word
    /// `ekos clickhouse ask`, so the variant needs an explicit `#[command(name = "clickhouse")]`
    /// override. Found live: `ekos clickhouse ask "..."` failed with "unrecognized subcommand
    /// 'clickhouse'" (suggesting 'click-house') the first time this was actually run from a
    /// shell, not caught by any unit test until this one was added.
    #[test]
    fn clickhouse_ask_parses_as_one_word_not_kebab_cased() {
        let cli = Cli::try_parse_from(["ekos", "clickhouse", "ask", "how many orders?"]).unwrap();
        assert!(matches!(
            cli.command,
            Commands::ClickHouse {
                subcommand: ClickHouseCommands::Ask { .. }
            }
        ));
    }
}
