use crate::extension::Extensions;
use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};
use std::ffi::OsString;
use std::path::PathBuf;

/// The whole command tree, for `ekos commands --json` (RFC 0186).
pub fn cli_command() -> clap::Command {
    Cli::command()
}

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

    /// The EKOS home holding projects and their compiled data (default: $EKOS_HOME, then
    /// ~/.ekos) — RFC 0178
    #[arg(long, global = true, value_name = "DIR")]
    home: Option<PathBuf>,

    /// Run against a project in the EKOS home instead of the current directory: its config,
    /// its sources, its own ledger (also $EKOS_PROJECT) — RFC 0178
    #[arg(long, global = true, value_name = "NAME")]
    project: Option<String>,

    /// The tenant the project belongs to (default: `default`; also $EKOS_TENANT)
    #[arg(long, global = true, value_name = "NAME")]
    tenant: Option<String>,

    /// Show only what this read scope allows: a JSON file of allow / deny rules by source, path
    /// or object (also $EKOS_READ_SCOPE). For `mcp serve`, `ekl` and `query` — RFC 0181
    #[arg(long, global = true, value_name = "FILE")]
    read_scope: Option<PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// RFC 0191: a command an extension added. Never produced by the derive; `parse_cli` builds
    /// it from the matches.
    #[command(skip)]
    Extension {
        name: String,
        matches: clap::ArgMatches,
    },
    /// Initialize .ekos/ workspace directory
    Init {
        /// Scan the workspace and write an ekos.toml that reflects what is actually in it —
        /// SQL dialect, exclusions for third-party/generated directories, detected inputs
        /// (RFC 0152)
        #[arg(long)]
        detect: bool,
        /// Print the config that would be written and write nothing
        #[arg(long)]
        dry_run: bool,
        /// Overwrite an existing ekos.toml
        #[arg(long)]
        force: bool,
    },
    /// Projects in the EKOS home: config and compiled data kept outside the sources (RFC 0178)
    Project {
        #[command(subcommand)]
        subcommand: ProjectCommands,
    },
    /// The EKOS home as a whole: back it up, restore it (RFC 0183)
    Home {
        #[command(subcommand)]
        subcommand: HomeCommands,
    },
    /// Install or reconfigure EKOS: where it goes, the web console, agents, start at login
    /// (RFC 0185). Opens a wizard in the browser, or applies `--answers` without one
    Setup {
        /// Apply these answers (TOML) instead of opening the wizard
        #[arg(long, value_name = "FILE")]
        answers: Option<PathBuf>,
        /// With --answers: print what would be done, do nothing
        #[arg(long)]
        dry_run: bool,
        /// Print the wizard's address instead of opening a browser
        #[arg(long)]
        no_browser: bool,
        /// Port for the wizard (default: any free one)
        #[arg(long, default_value_t = 0)]
        port: u16,
    },
    /// Remove what `ekos setup` installed. The home (projects, the console's database) stays
    /// unless --purge
    Uninstall {
        /// Also delete the EKOS home: every project and its compiled knowledge
        #[arg(long, conflicts_with = "ui")]
        purge: bool,
        /// Confirm --purge
        #[arg(long, conflicts_with = "ui")]
        yes: bool,
        /// Show what will be removed on a page in the browser and confirm there (RFC 0188)
        #[arg(long)]
        ui: bool,
        /// With --ui: print the page's address instead of opening a browser
        #[arg(long, requires = "ui")]
        no_browser: bool,
        /// With --ui: port for the page (default: any free one)
        #[arg(long, default_value_t = 0, requires = "ui")]
        port: u16,
        /// With --ui: serve the page from a new background process, print its address and
        /// return (what the web console uses)
        #[arg(long, requires = "ui")]
        detach: bool,
    },
    /// Every command, its arguments and what kind of command it is, as JSON: what the web
    /// console builds its run forms from (RFC 0186)
    #[command(name = "commands")]
    Catalogue {
        /// Print JSON (the only format)
        #[arg(long)]
        json: bool,
    },
    /// Every extension's console pages as JSON, checked against the command catalogue: what the
    /// web console draws extension tabs from (RFC 0198)
    Extensions {
        /// Accepted for symmetry with other commands; the output is always JSON
        #[arg(long)]
        json: bool,
    },
    /// The web console of this home: start, stop, open it (RFC 0185)
    Console {
        #[command(subcommand)]
        subcommand: ConsoleCommands,
    },
    /// The named inputs of a project: repositories and directories compiled into one ledger
    /// (RFC 0178)
    Source {
        #[command(subcommand)]
        subcommand: SourceCommands,
    },
    /// Run observation passes and write knowledge to the ledger
    Build {
        /// Re-observe one named source only; the others keep what the last build recorded
        #[arg(long, value_name = "NAME")]
        source: Option<String>,
    },
    /// Run knowledge-recovery compiler passes (SQL + Git analysis)
    Recover {
        /// Run DAG-independent passes concurrently instead of sequentially
        #[arg(long)]
        parallel: bool,
    },
    /// Resolve synonymous concepts across sources into canonical identities
    Resolve {
        /// Print conflicts as diagnostics but don't fail the pipeline on them
        #[arg(long)]
        force: bool,
    },
    /// Run the semantic compiler: KIR → Canonical Knowledge Model
    Compile,
    /// Commit the CKM to the append-only knowledge ledger
    Commit {
        /// Skip the LLM-spend confirmation prompt (RFC 0088's `[llm-description]`, only relevant
        /// when that's enabled in `ekos.toml`) and proceed automatically.
        #[arg(long)]
        yes: bool,
    },
    /// Ledger management subcommands
    Ledger {
        #[command(subcommand)]
        subcommand: LedgerCommands,
    },
    /// Clear the artifact cache (.ekos/artifacts/)
    Clean,
    /// Report which input kinds compiled into objects — and which silently produced nothing
    /// (RFC 0152)
    Coverage {
        /// Emit one machine-readable JSON object instead of the text table
        #[arg(long)]
        json: bool,
        /// Exit non-zero when any input kind produced zero objects
        #[arg(long)]
        strict: bool,
        /// Include kinds with no inputs in this workspace
        #[arg(long)]
        all: bool,
    },
    /// LLM routes per task, recorded usage and cost, and which models a key reaches (RFC 0197)
    Llm {
        #[command(subcommand)]
        subcommand: LlmCommands,
    },
    /// Which source files changed since the ledger was committed, and which compiled objects
    /// cite them (RFC 0171)
    Freshness {
        /// Emit one machine-readable JSON object instead of the text report
        #[arg(long)]
        json: bool,
        /// Paths listed per category (changed / added / removed)
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },
    /// Check the environment and configuration
    Doctor {
        /// Emit one machine-readable JSON object instead of the text checklist (RFC 0129 R5)
        #[arg(long)]
        json: bool,
    },
    /// Inspect `ekos.toml` — validate it, or preview what `ekos build` would observe (RFC 0130)
    Config {
        #[command(subcommand)]
        subcommand: ConfigCommands,
    },
    /// Show ledger entry count and object count (top-level alias for `ekos ledger status`)
    Status {
        /// Also report per-component storage sizes (RFC 0015)
        #[arg(long)]
        storage: bool,
        /// Emit a single machine-readable JSON object instead of the text report (RFC 0127 R2)
        #[arg(long)]
        json: bool,
    },
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
        /// Print the answer as it's generated instead of waiting for the full
        /// response (RFC 0098). Not compatible with --json, which needs the
        /// complete structured result.
        #[arg(long)]
        stream: bool,
        /// Continue a named multi-turn conversation (RFC 0099) — prior
        /// question/answer pairs from `.ekos/ask-sessions/<name>.json` are
        /// sent as real conversation history, and this turn is appended
        /// back to it. Letters, digits, '_', and '-' only.
        #[arg(long)]
        session: Option<String>,
        /// Use the pre-RFC-0123 retrieval path (BM25 → whole-object JSON → LLM)
        /// instead of the REASON planner + typed evidence set. Implied by
        /// --stream.
        #[arg(long)]
        classic: bool,
        /// Print the compiled query plan and the typed evidence set alongside
        /// the answer (RFC 0124). Not compatible with --classic.
        #[arg(long)]
        explain: bool,
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
    /// Business-semantics hypotheses recovered from code traces (RFC 0170)
    Semantics {
        #[command(subcommand)]
        subcommand: SemanticsCommands,
    },
    /// Where sources disagree about one fact — both claims kept, for a person to decide (RFC 0172)
    Conflicts {
        #[command(subcommand)]
        subcommand: ConflictsCommands,
    },
    /// The source-model review: scope, PII, conflicts, identity, write behaviour, glossary (RFC 0213)
    Review {
        #[command(subcommand)]
        subcommand: ReviewCommands,
    },
    /// Sign a review subject off on the digest you were shown; stale when a signed fact changes (RFC 0213)
    Signoff {
        #[command(subcommand)]
        subcommand: SignoffCommands,
    },
    /// Export compiled knowledge to other formats (RFC 0170: LinkML)
    Export {
        #[command(subcommand)]
        subcommand: ExportCommands,
    },
    /// Import an expert's edits back as review decisions (RFC 0170: LinkML round trip)
    Import {
        #[command(subcommand)]
        subcommand: ImportCommands,
    },
    /// Cross-system identity resolution subcommands (RFC 0029)
    Identity {
        #[command(subcommand)]
        subcommand: IdentityCommands,
    },
    /// DAO treasury compliance: match on-chain payments to governance approvals (RFC 0032)
    Treasury {
        #[command(subcommand)]
        subcommand: TreasuryCommands,
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
    /// Distributed-mode metadata coordinator (RFC 0113 B3)
    Coordinator {
        #[command(subcommand)]
        subcommand: CoordinatorCommands,
    },
    /// Distributed-mode compile worker — Service A (RFC 0113 B3)
    CompileWorker {
        #[command(subcommand)]
        subcommand: CompileWorkerCommands,
    },
    /// Distributed-mode query worker — Service B (RFC 0113 B4)
    QueryWorker {
        #[command(subcommand)]
        subcommand: QueryWorkerCommands,
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
    /// Bulk graph extraction from the compiled ledger (RFC 0127)
    Graph {
        #[command(subcommand)]
        subcommand: GraphCommands,
    },
    /// Pentaho -> dbt model export from the compiled Transformation IR (RFC 0036)
    Dbt {
        #[command(subcommand)]
        subcommand: DbtCommands,
    },
    /// Architecture Knowledge Model reasoning + investigation loop (RFC 0065/0066)
    Architecture {
        #[command(subcommand)]
        subcommand: ArchitectureCommands,
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
    /// End-to-end agent/answer evaluation harness (RFC 0138)
    Eval {
        #[command(subcommand)]
        subcommand: EvalCommands,
    },
    /// Evidence-backed migration to an analytical target (RFC 0154) — needs `[migrate] enabled = true`
    Migrate {
        #[command(subcommand)]
        subcommand: MigrateCommands,
    },
    /// Agent session memory inbox (RFC 0151) — needs `[session-memory] enabled = true`
    Session {
        #[command(subcommand)]
        subcommand: SessionCommands,
    },
}

#[derive(Subcommand)]
enum MigrateCommands {
    /// Create a migration project and its connection references (no credentials are ever stored)
    Init {
        /// Project name
        #[arg(long)]
        name: String,
        /// Source, as `postgres://<alias>/<database>` — a DSN carrying a password is refused
        #[arg(long)]
        source: String,
        /// Target, as `clickhouse://<alias>/<database>` or `delta://<alias>/<catalog>`
        #[arg(long)]
        target: String,
        /// Name of the environment variable holding the source password
        #[arg(long)]
        source_secret_env: Option<String>,
        /// Name of the environment variable holding the target password
        #[arg(long)]
        target_secret_env: Option<String>,
    },
    /// Read the live catalog, create a unit per table, and record live-vs-repo drift
    Discover {
        #[arg(long)]
        project: Option<String>,
        /// Restrict to these schemas (default: every non-system schema)
        #[arg(long = "schema")]
        schemas: Vec<String>,
    },
    /// Profile discovered units (p0: catalog only, p1: bounded sample, p2: exact, budgeted)
    Profile {
        #[arg(long)]
        project: Option<String>,
        #[arg(long, default_value = "p1")]
        tier: String,
        /// One unit only (default: all)
        #[arg(long)]
        unit: Option<String>,
    },
    /// Run the data-quality and target-compatibility rules against the live source
    Assess {
        #[arg(long)]
        project: Option<String>,
        /// One unit only (default: all)
        #[arg(long)]
        unit: Option<String>,
        /// Skip the affected-row measurements (rules only, no scans)
        #[arg(long)]
        no_measure: bool,
    },
    /// Choose target types and a table design for each unit, and emit DDL
    Map {
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        unit: Option<String>,
        /// Write the generated DDL to this path
        #[arg(long)]
        emit: Option<std::path::PathBuf>,
    },
    /// Create the target table and copy the data, chunk by chunk
    Load {
        #[arg(long)]
        project: Option<String>,
        /// The unit to load
        #[arg(long)]
        unit: String,
        #[arg(long, default_value = "sandbox")]
        env: String,
        /// Key-range width per chunk
        #[arg(long, default_value_t = 100_000)]
        chunk_rows: i64,
        /// Gate every statement and print it, executing nothing
        #[arg(long)]
        dry_run: bool,
    },
    /// Run the RFC 0156 validation tiers over a loaded unit
    Validate {
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        unit: String,
        /// v1 | v2 | v3
        #[arg(long, default_value = "v3")]
        tier: String,
    },
    /// List approval requests, or raise one for a unit
    Review {
        #[arg(long)]
        project: Option<String>,
        /// Raise a request for this unit (omit to list)
        #[arg(long)]
        unit: Option<String>,
        #[arg(long, default_value = "staging")]
        env: String,
    },
    /// Approve a request (human-only; there is deliberately no MCP equivalent)
    Approve {
        request: String,
        #[arg(long)]
        project: Option<String>,
        /// Who is approving; pass twice for an R4 action
        #[arg(long = "as")]
        subjects: Vec<String>,
        /// The object name, typed exactly, for an R4 action
        #[arg(long)]
        confirm: Option<String>,
        /// Record that the evidence was rendered and reviewed (required at R3+)
        #[arg(long)]
        show_evidence: bool,
    },
    /// Reject a request
    Reject {
        request: String,
        #[arg(long)]
        project: Option<String>,
        #[arg(long = "as")]
        subjects: Vec<String>,
        #[arg(long)]
        reason: String,
    },
    /// Compile the migration report from ledger facts
    Report {
        #[arg(long)]
        project: Option<String>,
        /// Groundedness the report must reach to be signable
        #[arg(long, default_value_t = 0.95)]
        threshold: f64,
        /// Write to this path instead of stdout
        #[arg(long)]
        out: Option<std::path::PathBuf>,
    },
    /// RFC 0164: lower every recovered view to the target, propose a redesign per trigger, and
    /// say which routines may be reconstructed. Reads the ledger and source files only
    Lower {
        /// Target database the migrated tables live in; replaces a source schema qualifier
        #[arg(long)]
        database: Option<String>,
        /// Print the lowered statement under each view
        #[arg(long)]
        sql: bool,
        /// Save each lowering and trigger proposal as a `MigrationProposal`, status `proposed`
        #[arg(long)]
        record: bool,
        #[arg(long)]
        json: bool,
    },
    /// RFC 0164 V5: run each lowered view against its source on two sandbox engines, over a
    /// fixture built from the view's own comparisons. Both connections must be `sandbox = true`
    V5 {
        /// PostgreSQL sandbox, as `postgres://<alias>/<database>`
        #[arg(long)]
        source_sandbox: String,
        /// ClickHouse sandbox, as `clickhouse://<alias>/<database>`
        #[arg(long)]
        target_sandbox: String,
        /// Save each proposal together with its V5 result
        #[arg(long)]
        record: bool,
        #[arg(long)]
        json: bool,
    },
    /// RFC 0164: ask the configured model for a ClickHouse implementation of one PL/pgSQL
    /// routine. Refused before any call unless every statement was recovered; the answer is kept
    /// only if it states nothing the routine does not
    Reconstruct {
        /// The routine: a name, or `<path>:<name>` when several share one
        #[arg(long)]
        source: String,
        /// Allow a model that is not local (and so may be billed) for this run
        #[arg(long)]
        allow_metered: bool,
        /// Save a surviving candidate as a `MigrationProposal` (author `llm`, status `proposed`)
        #[arg(long)]
        record: bool,
        #[arg(long)]
        json: bool,
    },
    /// RFC 0164: check that a candidate target statement states nothing its source does not
    CheckCandidate {
        /// The View or Procedure it replaces: a name, or `<path>:<name>` when several share one
        #[arg(long)]
        source: String,
        /// File holding the candidate ClickHouse statement
        #[arg(long)]
        candidate: std::path::PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Units by state, and what is blocking
    Status {
        /// Project name (default: `[migrate] project` from ekos.toml)
        #[arg(long)]
        project: Option<String>,
    },
}

#[derive(Subcommand)]
enum SessionCommands {
    /// Record a note in the redacted, capped session inbox (never touches the ledger)
    Note {
        /// The note text
        text: String,
        /// finding | decision | dead_end | constraint | todo
        #[arg(long, default_value = "finding")]
        kind: String,
        /// Why — the reasoning behind a decision or dead end
        #[arg(long)]
        rationale: Option<String>,
        /// A real object/path this note is about (repeatable); resolved later, never trusted here
        #[arg(long = "anchor")]
        anchors: Vec<String>,
        /// Session id (default: one per calendar day)
        #[arg(long)]
        session: Option<String>,
    },
    /// Show note counts, dropped notes and how many are pending commit
    Status {
        /// Only this session (default: all)
        #[arg(long)]
        session: Option<String>,
    },
    /// Turn pending inbox notes into unconfirmed, anchored ledger claims (observe → map → commit)
    Commit {
        #[arg(long)]
        session: Option<String>,
    },
    /// Search notes from earlier sessions (explicit refusal when nothing is relevant)
    Recall {
        query: String,
        #[arg(long, default_value_t = 8)]
        limit: usize,
        #[arg(long)]
        json: bool,
    },
    /// Print a token-budgeted session brief
    Brief {
        /// Object names / paths you are about to work on (repeatable)
        #[arg(long = "scope")]
        scope: Vec<String>,
        /// Derive scope from `git diff --name-only HEAD`
        #[arg(long)]
        scope_from_git: bool,
        #[arg(long, default_value_t = 800)]
        budget: usize,
        /// text | claude-hook (SessionStart hook JSON)
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Human-only: confirm, reject or supersede a session claim (no MCP equivalent, by design)
    Review {
        claim_id: String,
        /// confirm | reject | supersede
        decision: String,
        /// For `supersede`: the id of the claim that replaces this one
        #[arg(long)]
        by: Option<String>,
    },
    /// Delete inbox files and captured slices; ledger claims already committed are NOT undone
    Purge {
        #[arg(long)]
        session: Option<String>,
        /// Also delete every session whose inbox is older than this many days
        #[arg(long)]
        older_than_days: Option<u64>,
    },
    /// Redact a transcript and store it as content-addressed slices (outside the ledger)
    Capture {
        #[arg(long)]
        session: String,
        /// Transcript file (default: stdin)
        #[arg(long)]
        file: Option<PathBuf>,
    },
    /// Opt-in LLM extraction of claim proposals from captured slices (needs `[session-memory] extraction = true`)
    Extract {
        #[arg(long)]
        session: String,
    },
    /// Run the deterministic session-continuity proxy eval and print the report
    Eval {
        #[arg(long, default_value_t = 5)]
        runs: usize,
    },
    /// Measure how often anchor fingerprints flip vs raw property changes on this ledger
    FingerprintNoise,
}

#[derive(Subcommand)]
enum EvalCommands {
    /// Run a scenario dataset against a real, already-built workspace and print the report
    Run {
        /// Named dataset from evals/datasets/manifest.yaml, or a bare category file stem
        /// (e.g. "architecture"). Omit to run every *.yaml in --datasets-dir, named
        /// "ekos-<total scenario count>".
        #[arg(long)]
        dataset: Option<String>,
        /// Default: <cwd>/evals/datasets
        #[arg(long, value_name = "DIR")]
        datasets_dir: Option<PathBuf>,
        /// Only run scenarios from this dataset file's `category:`
        #[arg(long)]
        category: Option<String>,
        /// Override config.llm.provider for this run only: claude|ollama|openai
        #[arg(long)]
        agent: Option<String>,
        /// Grade only the first N matching scenarios
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        json: bool,
        /// Save the JSON report here instead of evals/reports/<timestamp>-<dataset>.json
        #[arg(long, value_name = "FILE")]
        output: Option<PathBuf>,
        /// Persist each answer and the evidence behind it into the report, so failures can be
        /// attributed and the run re-graded offline later (RFC 0139). Adds ~1MB per full run.
        #[arg(long)]
        save_answers: bool,
        /// Measure retrieval only: Recall@10 and whether each scenario's expected facts are in
        /// the evidence a model would be shown. Calls no model (RFC 0177)
        #[arg(long)]
        evidence_only: bool,
        /// Do not leave documents that quote a scenario's question out of search (RFC 0177)
        #[arg(long)]
        keep_question_echoes: bool,
    },
    /// Re-score a saved report under the current evaluators — no LLM calls (RFC 0139)
    Regrade {
        /// The saved report to re-grade (must have been written with --save-answers)
        report: PathBuf,
        /// Default: <cwd>/evals/datasets
        #[arg(long, value_name = "DIR")]
        datasets_dir: Option<PathBuf>,
        #[arg(long)]
        json: bool,
        /// Write the re-graded report here
        #[arg(long, value_name = "FILE")]
        output: Option<PathBuf>,
    },
    /// List saved eval runs (evals/reports/*.json) as a trend table
    History {
        /// Default: <cwd>/evals/reports
        #[arg(long, value_name = "DIR")]
        reports_dir: Option<PathBuf>,
        /// Show only the last N runs
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum CoordinatorCommands {
    /// Run the coordinator over newline-delimited JSON-RPC on TCP until killed
    Serve {
        /// Address to bind, e.g. 0.0.0.0:7333 or 127.0.0.1:0
        #[arg(long, default_value = "127.0.0.1:7333")]
        listen: String,
        /// JSON state file to load/persist (catalog + watermarks + entity index).
        /// Omit for an ephemeral, non-persisting coordinator.
        #[arg(long)]
        state: Option<PathBuf>,
        /// Write-lease TTL in seconds (default 30)
        #[arg(long)]
        ttl_seconds: Option<i64>,
    },
    /// Connect to a running coordinator and print its catalog + watermarks
    Status {
        #[arg(long, default_value = "127.0.0.1:7333")]
        coordinator: String,
    },
}

#[derive(Subcommand)]
enum CompileWorkerCommands {
    /// Under a coordinator write-lease, run the real build→recover→resolve→compile→commit
    /// pipeline against a local partitioned workspace, register its partitions, and commit the
    /// new manifest generation (fenced). RFC 0113 Service A.
    Run {
        #[arg(long, default_value = "127.0.0.1:7333")]
        coordinator: String,
        /// Lease name for this compile run (one writer per shard). With entity-kind partitioning
        /// there is effectively one shard for the workspace, e.g. "main".
        #[arg(long, default_value = "main")]
        shard: String,
        /// Workspace directory (must hold an ekos.toml with [storage.partition], not
        /// [storage.distributed])
        #[arg(long, default_value = ".")]
        workspace: PathBuf,
        /// Recover connectors in parallel
        #[arg(long)]
        parallel: bool,
        /// Print identity conflicts as diagnostics but don't fail the pipeline on them — the
        /// Service A equivalent of `ekos resolve --force` (a co-located `ekos resolve` has this
        /// flag; without it here, any conflict aborts every compile-worker run).
        #[arg(long)]
        force: bool,
        /// Seconds to keep retrying if the shard's lease is already held by another worker,
        /// before giving up — 0 (default) preserves the original fail-fast behavior. Only retries
        /// an "already leased" conflict; every other error (bad coordinator address, invalid
        /// workspace, …) still fails immediately regardless of this flag.
        #[arg(long, default_value_t = 0)]
        retry_lease_seconds: u64,
    },
}

#[derive(Subcommand)]
enum QueryWorkerCommands {
    /// Serve `KnowledgeStore` reads for coordinator-assigned partitions over JSON-RPC
    Serve {
        #[arg(long, default_value = "127.0.0.1:7333")]
        coordinator: String,
        #[arg(long, default_value = "127.0.0.1:7334")]
        listen: String,
        /// Local directory partitions are materialised into (object storage → local cache)
        #[arg(long, default_value = ".ekos/query-cache")]
        cache: PathBuf,
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
enum ArchitectureCommands {
    /// Run the RFC 0066 MVP investigation loop: broad collection, deterministic crate-topology
    /// extraction, LLM-backed role classification, evaluation, and targeted re-investigation of
    /// any crate the evaluator flagged unclassified — up to --max-iterations, or until
    /// --quality-threshold is reached. Always ends by generating curated docs (RFC 0035/0037).
    Investigate {
        /// Stop after this many iterations even if the quality threshold wasn't reached.
        #[arg(long, default_value_t = 3)]
        max_iterations: u32,
        /// Evaluation score (0.0-1.0) at which the investigation stops early.
        #[arg(long, default_value_t = 0.90)]
        quality_threshold: f32,
        /// Output directory for the generated curated docs (default: <workspace>/doc)
        #[arg(long, value_name = "DIR")]
        output: Option<PathBuf>,
    },
    /// Real architecture-level diff between two points in time (RFC 0068 §55) — technologies,
    /// crate role classifications, risks, and open questions that changed. Distinct from `ekos
    /// diff`'s raw ledger-entry-id report.
    Diff {
        #[arg(long)]
        from: DateTime<Utc>,
        #[arg(long)]
        to: DateTime<Utc>,
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
        /// Output layout: "objects" (default, one page per compiled object), "curated"
        /// (README.md/Architecture.md/API.md/SequenceDiagrams.md — RFC 0037, Markdown only), or
        /// "solution-architect" (DependencyRiskReport.md/OnboardingGuide.md/FindingsMemo.md —
        /// RFC 0090, Markdown only)
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
enum GraphCommands {
    /// Export the whole compiled graph as JSON (or NDJSON) — nodes, edges, and the filters and
    /// truncation that produced them. Read-only, deterministic (modulo `generated_at`), no LLM.
    Export {
        /// Workspace directory (default: current directory)
        #[arg(long, value_name = "DIR")]
        workspace: Option<PathBuf>,
        /// "object" (default, one node per compiled object) or "aggregate" (super-nodes)
        #[arg(long, default_value = "object")]
        level: String,
        /// "json" (default, one pretty object) or "ndjson" (header + one record per node/edge)
        #[arg(long, default_value = "json")]
        format: String,
        /// Object-kind include-list (repeatable). Default: every kind.
        #[arg(long = "kind", value_name = "ObjectKind")]
        kinds: Vec<String>,
        /// Relationship-kind include-list (repeatable). Default: every kind.
        #[arg(long = "rel-kind", value_name = "Kind")]
        rel_kinds: Vec<String>,
        /// Relationship kinds to drop, applied after --rel-kind (repeatable)
        #[arg(long = "exclude-rel-kind", value_name = "Kind")]
        exclude_rel_kinds: Vec<String>,
        /// "kind" (default) or "path-prefix" — only used with --level aggregate
        #[arg(long, default_value = "kind")]
        group_by: String,
        /// Path segments to group by for --group-by path-prefix
        #[arg(long, default_value_t = 2)]
        path_prefix_depth: usize,
        /// Cap on returned nodes; excess dropped by degree-descending (reported in `truncated`)
        #[arg(long, default_value_t = 5000)]
        max_nodes: usize,
        /// Cap on returned edges
        #[arg(long, default_value_t = 20000)]
        max_edges: usize,
        /// Drop object-level nodes whose post-filter degree is below this (single pass)
        #[arg(long, default_value_t = 0)]
        min_degree: usize,
        /// Object property keys to carry into each node's `p` (repeatable). Default: none.
        #[arg(long = "include-property", value_name = "KEY")]
        include_properties: Vec<String>,
        /// Reconstruct the graph as it stood at this RFC 3339 instant (RFC 0134). Default: now.
        #[arg(long = "as-of", value_name = "RFC3339")]
        as_of: Option<String>,
        /// Stamp each node/edge with its first-seen time (`fs` in the output) — RFC 0134
        #[arg(long = "first-seen")]
        first_seen: bool,
        /// Write to this file instead of stdout
        #[arg(long, value_name = "FILE")]
        output: Option<PathBuf>,
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
    /// Post a file's text to X exactly as written: no drafting, no 280-character cap
    /// (an X Premium account accepts long posts). Without --confirm it only previews.
    Post {
        /// The text file to post (trailing whitespace is trimmed)
        file: PathBuf,
        /// Really post it; the decision is made per run and ignores the
        /// `[marketing.twitter] enabled` gate, which governs `publish` only
        #[arg(long)]
        confirm: bool,
    },
}

#[derive(Subcommand)]
enum ArtifactCommands {
    /// Migrate loose artifact files into packed segments
    Repack,
}

#[derive(Subcommand)]
enum SourceCommands {
    /// Add a directory to the project under a name
    Add {
        /// Source name: lowercase letters, digits, `-` and `_`
        name: String,
        /// The directory; nothing is written into it
        #[arg(long, value_name = "DIR")]
        path: PathBuf,
        /// `git` or `dir` (default: `git` when the directory is a repository)
        #[arg(long, value_name = "KIND")]
        kind: Option<String>,
    },
    /// List the project's sources
    List {
        #[arg(long)]
        json: bool,
    },
    /// Remove a source from the project's config. Compiled objects and the directory stay
    Remove { name: String },
}

#[derive(Subcommand)]
enum ProjectCommands {
    /// Create a project for the sources in a directory
    Add {
        /// Project name: lowercase letters, digits, `-` and `_`
        name: String,
        /// The directory holding the project's sources; nothing is written into it. For a
        /// project of several inputs, repeat it as `<source name>=<dir>`
        #[arg(long, value_name = "DIR | NAME=DIR", required = true)]
        source: Vec<String>,
        /// Scan the sources and write a config that reflects what is in them (RFC 0152)
        #[arg(long)]
        detect: bool,
    },
    /// List every project in the home
    List {
        #[arg(long)]
        json: bool,
    },
    /// Show where a project's sources, config and state are
    Show {
        name: String,
        #[arg(long)]
        json: bool,
    },
    /// Remove a project. Its data is moved to the home's trash unless --purge is given
    Remove {
        name: String,
        /// Delete the ledger and compiled artifacts instead of keeping them in the trash
        #[arg(long)]
        purge: bool,
        /// Confirm --purge
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Subcommand)]
enum HomeCommands {
    /// Write a backup of every project's config and compiled data, and the console's database.
    /// With the global --tenant, that tenant's projects only
    Backup {
        /// Where to write it (default: <home>/backups/ekos-backup-<time>.tar.zst)
        #[arg(long, value_name = "FILE")]
        out: Option<PathBuf>,
    },
    /// Restore a backup into the home. Nothing that exists is replaced without --force
    Restore {
        /// The archive `ekos home backup` wrote
        archive: PathBuf,
        /// Replace projects (and the console database) that already exist; each is moved to
        /// the home's trash first
        #[arg(long)]
        force: bool,
    },
}

#[derive(Subcommand)]
enum ConsoleCommands {
    /// Run the console in the foreground and start it again when it asks to be (what a login
    /// service runs)
    Run,
    /// Start the console in the background and wait until it answers
    Start,
    /// Stop the console
    Stop,
    /// Restart the console, e.g. after changing its settings
    Restart,
    /// Whether the console runs, where, and its process ids
    Status,
    /// Open the console in the browser; in local mode, signed in with a one-time code
    Open {
        /// Print the address (and code) instead of opening a browser
        #[arg(long)]
        no_browser: bool,
    },
    /// The end of the console's log
    Logs {
        #[arg(long, default_value_t = 100)]
        lines: usize,
    },
}

#[derive(Subcommand)]
enum McpCommands {
    /// Serve MCP over stdio (default), or over TCP (`--tcp`) or Streamable HTTP (`--http`)
    Serve {
        /// Workspace directory containing .ekos/ (default: current directory)
        #[arg(long, value_name = "DIR")]
        workspace: Option<PathBuf>,
        /// Serve over raw NDJSON/TCP at this address instead of stdio (RFC 0115),
        /// e.g. 127.0.0.1:7331 — bind loopback / a trusted network only
        #[arg(long, value_name = "ADDR")]
        tcp: Option<String>,
        /// Serve MCP over Streamable HTTP at this address instead of stdio (RFC 0143) — one
        /// `POST /mcp` endpoint, for clients that take a URL (VS Code / Copilot, Visual Studio).
        /// Bind loopback / a trusted network only
        #[arg(long, value_name = "ADDR", conflicts_with = "tcp")]
        http: Option<String>,
        /// Extra `Origin` value to allow on `--http` beyond loopback (repeatable)
        #[arg(long = "http-allow-origin", value_name = "ORIGIN", requires = "http")]
        http_allow_origin: Vec<String>,
        /// Require a bearer token on every `--tcp` / `--http` connection (RFC 0128). For `--tcp`
        /// the first message must be an `initialize` carrying a matching `params._meta.token`;
        /// for `--http` it is an `Authorization: Bearer <token>` header on every request. Read
        /// from this file (whitespace-trimmed) or, if absent, the `EKOS_MCP_TOKEN` env var. No
        /// effect on stdio.
        #[arg(long, value_name = "FILE", alias = "tcp-token-file")]
        token_file: Option<PathBuf>,
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
enum ReviewCommands {
    /// Every subject with item, decided, outdated and blocking counts and its sign-off state
    List {
        #[arg(long)]
        json: bool,
    },
    /// One subject's items, the facts shown, decisions and the digest a sign-off needs
    Show {
        /// scope, pii, conflicts, identity, write-behaviour or glossary
        subject: String,
        /// Only items without a current decision
        #[arg(long)]
        undecided: bool,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        json: bool,
    },
    /// Put tables in or out of scope (human-only; never via MCP)
    Scope {
        /// Table names
        #[arg(required = true)]
        tables: Vec<String>,
        #[arg(long = "in", conflicts_with = "out", required_unless_present = "out")]
        r#in: bool,
        #[arg(long)]
        out: bool,
        #[arg(long)]
        reason: Option<String>,
        /// Who is deciding (default: $USER)
        #[arg(long = "as")]
        by: Option<String>,
    },
    /// Confirm or reject PII candidates (human-only; never via MCP)
    Pii {
        /// `table.column` names
        #[arg(required = true)]
        columns: Vec<String>,
        #[arg(long, conflicts_with = "reject", required_unless_present = "reject")]
        confirm: bool,
        #[arg(long)]
        reject: bool,
        /// How a confirmed column is masked downstream: hash, redact (default) or none
        #[arg(long, requires = "confirm")]
        mask: Option<String>,
        #[arg(long)]
        reason: Option<String>,
        #[arg(long = "as")]
        by: Option<String>,
    },
    /// Confirm a table's write behaviour, or override fields with a reason (human-only)
    WriteBehaviour {
        /// Table names
        #[arg(required = true)]
        tables: Vec<String>,
        #[arg(long, conflicts_with = "set", required_unless_present = "set")]
        confirm: bool,
        /// `field=value` (update_class, delete_classes as a comma list, change_log true/false)
        #[arg(long = "set")]
        set: Vec<String>,
        #[arg(long)]
        reason: Option<String>,
        #[arg(long = "as")]
        by: Option<String>,
    },
}

#[derive(Subcommand)]
enum SignoffCommands {
    /// A subject's digest and whether its sign-off is valid, stale or missing
    Show {
        subject: String,
        #[arg(long)]
        json: bool,
    },
    /// Sign a subject off on the digest `show` printed (human-only; never via MCP)
    Approve {
        subject: String,
        /// The digest you reviewed; refused if the review changed since
        #[arg(long)]
        digest: String,
        #[arg(long)]
        note: Option<String>,
        #[arg(long = "as")]
        by: Option<String>,
    },
    /// Withdraw a subject's sign-off (human-only)
    Revoke {
        subject: String,
        #[arg(long)]
        reason: String,
        #[arg(long = "as")]
        by: Option<String>,
    },
}

#[derive(Subcommand)]
enum ConflictsCommands {
    /// The current conflicts: duplicate definitions, merge losses, label mismatches, code ↔ database
    List {
        /// Only one status: open, resolved or dismissed
        #[arg(long)]
        status: Option<String>,
        /// Only one type: duplicate_definition, merge_loss, label_mismatch, doc_vs_data, or (RFC 0211)
        /// model_table_missing, table_without_model, model_column_missing, mandatory_but_nullable,
        /// routine_text_differs
        #[arg(long = "type")]
        conflict_type: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// One conflict with every claim's file and line
    Show {
        /// The conflict's name (`subject.attribute`) or id
        target: String,
        #[arg(long)]
        json: bool,
    },
    /// Decide a conflict (human-only; never via MCP): which claim is right, or both are
    Resolve {
        /// The conflict's name (`subject.attribute`) or id
        target: String,
        /// The 1-based number of the right claim, as `show` lists them
        #[arg(
            long,
            conflicts_with = "both_valid",
            required_unless_present = "both_valid"
        )]
        pick: Option<usize>,
        /// Both claims are valid (needs --note)
        #[arg(long)]
        both_valid: bool,
        /// Who is deciding (default: $USER)
        #[arg(long = "as")]
        by: Option<String>,
        #[arg(long)]
        note: Option<String>,
    },
}

#[derive(Subcommand)]
enum SemanticsCommands {
    /// List the current concepts, enum meanings, constraints, gaps and rationale links — every one
    /// a hypothesis recovered from code traces, never a confirmed definition
    List {
        /// Only one kind: concept, enum, constraint, gap, conflict or rationale
        #[arg(long)]
        kind: Option<String>,
        /// Only one status: hypothesis, confirmed, rejected or needs_review
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Confirm a hypothesis as a correct business definition (human-only; never via MCP)
    Confirm {
        /// The items' names or ids — several are confirmed together, all or nothing
        #[arg(required = true)]
        targets: Vec<String>,
        /// Who is confirming (default: $USER)
        #[arg(long = "as")]
        by: Option<String>,
        #[arg(long)]
        note: Option<String>,
    },
    /// Reject a hypothesis (a reason is required)
    Reject {
        /// The items' names or ids — several are rejected together (one note), all or nothing
        #[arg(required = true)]
        targets: Vec<String>,
        #[arg(long = "as")]
        by: Option<String>,
        #[arg(long)]
        note: Option<String>,
    },
    /// Confirm with corrections: a better name, a description, or a code's label
    Edit {
        target: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        description: Option<String>,
        /// For a coded value (EnumMeaning)
        #[arg(long)]
        label: Option<String>,
        #[arg(long = "as")]
        by: Option<String>,
        #[arg(long)]
        note: Option<String>,
    },
    /// One item with its evidence (path:line) and links
    Show {
        /// The item's name or id
        target: String,
        #[arg(long)]
        json: bool,
    },
    /// The gap report: coded values no source explains, concepts nobody documented
    Gaps {
        #[arg(long)]
        json: bool,
    },
    /// A blank gold set for a domain expert — the workspace's tables and columns only, never EKOS's
    /// hypotheses — to be filled in before anyone looks at EKOS's output
    GoldTemplate {
        /// Only these tables (default: all)
        #[arg(long, value_delimiter = ',')]
        tables: Vec<String>,
        /// Write here instead of stdout
        #[arg(long)]
        out: Option<std::path::PathBuf>,
    },
    /// Score the recovered semantics against an expert-written gold set (YAML)
    Eval {
        /// The gold-set file
        #[arg(long)]
        gold: std::path::PathBuf,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum ExportCommands {
    /// A draft LinkML schema of the business-semantics items (RFC 0170). Hypotheses are left out
    /// unless asked for: the default is `--status confirmed`
    Linkml {
        #[arg(long, value_enum, default_value = "confirmed")]
        status: crate::commands::export::StatusFilter,
        /// Write here instead of stdout
        #[arg(long)]
        out: Option<std::path::PathBuf>,
        /// Schema name (default: the workspace directory's name)
        #[arg(long)]
        name: Option<String>,
        /// Print the schema as JSON instead of YAML
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum ImportCommands {
    /// Turn edits to an exported LinkML schema into review decisions — renames, descriptions,
    /// labels, `ekos_status: confirmed|rejected`. Human-only; all or nothing
    Linkml {
        /// The edited schema (from `ekos export linkml`)
        file: std::path::PathBuf,
        /// Show the decisions without writing them
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        json: bool,
        /// Who is reviewing (default: $USER)
        #[arg(long = "as")]
        by: Option<String>,
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
enum TreasuryCommands {
    /// Score committed `TreasuryPayment` objects against `GovernanceProposal` objects and write
    /// each candidate as an `unconfirmed` `AuthorizedBy` relationship, then list the payments with
    /// no candidate approval. Never confirms anything — a person reviews with `ekos treasury
    /// confirm|reject`.
    Scan,
    /// Confirm that a payment was authorized by a proposal (an `AuthorizedBy` candidate).
    /// Human-only: no MCP tool can do this.
    Confirm {
        /// The `AuthorizedBy` relationship id
        relationship_id: String,
        #[arg(long = "as")]
        by: Option<String>,
        #[arg(long)]
        note: Option<String>,
    },
    /// Reject an `AuthorizedBy` candidate (a reason is required). Human-only.
    Reject {
        /// The `AuthorizedBy` relationship id
        relationship_id: String,
        #[arg(long = "as")]
        by: Option<String>,
        #[arg(long)]
        note: Option<String>,
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
enum LlmCommands {
    /// Each task's provider, models (fallbacks after the first), wire format and price
    Routes {
        #[arg(long)]
        json: bool,
    },
    /// Calls that reached a model (not disk-cache hits): tokens and cost by task and model
    Usage {
        /// Only calls at or after this time (RFC 3339 or YYYY-MM-DD)
        #[arg(long)]
        since: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Send one tiny request per model (a few tokens each, uncached) and report which answer
    Probe {
        /// The task whose route (provider, host, key) to probe through; default `[llm]`
        #[arg(long)]
        task: Option<String>,
        /// Models to try, comma-separated; default every model any route names
        #[arg(long, value_delimiter = ',')]
        models: Vec<String>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum LedgerCommands {
    /// Show ledger entry count and object count
    Status {
        /// Also report per-component storage sizes (RFC 0015)
        #[arg(long)]
        storage: bool,
        /// Emit a single machine-readable JSON object instead of the text report (RFC 0127 R2)
        #[arg(long)]
        json: bool,
    },
    /// Migrate the ledger: v2 compact format (RFC 0015), or --v3 for the
    /// fact engine (RFC 0016)
    Migrate {
        /// Migrate to the RFC 0016 fact-segment engine
        #[arg(long)]
        v3: bool,
    },
    /// Verify every sealed segment's integrity and self-heal any torn active-segment tail or
    /// stale index runs (RFC 0105 Phase 2). Fact engine (RFC 0016) only.
    Repair,
    /// Cumulative object/relationship counts bucketed over time — the growth timeline behind the
    /// web console dashboard (RFC 0129 R6). Output is always JSON.
    Timeline {
        /// Accepted for parity with `status --json` / `doctor --json`; output is JSON regardless
        #[arg(long)]
        json: bool,
        /// Bucket granularity: `day` (default), `week`, or `month`
        #[arg(long, default_value = "day")]
        bucket: String,
        /// Only include buckets at or after this RFC 3339 timestamp
        #[arg(long)]
        since: Option<String>,
    },
    /// Write history of one object/relationship with per-write provenance — which run and
    /// pipeline stage produced each version, and the artifact it came from (RFC 0135 Part B).
    Audit {
        /// Object or relationship id (UUID) — from `ekos query`, `ekos_search`, or a graph node
        id: String,
        /// Emit JSON instead of the text table
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum ConfigCommands {
    /// Parse `ekos.toml` and report syntax errors plus `[observe]` mistakes (RFC 0130 R7)
    Validate {
        /// Emit one machine-readable JSON object instead of the text summary
        #[arg(long)]
        json: bool,
        /// Validate this file instead of the workspace's `ekos.toml` — for editors checking
        /// unsaved text. `[observe]` paths are still resolved against the workspace root.
        #[arg(long)]
        file: Option<PathBuf>,
    },
    /// Count what `ekos build` would observe under `[observe] paths` / `ignore-patterns`, without
    /// reading or compiling anything (RFC 0130 R8)
    PreviewScan {
        /// Emit one machine-readable JSON object instead of the text summary
        #[arg(long)]
        json: bool,
        /// Stop the walk after this many files and set `truncated: true`
        #[arg(long, default_value_t = 200_000)]
        max_files: usize,
    },
    /// The JSON schema of `ekos.toml`: every section and setting, its type, default and
    /// description — what the web console builds its settings forms from (RFC 0186)
    Schema {
        /// Print JSON (the only format)
        #[arg(long)]
        json: bool,
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
    Find {
        query: String,
        /// Print the compiled query plan (RFC 0124) — how the text is classified
        /// and routed — before the results.
        #[arg(long)]
        explain: bool,
        /// Retrieval mode (RFC 0125): `lexical` (BM25, default), `vector`
        /// (semantic only — needs `[embeddings]`), or `hybrid` (both, fused).
        #[arg(long, default_value = "lexical")]
        mode: String,
    },
    /// BFS neighbourhood graph up to --depth hops
    Neighbourhood {
        id: String,
        #[arg(long, default_value = "1")]
        depth: u32,
    },
}

/// Resolve which `ekos.toml` to load. Precedence: explicit `--config`, then `EKOS_CONFIG`, then
/// — for `ekos mcp serve` only — the `--workspace <dir>` flag's or `EKOS_WORKSPACE`'s `ekos.toml`
/// (agent hosts spawn the server from an arbitrary cwd, so the workspace's own config must follow
/// it — otherwise a `[storage.partition]`/`[storage.distributed]` workspace opens with the wrong
/// routing config and every tool call fails), then `./ekos.toml`.
fn resolve_config_path(
    explicit: Option<PathBuf>,
    env_config: Option<PathBuf>,
    is_mcp: bool,
    mcp_workspace: Option<&std::path::Path>,
    env_workspace: Option<&std::path::Path>,
) -> PathBuf {
    explicit
        .or(env_config)
        .or_else(|| {
            is_mcp
                .then(|| mcp_workspace.or(env_workspace).map(|w| w.join("ekos.toml")))
                .flatten()
        })
        .unwrap_or_else(|| PathBuf::from("ekos.toml"))
}

/// RFC 0181: load the read scope the process was started with into the config, from where every
/// store the command opens picks it up.
///
/// Only commands that read knowledge solely through the store accept one. Any other command also
/// reads files beside the ledger (the freshness report, conflict counts, the source tree itself)
/// which no store wrapper filters, so a scope there would be a promise the command cannot keep.
fn with_read_scope(
    mut config: ekos_compiler_core::EkosConfig,
    command: &Commands,
    file: Option<PathBuf>,
    invoked_from: &std::path::Path,
) -> Result<ekos_compiler_core::EkosConfig> {
    let Some(file) = file else {
        return Ok(config);
    };
    if !matches!(
        command,
        Commands::Mcp { .. } | Commands::Ekl { .. } | Commands::Query { .. }
    ) {
        bail!(
            "--read-scope applies to `ekos mcp serve`, `ekos ekl` and `ekos query` only: other \
             commands read more than the ledger, and a scope could not be enforced on them"
        );
    }
    let path = invoked_from.join(file);
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("reading the read scope {}", path.display()))?;
    // Parsed here so a bad scope stops the command before anything is read.
    ekos_ledger::scope::ReadScope::from_json(&text)
        .map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
    config.read_scope = Some(text);
    Ok(config)
}

/// RFC 0178: what cannot be combined with `--project`. Each would point the command at a second
/// config or workspace, or write a config into the source tree.
fn refuse_with_project(
    command: &Commands,
    explicit_config: bool,
    mcp_workspace: &Option<PathBuf>,
) -> Result<()> {
    if explicit_config {
        bail!("--config cannot be combined with --project: a project has its own ekos.toml");
    }
    if mcp_workspace.is_some() {
        bail!("--workspace cannot be combined with --project");
    }
    if matches!(command, Commands::Init { .. }) {
        bail!(
            "`ekos init` writes ekos.toml into the current directory. A project's config is \
             created by `ekos project add <name> --source <dir>`"
        );
    }
    Ok(())
}

fn dispatch_source(
    subcommand: SourceCommands,
    config: &ekos_compiler_core::EkosConfig,
    cwd: &std::path::Path,
    config_path: &std::path::Path,
    invoked_from: &std::path::Path,
) -> Result<()> {
    use crate::commands::source;
    match subcommand {
        SourceCommands::Add { name, path, kind } => {
            let kind = kind.as_deref().map(source::parse_kind).transpose()?;
            let objects =
                crate::commands::store::open_store_read_only(config, cwd)?.object_count()?;
            let path = invoked_from.join(path);
            source::add(config_path, config, cwd, objects, &name, &path, kind)
        }
        SourceCommands::List { json } => source::list(config, cwd, json),
        SourceCommands::Remove { name } => source::remove(config_path, &name),
    }
}

fn dispatch_project(
    subcommand: ProjectCommands,
    home_dir: &std::path::Path,
    tenant: Option<String>,
) -> Result<()> {
    use crate::commands::project;
    let tenant = tenant
        .or_else(|| std::env::var("EKOS_TENANT").ok())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| crate::home::DEFAULT_TENANT.to_string());
    let at = |name: &str| crate::home::location(home_dir, &tenant, name);
    match subcommand {
        ProjectCommands::Add {
            name,
            source,
            detect,
        } => {
            let sources = source
                .iter()
                .map(|s| crate::commands::source::parse_spec(s))
                .collect::<Result<Vec<_>>>()?;
            project::add(&at(&name)?, &sources, detect)
        }
        ProjectCommands::List { json } => project::list(home_dir, json),
        ProjectCommands::Show { name, json } => project::show(&at(&name)?, json),
        ProjectCommands::Remove { name, purge, yes } => {
            project::remove(&at(&name)?, purge, yes, chrono::Utc::now()).map(|_| ())
        }
    }
}

/// True for subcommands whose stdout is a single machine-readable document (JSON / NDJSON) that a
/// program parses — their logs must go to stderr so nothing interleaves. Kept in sync with the
/// `--json` flags on these subcommands and with `graph export` (always machine output).
fn emits_machine_output(command: &Commands) -> bool {
    match command {
        Commands::Status { json, .. }
        | Commands::Doctor { json }
        | Commands::Ekl { json, .. }
        | Commands::Coverage { json, .. }
        | Commands::Freshness { json, .. } => *json,
        Commands::Llm { subcommand } => match subcommand {
            LlmCommands::Routes { json }
            | LlmCommands::Usage { json, .. }
            | LlmCommands::Probe { json, .. } => *json,
        },
        Commands::Graph {
            subcommand: GraphCommands::Export { .. },
        } => true,
        Commands::Ledger { subcommand } => matches!(
            subcommand,
            LedgerCommands::Status { json: true, .. }
                | LedgerCommands::Timeline { .. }
                | LedgerCommands::Audit { json: true, .. }
        ),
        Commands::Config { subcommand } => match subcommand {
            ConfigCommands::Validate { json, .. } => *json,
            ConfigCommands::PreviewScan { json, .. } => *json,
            ConfigCommands::Schema { .. } => true,
        },
        Commands::Eval { subcommand } => match subcommand {
            EvalCommands::Run { json, .. } => *json,
            EvalCommands::History { json, .. } => *json,
            EvalCommands::Regrade { json, .. } => *json,
        },
        _ => false,
    }
}

/// Parses the arguments against the built-in commands plus every extension's (RFC 0191). On a
/// usage error or `--help` it prints and exits, as `Cli::parse` does.
fn parse_cli<I, T>(extensions: &Extensions, args: I) -> Result<Cli>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let root = extensions.extend_cli(cli_command())?;
    let matches = root.get_matches_from(args);
    if let Some((name, sub)) = matches.subcommand()
        && extensions.command_owner(name).is_some()
    {
        return Ok(Cli {
            config: matches.get_one::<PathBuf>("config").cloned(),
            home: matches.get_one::<PathBuf>("home").cloned(),
            project: matches.get_one::<String>("project").cloned(),
            tenant: matches.get_one::<String>("tenant").cloned(),
            read_scope: matches.get_one::<PathBuf>("read_scope").cloned(),
            command: Commands::Extension {
                name: name.to_string(),
                matches: sub.clone(),
            },
        });
    }
    Ok(Cli::from_arg_matches(&matches).unwrap_or_else(|e| e.exit()))
}

/// The whole `ekos` CLI: parses arguments and dispatches every command. The public binary calls
/// it with [`Extensions::none`]; an RFC 0149 build passes its own extensions, which then take
/// part in `build`/`recover`/`commit`, the MCP server, `architecture investigate` and the
/// cluster compile worker.
pub async fn main_with(extensions: Extensions) -> Result<()> {
    main_with_args(extensions, std::env::args_os()).await
}

/// [`main_with`] with explicit arguments (the first is the program name), for tests.
pub async fn main_with_args<I, T>(extensions: Extensions, args: I) -> Result<()>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let cli = parse_cli(&extensions, args)?;

    let env_workspace = std::env::var_os("EKOS_WORKSPACE").map(PathBuf::from);
    let mcp_workspace_flag: Option<PathBuf> = match &cli.command {
        Commands::Mcp {
            subcommand: McpCommands::Serve { workspace, .. },
        } => workspace.clone(),
        _ => None,
    };
    // RFC 0178: the home, and the project a command selected in it, if any.
    let home_dir = crate::home::resolve_home(
        cli.home,
        std::env::var_os("EKOS_HOME").map(PathBuf::from),
        crate::home::user_home_from_env(),
    );
    let tenant_flag = cli.tenant;
    let command = match cli.command {
        Commands::Config {
            subcommand: ConfigCommands::Schema { .. },
        } => {
            let schema = schemars::schema_for!(ekos_compiler_core::EkosConfig);
            println!("{}", serde_json::to_string(&schema)?);
            return Ok(());
        }
        Commands::Project { subcommand } => {
            if cli.project.is_some() {
                bail!("`ekos project` takes the project name as an argument, not --project");
            }
            crate::commands::init_logging(&ekos_compiler_core::EkosConfig::default());
            return dispatch_project(subcommand, &home_dir?, tenant_flag);
        }
        Commands::Extensions { json: _ } => {
            let mut root = extensions.extend_cli(cli_command())?;
            root.build();
            let cat =
                crate::commands::catalogue::catalogue_with(&root, &extensions.command_classes());
            let out = crate::commands::extension_pages::describe(&extensions, &cat)?;
            println!("{}", serde_json::to_string(&out)?);
            return Ok(());
        }
        Commands::Catalogue { json: _ } => {
            let mut root = extensions.extend_cli(cli_command())?;
            root.build();
            let out = serde_json::json!({
                "version": env!("CARGO_PKG_VERSION"),
                "commands": crate::commands::catalogue::catalogue_with(
                    &root,
                    &extensions.command_classes(),
                ),
            });
            println!("{}", serde_json::to_string(&out)?);
            return Ok(());
        }
        Commands::Setup {
            answers,
            dry_run,
            no_browser,
            port,
        } => {
            return run_setup(home_dir?, answers, dry_run, no_browser, port).await;
        }
        Commands::Uninstall {
            purge,
            yes,
            ui,
            no_browser,
            port,
            detach,
        } => {
            if ui {
                let home = home_dir?;
                if detach {
                    return crate::commands::console::detach_uninstall_page(&home, port);
                }
                let machine = crate::commands::setup::Machine::detect(&std::env::current_exe()?)?;
                let page = crate::commands::uninstall_server::UninstallPage::new(
                    crate::commands::setup_server::new_token(),
                    home,
                    machine,
                    std::sync::Arc::new(crate::commands::setup::RealEffects),
                );
                return crate::commands::uninstall_server::serve(page, port, !no_browser).await;
            }
            if purge && !yes {
                bail!("--purge deletes every project in the home; add --yes to confirm");
            }
            let home = home_dir?;
            let machine = crate::commands::setup::Machine::detect(&std::env::current_exe()?)?;
            let steps = crate::commands::setup::uninstall(
                &home,
                purge,
                &machine,
                &crate::commands::setup::RealEffects,
            )?;
            print_steps(&steps);
            return Ok(());
        }
        Commands::Console { subcommand } => {
            if cli.project.is_some() {
                bail!("`ekos console` serves the whole home; --project does not apply");
            }
            let home = home_dir?;
            use crate::commands::console;
            return match subcommand {
                ConsoleCommands::Run => console::run(&home),
                ConsoleCommands::Start => console::start(&home),
                ConsoleCommands::Stop => console::stop(&home),
                ConsoleCommands::Restart => console::restart(&home),
                ConsoleCommands::Status => console::status(&home),
                ConsoleCommands::Open { no_browser } => console::open(&home, no_browser),
                ConsoleCommands::Logs { lines } => console::logs(&home, lines),
            };
        }
        Commands::Home { subcommand } => {
            if cli.project.is_some() {
                bail!(
                    "`ekos home` works on the whole home; --tenant narrows it, --project does not"
                );
            }
            crate::commands::init_logging(&ekos_compiler_core::EkosConfig::default());
            let home = home_dir?;
            let invoked_from = std::env::current_dir()?;
            let tenant = tenant_flag.as_deref();
            return match subcommand {
                HomeCommands::Backup { out } => crate::commands::home_backup::run_backup(
                    &home,
                    tenant,
                    out.map(|o| invoked_from.join(o)),
                ),
                HomeCommands::Restore { archive, force } => {
                    crate::commands::home_backup::run_restore(
                        &home,
                        &invoked_from.join(archive),
                        tenant,
                        force,
                    )
                }
            };
        }
        other => other,
    };
    let selected = match (cli.project, std::env::var("EKOS_PROJECT").ok()) {
        (None, None) if tenant_flag.is_none() => None,
        (project, env_project) => crate::home::select_project(
            &home_dir?,
            project,
            env_project,
            tenant_flag,
            std::env::var("EKOS_TENANT").ok(),
        )?,
    };

    // Where the command was typed: a path argument is relative to this, even after a project
    // moves the process into its sources.
    let invoked_from = std::env::current_dir()?;
    let (config, cwd, config_path, env_workspace) = match selected {
        Some(location) => {
            refuse_with_project(&command, cli.config.is_some(), &mcp_workspace_flag)?;
            let (config, root) = crate::home::load_project(&location)?;
            // Every relative path a command or the config names resolves in the sources.
            std::env::set_current_dir(&root)
                .with_context(|| format!("entering {}", root.display()))?;
            (config, root, location.config_path(), None)
        }
        None => {
            let config_path = resolve_config_path(
                cli.config,
                std::env::var_os("EKOS_CONFIG").map(PathBuf::from),
                matches!(command, Commands::Mcp { .. }),
                mcp_workspace_flag.as_deref(),
                env_workspace.as_deref(),
            );
            let config = ekos_compiler_core::EkosConfig::from_file_or_default(&config_path);
            (config, std::env::current_dir()?, config_path, env_workspace)
        }
    };

    let config = with_read_scope(
        config,
        &command,
        cli.read_scope
            .or_else(|| std::env::var_os("EKOS_READ_SCOPE").map(PathBuf::from)),
        &invoked_from,
    )?;

    // The MCP server owns stdout for protocol frames; the machine-readable `--json` / graph-export
    // commands own stdout for the document a consumer parses. Both send logs to stderr so nothing
    // interleaves with what a program reads (RFC 0127 R1/R2, RFC 0129 R5/R6).
    // RFC 0220: a pipeline command writes a structured run log.
    let run_log = pipeline_command_name(&command)
        .and_then(|name| crate::run_log::for_command(name, &config, &cwd));
    if matches!(command, Commands::Mcp { .. }) || emits_machine_output(&command) {
        crate::commands::init_logging_stderr(&config);
    } else {
        crate::commands::init_logging_with(&config, run_log.as_ref());
    }

    let result = match command {
        Commands::Init {
            detect,
            dry_run,
            force,
        } => crate::commands::init::run_with_options(
            &config,
            &cwd,
            crate::commands::init::InitOptions {
                detect,
                dry_run,
                force,
            },
        ),
        Commands::Coverage { json, strict, all } => {
            crate::commands::coverage::run(&config, &cwd, json, strict, all)
        }
        Commands::Freshness { json, limit } => {
            crate::commands::freshness::run(&config, &cwd, json, limit)
        }
        Commands::Llm { subcommand } => match subcommand {
            LlmCommands::Routes { json } => crate::commands::llm::routes(&config, &cwd, json),
            LlmCommands::Usage { since, json } => {
                crate::commands::llm::usage(&config, &cwd, since.as_deref(), json)
            }
            LlmCommands::Probe { task, models, json } => {
                crate::commands::llm::probe(&config, &cwd, task.as_deref(), &models, json).await
            }
        },
        Commands::Build { source } => {
            crate::commands::build::run_selected(&config, &cwd, &extensions, source.as_deref())
                .await
        }
        Commands::Source { subcommand } => {
            dispatch_source(subcommand, &config, &cwd, &config_path, &invoked_from)
        }
        Commands::Recover { parallel } => {
            crate::commands::recover::run_with(&config, &cwd, parallel, &extensions).await
        }
        Commands::Resolve { force } => crate::commands::resolve::run(&config, &cwd, force),
        Commands::Conflicts { subcommand } => dispatch_conflicts(subcommand, &config, &cwd),
        Commands::Review { subcommand } => dispatch_review(subcommand, &config, &cwd),
        Commands::Signoff { subcommand } => dispatch_signoff(subcommand, &config, &cwd),
        Commands::Semantics { subcommand } => dispatch_semantics(subcommand, &config, &cwd),
        Commands::Export { subcommand } => dispatch_export(subcommand, &config, &cwd),
        Commands::Import { subcommand } => dispatch_import(subcommand, &config, &cwd),
        Commands::Identity { subcommand } => dispatch_identity(subcommand, &config, &cwd),
        Commands::Treasury { subcommand } => dispatch_treasury(subcommand, &config, &cwd),
        Commands::Compile => crate::commands::compile::run(&config, &cwd).await,
        Commands::Commit { yes } => {
            crate::commands::commit::run_with(&config, &cwd, yes, &extensions).await
        }
        Commands::Ledger { subcommand } => dispatch_ledger(subcommand, &config, &cwd),
        Commands::Clean => crate::commands::clean::run(&config, &cwd),
        Commands::Doctor { json } => {
            crate::commands::doctor::run(&config, &cwd, &config_path, json)
        }
        Commands::Config { subcommand } => match subcommand {
            ConfigCommands::Validate { json, file } => {
                let target = file.as_deref().unwrap_or(&config_path);
                crate::commands::config::validate(&config, &cwd, target, json)
            }
            ConfigCommands::PreviewScan { json, max_files } => {
                crate::commands::config::preview_scan(&config, &cwd, max_files, json)
            }
            ConfigCommands::Schema { .. } => unreachable!("handled before a config is loaded"),
        },
        Commands::Status { storage, json } => {
            crate::commands::ledger::status(&config, &cwd, storage, json)
        }
        Commands::Query { subcommand } => dispatch_query(subcommand, &config, &cwd),
        Commands::Ask {
            question,
            json,
            stream,
            session,
            classic,
            explain,
        } => {
            crate::commands::ask::run(
                &config,
                &cwd,
                &question,
                crate::commands::ask::AskOpts {
                    json,
                    stream,
                    session: session.as_deref(),
                    classic,
                    explain,
                },
            )
            .await
        }
        Commands::ClickHouse { subcommand } => {
            dispatch_click_house(subcommand, &config, &cwd).await
        }
        Commands::Ekl { query, json } => crate::commands::ekl::run(&config, &cwd, &query, json),
        Commands::Diff { from, to } => crate::commands::diff::run(&config, &cwd, from, to),
        Commands::Branch { subcommand } => dispatch_branch(subcommand, &config, &cwd),
        Commands::Mcp { subcommand } => match subcommand {
            McpCommands::Serve {
                workspace,
                tcp,
                http,
                http_allow_origin,
                token_file,
            } => {
                let workspace = workspace.or(env_workspace).unwrap_or_else(|| cwd.clone());
                // RFC 0128 §1.1: `--token-file` wins over `EKOS_MCP_TOKEN`; neither → no auth.
                let token = match token_file {
                    Some(path) => Some(
                        tokio::fs::read_to_string(&path)
                            .await
                            .with_context(|| format!("reading --token-file {}", path.display()))?
                            .trim()
                            .to_string(),
                    ),
                    None => std::env::var("EKOS_MCP_TOKEN")
                        .ok()
                        .filter(|t| !t.is_empty()),
                };
                crate::commands::mcp::run_with(
                    &config,
                    &workspace,
                    tcp.as_deref(),
                    http.as_deref(),
                    &http_allow_origin,
                    token,
                    &extensions,
                )
            }
        },
        Commands::Artifact { subcommand } => dispatch_artifact(subcommand, &config, &cwd),
        Commands::Coordinator { subcommand } => dispatch_coordinator(subcommand).await,
        Commands::CompileWorker { subcommand } => {
            dispatch_compile_worker(subcommand, &extensions).await
        }
        Commands::QueryWorker { subcommand } => dispatch_query_worker(subcommand).await,
        Commands::Marketing { subcommand } => dispatch_marketing(subcommand, &config, &cwd).await,
        Commands::Docs { subcommand } => dispatch_docs(subcommand, &config, &cwd).await,
        Commands::Graph { subcommand } => dispatch_graph(subcommand, &config, &cwd),
        Commands::Dbt { subcommand } => dispatch_dbt(subcommand, &config, &cwd).await,
        Commands::Architecture { subcommand } => {
            dispatch_architecture(subcommand, &config, &cwd, &extensions).await
        }
        Commands::Simulate {
            scenario,
            rounds,
            ledger,
            seed,
        } => crate::commands::simulate::run(&config, &cwd, &scenario, rounds, ledger, seed),
        Commands::Replay {
            scenario,
            round,
            ledger,
        } => crate::commands::replay::run(&config, &cwd, &scenario, round, ledger),
        Commands::Migrate { subcommand } => dispatch_migrate(subcommand, &config, &cwd).await,
        Commands::Session { subcommand } => dispatch_session(subcommand, &config, &cwd),
        Commands::Eval { subcommand } => dispatch_eval(subcommand, &config, &cwd).await,
        Commands::Extension { name, matches } => {
            let owner = extensions
                .command_owner(&name)
                .context("an extension command without its extension")?;
            let ctx = crate::extension::CommandContext {
                config: &config,
                cwd: &cwd,
                config_path: &config_path,
                invoked_from: &invoked_from,
                extensions: &extensions,
            };
            owner.run_command(&name, &matches, &ctx).await
        }
        Commands::Project { .. }
        | Commands::Home { .. }
        | Commands::Console { .. }
        | Commands::Setup { .. }
        | Commands::Uninstall { .. }
        | Commands::Catalogue { .. }
        | Commands::Extensions { .. } => {
            unreachable!("handled before a config is loaded")
        }
    };
    crate::run_log::finish(run_log.as_ref(), &result);
    result
}

/// The run-log name of a pipeline command (RFC 0220), `None` for every other command.
fn pipeline_command_name(command: &Commands) -> Option<&'static str> {
    match command {
        Commands::Build { .. } => Some("build"),
        Commands::Recover { .. } => Some("recover"),
        Commands::Resolve { .. } => Some("resolve"),
        Commands::Compile => Some("compile"),
        Commands::Commit { .. } => Some("commit"),
        _ => None,
    }
}

fn print_steps(steps: &[crate::commands::setup::Step]) {
    for s in steps {
        let mark = if s.ok { "ok  " } else { "FAIL" };
        if s.message.is_empty() {
            println!("{mark}  {}", s.action);
        } else {
            println!("{mark}  {} — {}", s.action, s.message);
        }
    }
}

/// `ekos setup`: the wizard, or `--answers` applied without one (RFC 0185 §3).
async fn run_setup(
    home: std::path::PathBuf,
    answers: Option<std::path::PathBuf>,
    dry_run: bool,
    no_browser: bool,
    port: u16,
) -> Result<()> {
    use crate::commands::{setup, setup_server};
    let machine = setup::Machine::detect(&std::env::current_exe()?)?;
    let Some(file) = answers else {
        let wizard = setup_server::Wizard::new(setup_server::new_token(), machine, home);
        return setup_server::serve(wizard, port, !no_browser).await;
    };
    let text = tokio::fs::read_to_string(&file)
        .await
        .with_context(|| format!("reading {}", file.display()))?;
    let mut a: setup::Answers =
        toml::from_str(&text).with_context(|| format!("reading {}", file.display()))?;
    if a.home.trim().is_empty() {
        a.home = home.display().to_string();
    }
    let actions = setup::plan(&a, &machine)?;
    if dry_run {
        for action in &actions {
            println!("{}", action.describe());
        }
        return Ok(());
    }
    let steps = tokio::task::spawn_blocking(move || {
        setup::apply(&actions, &a, &machine, &setup::RealEffects)
    })
    .await??;
    print_steps(&steps);
    if steps.iter().any(|s| !s.ok) {
        bail!("setup stopped at a step that failed");
    }
    Ok(())
}

/// `ekos conflicts …`
fn dispatch_review(
    subcommand: ReviewCommands,
    config: &ekos_compiler_core::EkosConfig,
    cwd: &std::path::Path,
) -> Result<()> {
    use crate::commands::review;
    use ekos_semantic::review::Subject;
    use serde_json::json;
    match subcommand {
        ReviewCommands::List { json } => review::list(config, cwd, json),
        ReviewCommands::Show {
            subject,
            undecided,
            limit,
            json,
        } => review::show(config, cwd, &subject, undecided, limit, json),
        ReviewCommands::Scope {
            tables,
            r#in,
            reason,
            by,
            ..
        } => {
            let d = json!({ "scope": if r#in { "in" } else { "out" } });
            review::decide(config, cwd, Subject::Scope, &tables, d, reason, by)
        }
        ReviewCommands::Pii {
            columns,
            confirm,
            mask,
            reason,
            by,
            ..
        } => {
            let d = if confirm {
                json!({ "pii": "confirm", "mask": mask })
            } else {
                json!({ "pii": "reject" })
            };
            review::decide(config, cwd, Subject::Pii, &columns, d, reason, by)
        }
        ReviewCommands::WriteBehaviour {
            tables,
            confirm,
            set,
            reason,
            by,
        } => {
            let d = if confirm {
                json!({ "behaviour": "confirm" })
            } else {
                json!({ "behaviour": "override", "fields": overrides(&set)? })
            };
            review::decide(config, cwd, Subject::WriteBehaviour, &tables, d, reason, by)
        }
    }
}

/// `--set field=value` pairs → the override's fields.
fn overrides(set: &[String]) -> Result<serde_json::Map<String, serde_json::Value>> {
    use serde_json::json;
    let mut out = serde_json::Map::new();
    for s in set {
        let (k, v) = s
            .split_once('=')
            .ok_or_else(|| anyhow::anyhow!("--set takes field=value, got `{s}`"))?;
        let value = match k {
            "change_log" => json!(
                v.parse::<bool>()
                    .map_err(|_| anyhow::anyhow!("change_log is true or false"))?
            ),
            "delete_classes" => json!(
                v.split(',')
                    .map(str::trim)
                    .filter(|x| !x.is_empty())
                    .collect::<Vec<_>>()
            ),
            _ => json!(v),
        };
        out.insert(k.to_string(), value);
    }
    Ok(out)
}

fn dispatch_signoff(
    subcommand: SignoffCommands,
    config: &ekos_compiler_core::EkosConfig,
    cwd: &std::path::Path,
) -> Result<()> {
    use crate::commands::review;
    match subcommand {
        SignoffCommands::Show { subject, json } => {
            review::signoff_show(config, cwd, &subject, json)
        }
        SignoffCommands::Approve {
            subject,
            digest,
            note,
            by,
        } => review::approve(config, cwd, &subject, &digest, note, by),
        SignoffCommands::Revoke {
            subject,
            reason,
            by,
        } => review::revoke(config, cwd, &subject, &reason, by),
    }
}

fn dispatch_conflicts(
    subcommand: ConflictsCommands,
    config: &ekos_compiler_core::EkosConfig,
    cwd: &std::path::Path,
) -> Result<()> {
    match subcommand {
        ConflictsCommands::List {
            status,
            conflict_type,
            json,
        } => crate::commands::conflicts::list(
            config,
            cwd,
            status.as_deref(),
            conflict_type.as_deref(),
            json,
        ),
        ConflictsCommands::Show { target, json } => {
            crate::commands::conflicts::show(config, cwd, &target, json)
        }
        ConflictsCommands::Resolve {
            target,
            pick,
            both_valid,
            by,
            note,
        } => {
            let decision = match (pick, both_valid) {
                (Some(n), false) => ekos_semantic::conflicts::Resolution::Pick(n),
                _ => ekos_semantic::conflicts::Resolution::BothValid,
            };
            crate::commands::conflicts::resolve(config, cwd, &target, decision, by, note)
        }
    }
}

/// `ekos semantics …`
fn dispatch_semantics(
    subcommand: SemanticsCommands,
    config: &ekos_compiler_core::EkosConfig,
    cwd: &std::path::Path,
) -> Result<()> {
    match subcommand {
        SemanticsCommands::List { kind, status, json } => {
            crate::commands::semantics::list(config, cwd, kind.as_deref(), status.as_deref(), json)
        }
        SemanticsCommands::Confirm { targets, by, note } => {
            crate::commands::semantics::review_many(
                config,
                cwd,
                &targets,
                ekos_semantic::semantics_review::Decision::Confirm,
                by,
                note,
            )
        }
        SemanticsCommands::Reject { targets, by, note } => crate::commands::semantics::review_many(
            config,
            cwd,
            &targets,
            ekos_semantic::semantics_review::Decision::Reject,
            by,
            note,
        ),
        SemanticsCommands::Edit {
            target,
            name,
            description,
            label,
            by,
            note,
        } => crate::commands::semantics::review(
            config,
            cwd,
            &target,
            ekos_semantic::semantics_review::Decision::Edit {
                name,
                description,
                label,
            },
            by,
            note,
        ),
        SemanticsCommands::Show { target, json } => {
            crate::commands::semantics::show(config, cwd, &target, json)
        }
        SemanticsCommands::Gaps { json } => crate::commands::semantics::gaps(config, cwd, json),
        SemanticsCommands::GoldTemplate { tables, out } => {
            crate::commands::semantics::gold_template(config, cwd, &tables, out.as_deref())
        }
        SemanticsCommands::Eval { gold, json } => {
            crate::commands::semantics::eval(config, cwd, &gold, json)
        }
    }
}

/// `ekos export …`
fn dispatch_export(
    subcommand: ExportCommands,
    config: &ekos_compiler_core::EkosConfig,
    cwd: &std::path::Path,
) -> Result<()> {
    match subcommand {
        ExportCommands::Linkml {
            status,
            out,
            name,
            json,
        } => crate::commands::export::linkml(
            config,
            cwd,
            &crate::commands::export::LinkmlOptions {
                status,
                out,
                name,
                json,
            },
        ),
    }
}

/// `ekos import …`
fn dispatch_import(
    subcommand: ImportCommands,
    config: &ekos_compiler_core::EkosConfig,
    cwd: &std::path::Path,
) -> Result<()> {
    match subcommand {
        ImportCommands::Linkml {
            file,
            dry_run,
            json,
            by,
        } => crate::commands::import::linkml(
            config,
            cwd,
            &crate::commands::import::ImportOptions {
                file: &file,
                dry_run,
                json,
                by,
            },
        ),
    }
}

/// `ekos identity …`
fn dispatch_identity(
    subcommand: IdentityCommands,
    config: &ekos_compiler_core::EkosConfig,
    cwd: &std::path::Path,
) -> Result<()> {
    match subcommand {
        IdentityCommands::Scan => crate::commands::identity::scan(config, cwd),
    }
}

/// `ekos treasury …`
fn dispatch_treasury(
    subcommand: TreasuryCommands,
    config: &ekos_compiler_core::EkosConfig,
    cwd: &std::path::Path,
) -> Result<()> {
    match subcommand {
        TreasuryCommands::Scan => crate::commands::treasury::scan(config, cwd),
        TreasuryCommands::Confirm {
            relationship_id,
            by,
            note,
        } => crate::commands::treasury::review_authorization(
            config,
            cwd,
            &relationship_id,
            true,
            by,
            note,
        ),
        TreasuryCommands::Reject {
            relationship_id,
            by,
            note,
        } => crate::commands::treasury::review_authorization(
            config,
            cwd,
            &relationship_id,
            false,
            by,
            note,
        ),
    }
}

/// `ekos ledger …`
fn dispatch_ledger(
    subcommand: LedgerCommands,
    config: &ekos_compiler_core::EkosConfig,
    cwd: &std::path::Path,
) -> Result<()> {
    match subcommand {
        LedgerCommands::Status { storage, json } => {
            crate::commands::ledger::status(config, cwd, storage, json)
        }
        LedgerCommands::Migrate { v3 } => crate::commands::ledger::migrate(config, cwd, v3),
        LedgerCommands::Repair => crate::commands::ledger::repair(config, cwd),
        LedgerCommands::Timeline {
            json: _,
            bucket,
            since,
        } => crate::commands::ledger::timeline(config, cwd, &bucket, since.as_deref()),
        LedgerCommands::Audit { id, json } => {
            crate::commands::ledger::audit(config, cwd, &id, json)
        }
    }
}

/// `ekos query …`
fn dispatch_query(
    subcommand: QueryCommands,
    config: &ekos_compiler_core::EkosConfig,
    cwd: &std::path::Path,
) -> Result<()> {
    match subcommand {
        QueryCommands::Object { id, format } => {
            crate::commands::query::object(config, cwd, &id, &format)
        }
        QueryCommands::Find {
            query,
            explain,
            mode,
        } => crate::commands::query::find(config, cwd, &query, explain, &mode),
        QueryCommands::Neighbourhood { id, depth } => {
            crate::commands::query::neighbourhood(config, cwd, &id, depth)
        }
    }
}

/// `ekos click-house …`
async fn dispatch_click_house(
    subcommand: ClickHouseCommands,
    config: &ekos_compiler_core::EkosConfig,
    cwd: &std::path::Path,
) -> Result<()> {
    match subcommand {
        ClickHouseCommands::Ask { question, json } => {
            crate::commands::clickhouse::ask(config, cwd, &question, json).await
        }
    }
}

/// `ekos branch …`
fn dispatch_branch(
    subcommand: BranchCommands,
    config: &ekos_compiler_core::EkosConfig,
    cwd: &std::path::Path,
) -> Result<()> {
    match subcommand {
        BranchCommands::Create { name } => crate::commands::branch::create(config, cwd, &name),
        BranchCommands::List => crate::commands::branch::list(config, cwd),
        BranchCommands::Merge { name } => crate::commands::branch::merge(config, cwd, &name),
        BranchCommands::Delete { name } => crate::commands::branch::delete(config, cwd, &name),
    }
}

/// `ekos artifact …`
fn dispatch_artifact(
    subcommand: ArtifactCommands,
    config: &ekos_compiler_core::EkosConfig,
    cwd: &std::path::Path,
) -> Result<()> {
    match subcommand {
        ArtifactCommands::Repack => crate::commands::artifact::repack(config, cwd),
    }
}

/// `ekos coordinator …`
async fn dispatch_coordinator(subcommand: CoordinatorCommands) -> Result<()> {
    match subcommand {
        CoordinatorCommands::Serve {
            listen,
            state,
            ttl_seconds,
        } => {
            crate::commands::cluster::serve_coordinator(&listen, state.as_deref(), ttl_seconds)
                .await
        }
        CoordinatorCommands::Status { coordinator } => {
            crate::commands::cluster::status(&coordinator).await
        }
    }
}

/// `ekos compile-worker …`
async fn dispatch_compile_worker(
    subcommand: CompileWorkerCommands,
    extensions: &Extensions,
) -> Result<()> {
    match subcommand {
        CompileWorkerCommands::Run {
            coordinator,
            shard,
            workspace,
            parallel,
            force,
            retry_lease_seconds,
        } => {
            crate::commands::cluster::compile_worker_run_with(
                &coordinator,
                &shard,
                &workspace,
                parallel,
                force,
                retry_lease_seconds,
                extensions,
            )
            .await
        }
    }
}

/// `ekos query-worker …`
async fn dispatch_query_worker(subcommand: QueryWorkerCommands) -> Result<()> {
    match subcommand {
        QueryWorkerCommands::Serve {
            coordinator,
            listen,
            cache,
        } => crate::commands::cluster::serve_query_worker(&coordinator, &listen, &cache).await,
    }
}

/// `ekos marketing …`
async fn dispatch_marketing(
    subcommand: MarketingCommands,
    config: &ekos_compiler_core::EkosConfig,
    cwd: &std::path::Path,
) -> Result<()> {
    match subcommand {
        MarketingCommands::Publish {
            devlog,
            yes,
            dry_run,
        } => crate::commands::marketing::publish(config, cwd, devlog, yes, dry_run).await,
        MarketingCommands::Post { file, confirm } => {
            crate::commands::marketing::post(cwd, &file, confirm).await
        }
    }
}

/// `ekos docs …`
async fn dispatch_docs(
    subcommand: DocsCommands,
    config: &ekos_compiler_core::EkosConfig,
    cwd: &std::path::Path,
) -> Result<()> {
    match subcommand {
        DocsCommands::Generate {
            output,
            format,
            layout,
            prose,
            yes,
        } => {
            let output =
                crate::commands::docs::resolve_output_dir(&config.output_base(cwd), output);
            let format = crate::commands::docs::Format::parse(&format)?;
            let layout = crate::commands::docs::Layout::parse(&layout)?;
            crate::commands::docs::generate(config, cwd, &output, format, layout, prose, yes).await
        }
    }
}

/// `ekos graph …`
fn dispatch_graph(
    subcommand: GraphCommands,
    config: &ekos_compiler_core::EkosConfig,
    cwd: &std::path::Path,
) -> Result<()> {
    match subcommand {
        GraphCommands::Export {
            workspace,
            level,
            format,
            kinds,
            rel_kinds,
            exclude_rel_kinds,
            group_by,
            path_prefix_depth,
            max_nodes,
            max_edges,
            min_degree,
            include_properties,
            as_of,
            first_seen,
            output,
        } => crate::commands::graph::export(
            config,
            cwd,
            crate::commands::graph::ExportArgs {
                workspace,
                level,
                format,
                kinds,
                rel_kinds,
                exclude_rel_kinds,
                group_by,
                path_prefix_depth,
                max_nodes,
                max_edges,
                min_degree,
                include_properties,
                as_of,
                first_seen,
                output,
            },
        ),
    }
}

/// `ekos dbt …`
async fn dispatch_dbt(
    subcommand: DbtCommands,
    config: &ekos_compiler_core::EkosConfig,
    cwd: &std::path::Path,
) -> Result<()> {
    match subcommand {
        DbtCommands::Generate { output } => {
            let output = crate::commands::dbt::resolve_output_dir(&config.output_base(cwd), output);
            crate::commands::dbt::generate(cwd, &output, config).await
        }
    }
}

/// `ekos architecture …`
async fn dispatch_architecture(
    subcommand: ArchitectureCommands,
    config: &ekos_compiler_core::EkosConfig,
    cwd: &std::path::Path,
    extensions: &Extensions,
) -> Result<()> {
    match subcommand {
        ArchitectureCommands::Investigate {
            max_iterations,
            quality_threshold,
            output,
        } => {
            let output =
                crate::commands::architecture::resolve_output_dir(&config.output_base(cwd), output);
            let opts = crate::commands::architecture::InvestigateOptions {
                max_iterations,
                quality_threshold,
                output,
                extensions: extensions.clone(),
            };
            crate::commands::architecture::investigate(config, cwd, opts).await
        }
        ArchitectureCommands::Diff { from, to } => {
            crate::commands::architecture::diff(config, cwd, from, to)
        }
    }
}

/// `ekos migrate …`
async fn dispatch_migrate(
    subcommand: MigrateCommands,
    config: &ekos_compiler_core::EkosConfig,
    cwd: &std::path::Path,
) -> Result<()> {
    match subcommand {
        MigrateCommands::Init {
            name,
            source,
            target,
            source_secret_env,
            target_secret_env,
        } => crate::commands::migrate::init(
            config,
            cwd,
            name,
            source,
            target,
            source_secret_env,
            target_secret_env,
        ),
        MigrateCommands::Discover { project, schemas } => {
            crate::commands::migrate::discover(config, cwd, project, schemas)
        }
        MigrateCommands::Profile {
            project,
            tier,
            unit,
        } => crate::commands::migrate::profile(config, cwd, project, tier, unit),
        MigrateCommands::Assess {
            project,
            unit,
            no_measure,
        } => crate::commands::migrate::assess(config, cwd, project, unit, !no_measure),
        MigrateCommands::Map {
            project,
            unit,
            emit,
        } => crate::commands::migrate::map(config, cwd, project, unit, emit),
        MigrateCommands::Load {
            project,
            unit,
            env,
            chunk_rows,
            dry_run,
        } => crate::commands::migrate::load(config, cwd, project, unit, env, chunk_rows, dry_run),
        MigrateCommands::Validate {
            project,
            unit,
            tier,
        } => crate::commands::migrate::validate(config, cwd, project, unit, tier),
        MigrateCommands::Review { project, unit, env } => {
            crate::commands::migrate::review(config, cwd, project, unit, env)
        }
        MigrateCommands::Approve {
            request,
            project,
            subjects,
            confirm,
            show_evidence,
        } => crate::commands::migrate::decide(
            config,
            cwd,
            project,
            crate::commands::migrate::Decision {
                request_id: request,
                subjects,
                reject_reason: None,
                typed_confirmation: confirm,
                evidence_shown: show_evidence,
            },
        ),
        MigrateCommands::Reject {
            request,
            project,
            subjects,
            reason,
        } => crate::commands::migrate::decide(
            config,
            cwd,
            project,
            crate::commands::migrate::Decision {
                request_id: request,
                subjects,
                reject_reason: Some(reason),
                typed_confirmation: None,
                evidence_shown: false,
            },
        ),
        MigrateCommands::Report {
            project,
            threshold,
            out,
        } => crate::commands::migrate::report(config, cwd, project, threshold, out),
        MigrateCommands::Lower {
            database,
            sql,
            record,
            json,
        } => crate::commands::migrate_logic::lower(config, cwd, database, sql, record, json),
        MigrateCommands::V5 {
            source_sandbox,
            target_sandbox,
            record,
            json,
        } => crate::commands::migrate_logic::run_v5(
            config,
            cwd,
            &source_sandbox,
            &target_sandbox,
            record,
            json,
        ),
        MigrateCommands::Reconstruct {
            source,
            allow_metered,
            record,
            json,
        } => {
            crate::commands::migrate_logic::run_reconstruct(
                config,
                cwd,
                &source,
                allow_metered,
                record,
                json,
            )
            .await
        }
        MigrateCommands::CheckCandidate {
            source,
            candidate,
            json,
        } => crate::commands::migrate_logic::check(config, cwd, &source, &candidate, json),
        MigrateCommands::Status { project } => {
            crate::commands::migrate::status(config, cwd, project)
        }
    }
}

/// `ekos session …`
fn dispatch_session(
    subcommand: SessionCommands,
    config: &ekos_compiler_core::EkosConfig,
    cwd: &std::path::Path,
) -> Result<()> {
    match subcommand {
        SessionCommands::Note {
            text,
            kind,
            rationale,
            anchors,
            session,
        } => crate::commands::session::note(config, cwd, session, &kind, text, rationale, anchors),
        SessionCommands::Status { session } => {
            crate::commands::session::status(config, cwd, session)
        }
        SessionCommands::Commit { session } => {
            crate::commands::session::commit(config, cwd, session)
        }
        SessionCommands::Recall { query, limit, json } => {
            crate::commands::session::recall(config, cwd, &query, limit, json)
        }
        SessionCommands::Brief {
            scope,
            scope_from_git,
            budget,
            format,
        } => crate::commands::session::brief(config, cwd, scope, scope_from_git, budget, &format),
        SessionCommands::Review {
            claim_id,
            decision,
            by,
        } => crate::commands::session::review(config, cwd, &claim_id, &decision, by),
        SessionCommands::Purge {
            session,
            older_than_days,
        } => crate::commands::session::purge(config, cwd, session, older_than_days),
        SessionCommands::Capture { session, file } => {
            crate::commands::session::capture(config, cwd, &session, file)
        }
        SessionCommands::Extract { session } => {
            crate::commands::session::extract(config, cwd, &session)
        }
        SessionCommands::Eval { runs } => crate::commands::session::eval(runs),
        SessionCommands::FingerprintNoise => {
            crate::commands::session::fingerprint_noise(config, cwd)
        }
    }
}

/// `ekos eval …`
async fn dispatch_eval(
    subcommand: EvalCommands,
    config: &ekos_compiler_core::EkosConfig,
    cwd: &std::path::Path,
) -> Result<()> {
    match subcommand {
        EvalCommands::Run {
            dataset,
            datasets_dir,
            category,
            agent,
            limit,
            json,
            output,
            save_answers,
            evidence_only,
            keep_question_echoes,
        } => {
            let opts = crate::commands::eval::EvalRunOpts {
                dataset: dataset.as_deref(),
                datasets_dir,
                category: category.as_deref(),
                agent: agent.as_deref(),
                limit,
                json,
                output,
                save_answers,
                evidence_only,
                keep_question_echoes,
            };
            crate::commands::eval::run(config, cwd, opts).await
        }
        EvalCommands::Regrade {
            report,
            datasets_dir,
            json,
            output,
        } => crate::commands::eval::regrade(
            cwd,
            config,
            crate::commands::eval::EvalRegradeOpts {
                report,
                datasets_dir,
                json,
                output,
            },
        ),
        EvalCommands::History {
            reports_dir,
            limit,
            json,
        } => {
            let opts = crate::commands::eval::EvalHistoryOpts {
                reports_dir,
                limit,
                json,
            };
            crate::commands::eval::history(cwd, opts)
        }
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

    #[test]
    fn config_path_resolution_precedence() {
        use std::path::Path;
        let p = |s: &str| Some(PathBuf::from(s));

        // explicit --config wins over everything
        assert_eq!(
            resolve_config_path(p("a.toml"), p("b.toml"), true, Some(Path::new("ws")), None),
            PathBuf::from("a.toml")
        );
        // EKOS_CONFIG next
        assert_eq!(
            resolve_config_path(None, p("b.toml"), true, Some(Path::new("ws")), None),
            PathBuf::from("b.toml")
        );
        // `ekos mcp serve --workspace ws` → ws/ekos.toml (the F5 fix)
        assert_eq!(
            resolve_config_path(None, None, true, Some(Path::new("ws")), None),
            PathBuf::from("ws/ekos.toml")
        );
        // EKOS_WORKSPACE fallback for MCP
        assert_eq!(
            resolve_config_path(None, None, true, None, Some(Path::new("envws"))),
            PathBuf::from("envws/ekos.toml")
        );
        // non-MCP command ignores the workspace entirely
        assert_eq!(
            resolve_config_path(None, None, false, Some(Path::new("ws")), None),
            PathBuf::from("ekos.toml")
        );
    }
}
