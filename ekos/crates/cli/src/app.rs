use crate::extension::Extensions;
use anyhow::{Context, Result};
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
    /// Run observation passes and write knowledge to the ledger
    Build,
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
    /// Agent session memory inbox (RFC 0151) — needs `[session-memory] enabled = true`
    Session {
        #[command(subcommand)]
        subcommand: SessionCommands,
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
}

#[derive(Subcommand)]
enum ArtifactCommands {
    /// Migrate loose artifact files into packed segments
    Repack,
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
    /// no candidate approval. Never confirms anything — review with the `ekos_identity_review`
    /// MCP tool.
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

/// True for subcommands whose stdout is a single machine-readable document (JSON / NDJSON) that a
/// program parses — their logs must go to stderr so nothing interleaves. Kept in sync with the
/// `--json` flags on these subcommands and with `graph export` (always machine output).
fn emits_machine_output(command: &Commands) -> bool {
    match command {
        Commands::Status { json, .. }
        | Commands::Doctor { json }
        | Commands::Ekl { json, .. }
        | Commands::Coverage { json, .. } => *json,
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
        },
        Commands::Eval { subcommand } => match subcommand {
            EvalCommands::Run { json, .. } => *json,
            EvalCommands::History { json, .. } => *json,
            EvalCommands::Regrade { json, .. } => *json,
        },
        _ => false,
    }
}

/// The whole `ekos` CLI: parses arguments and dispatches every command. The public binary calls
/// it with [`Extensions::none`]; an RFC 0149 build passes its own extensions, which then take
/// part in `build`/`recover`/`commit`, the MCP server, `architecture investigate` and the
/// cluster compile worker.
pub async fn main_with(extensions: Extensions) -> Result<()> {
    let cli = Cli::parse();

    let env_workspace = std::env::var_os("EKOS_WORKSPACE").map(PathBuf::from);
    let mcp_workspace_flag: Option<PathBuf> = match &cli.command {
        Commands::Mcp {
            subcommand: McpCommands::Serve { workspace, .. },
        } => workspace.clone(),
        _ => None,
    };
    let config_path = resolve_config_path(
        cli.config,
        std::env::var_os("EKOS_CONFIG").map(PathBuf::from),
        matches!(cli.command, Commands::Mcp { .. }),
        mcp_workspace_flag.as_deref(),
        env_workspace.as_deref(),
    );
    let config = ekos_compiler_core::EkosConfig::from_file_or_default(&config_path);
    let cwd = std::env::current_dir()?;

    // The MCP server owns stdout for protocol frames; the machine-readable `--json` / graph-export
    // commands own stdout for the document a consumer parses. Both send logs to stderr so nothing
    // interleaves with what a program reads (RFC 0127 R1/R2, RFC 0129 R5/R6).
    if matches!(cli.command, Commands::Mcp { .. }) || emits_machine_output(&cli.command) {
        crate::commands::init_logging_stderr(&config);
    } else {
        crate::commands::init_logging(&config);
    }

    match cli.command {
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
        Commands::Build => crate::commands::build::run_with(&config, &cwd, &extensions).await,
        Commands::Recover { parallel } => {
            crate::commands::recover::run_with(&config, &cwd, parallel, &extensions).await
        }
        Commands::Resolve { force } => crate::commands::resolve::run(&config, &cwd, force),
        Commands::Identity { subcommand } => match subcommand {
            IdentityCommands::Scan => crate::commands::identity::scan(&config, &cwd),
        },
        Commands::Treasury { subcommand } => match subcommand {
            TreasuryCommands::Scan => crate::commands::treasury::scan(&config, &cwd),
        },
        Commands::Compile => crate::commands::compile::run(&config, &cwd).await,
        Commands::Commit { yes } => {
            crate::commands::commit::run_with(&config, &cwd, yes, &extensions).await
        }
        Commands::Ledger { subcommand } => match subcommand {
            LedgerCommands::Status { storage, json } => {
                crate::commands::ledger::status(&config, &cwd, storage, json)
            }
            LedgerCommands::Migrate { v3 } => crate::commands::ledger::migrate(&config, &cwd, v3),
            LedgerCommands::Repair => crate::commands::ledger::repair(&config, &cwd),
            LedgerCommands::Timeline {
                json: _,
                bucket,
                since,
            } => crate::commands::ledger::timeline(&config, &cwd, &bucket, since.as_deref()),
            LedgerCommands::Audit { id, json } => {
                crate::commands::ledger::audit(&config, &cwd, &id, json)
            }
        },
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
        },
        Commands::Status { storage, json } => {
            crate::commands::ledger::status(&config, &cwd, storage, json)
        }
        Commands::Query { subcommand } => match subcommand {
            QueryCommands::Object { id, format } => {
                crate::commands::query::object(&config, &cwd, &id, &format)
            }
            QueryCommands::Find {
                query,
                explain,
                mode,
            } => crate::commands::query::find(&config, &cwd, &query, explain, &mode),
            QueryCommands::Neighbourhood { id, depth } => {
                crate::commands::query::neighbourhood(&config, &cwd, &id, depth)
            }
        },
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
        Commands::ClickHouse { subcommand } => match subcommand {
            ClickHouseCommands::Ask { question, json } => {
                crate::commands::clickhouse::ask(&config, &cwd, &question, json).await
            }
        },
        Commands::Ekl { query, json } => crate::commands::ekl::run(&config, &cwd, &query, json),
        Commands::Diff { from, to } => crate::commands::diff::run(&config, &cwd, from, to),
        Commands::Branch { subcommand } => match subcommand {
            BranchCommands::Create { name } => {
                crate::commands::branch::create(&config, &cwd, &name)
            }
            BranchCommands::List => crate::commands::branch::list(&config, &cwd),
            BranchCommands::Merge { name } => crate::commands::branch::merge(&config, &cwd, &name),
            BranchCommands::Delete { name } => {
                crate::commands::branch::delete(&config, &cwd, &name)
            }
        },
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
        Commands::Artifact { subcommand } => match subcommand {
            ArtifactCommands::Repack => crate::commands::artifact::repack(&config, &cwd),
        },
        Commands::Coordinator { subcommand } => match subcommand {
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
        },
        Commands::CompileWorker { subcommand } => match subcommand {
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
                    &extensions,
                )
                .await
            }
        },
        Commands::QueryWorker { subcommand } => match subcommand {
            QueryWorkerCommands::Serve {
                coordinator,
                listen,
                cache,
            } => crate::commands::cluster::serve_query_worker(&coordinator, &listen, &cache).await,
        },
        Commands::Marketing { subcommand } => match subcommand {
            MarketingCommands::Publish {
                devlog,
                yes,
                dry_run,
            } => crate::commands::marketing::publish(&config, &cwd, devlog, yes, dry_run).await,
        },
        Commands::Docs { subcommand } => match subcommand {
            DocsCommands::Generate {
                output,
                format,
                layout,
                prose,
                yes,
            } => {
                let output = crate::commands::docs::resolve_output_dir(&cwd, output);
                let format = crate::commands::docs::Format::parse(&format)?;
                let layout = crate::commands::docs::Layout::parse(&layout)?;
                crate::commands::docs::generate(&config, &cwd, &output, format, layout, prose, yes)
                    .await
            }
        },
        Commands::Graph { subcommand } => match subcommand {
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
                &config,
                &cwd,
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
        },
        Commands::Dbt { subcommand } => match subcommand {
            DbtCommands::Generate { output } => {
                let output = crate::commands::dbt::resolve_output_dir(&cwd, output);
                crate::commands::dbt::generate(&cwd, &output, &config).await
            }
        },
        Commands::Architecture { subcommand } => match subcommand {
            ArchitectureCommands::Investigate {
                max_iterations,
                quality_threshold,
                output,
            } => {
                let output = crate::commands::architecture::resolve_output_dir(&cwd, output);
                let opts = crate::commands::architecture::InvestigateOptions {
                    max_iterations,
                    quality_threshold,
                    output,
                    extensions: extensions.clone(),
                };
                crate::commands::architecture::investigate(&config, &cwd, opts).await
            }
            ArchitectureCommands::Diff { from, to } => {
                crate::commands::architecture::diff(&config, &cwd, from, to)
            }
        },
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
        Commands::Session { subcommand } => match subcommand {
            SessionCommands::Note {
                text,
                kind,
                rationale,
                anchors,
                session,
            } => crate::commands::session::note(
                &config, &cwd, session, &kind, text, rationale, anchors,
            ),
            SessionCommands::Status { session } => {
                crate::commands::session::status(&config, &cwd, session)
            }
            SessionCommands::Commit { session } => {
                crate::commands::session::commit(&config, &cwd, session)
            }
            SessionCommands::Recall { query, limit, json } => {
                crate::commands::session::recall(&config, &cwd, &query, limit, json)
            }
            SessionCommands::Brief {
                scope,
                scope_from_git,
                budget,
                format,
            } => crate::commands::session::brief(
                &config,
                &cwd,
                scope,
                scope_from_git,
                budget,
                &format,
            ),
            SessionCommands::Review {
                claim_id,
                decision,
                by,
            } => crate::commands::session::review(&config, &cwd, &claim_id, &decision, by),
            SessionCommands::Purge {
                session,
                older_than_days,
            } => crate::commands::session::purge(&config, &cwd, session, older_than_days),
            SessionCommands::Capture { session, file } => {
                crate::commands::session::capture(&config, &cwd, &session, file)
            }
            SessionCommands::Extract { session } => {
                crate::commands::session::extract(&config, &cwd, &session)
            }
            SessionCommands::Eval { runs } => crate::commands::session::eval(runs),
            SessionCommands::FingerprintNoise => {
                crate::commands::session::fingerprint_noise(&config, &cwd)
            }
        },
        Commands::Eval { subcommand } => match subcommand {
            EvalCommands::Run {
                dataset,
                datasets_dir,
                category,
                agent,
                limit,
                json,
                output,
                save_answers,
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
                };
                crate::commands::eval::run(&config, &cwd, opts).await
            }
            EvalCommands::Regrade {
                report,
                datasets_dir,
                json,
                output,
            } => crate::commands::eval::regrade(
                &cwd,
                &config,
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
                crate::commands::eval::history(&cwd, opts)
            }
        },
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
