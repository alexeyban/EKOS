//! RFC 0240 — benchmark adapters for `ekos eval bench`.
//!
//! RFC 0138's harness grades EKOS's own questions by string and id matching. An external
//! benchmark brings its own data layout, is scored by *executing* SQL, has leakage hazards and a
//! submission format, and only means something as a comparison between configurations on the
//! same questions. A [`BenchAdapter`] owns what is specific to one benchmark; everything else —
//! splits, leakage guards, the run loop, statistics, reports — is shared here.
//!
//! ```text
//! suite    — suite.toml, archive checksums, the data/ layout
//! split    — fixed-seed stratified tune / hold-out split
//! leakage  — allowlist + scan of project sources and ledgers for questions, gold SQL, evidence
//! context  — raw DDL, the EKOS pack (from a ledger), value lookup, SQL shape measures
//! sqlexec  — read-only SQLite execution with limits; EX, Soft-F1, R-VES
//! stats    — bootstrap intervals, paired comparison, McNemar
//! report   — the run report, contamination check, compare
//! bird     — the BIRD adapter
//! fixture  — a two-database, ten-question suite for tests
//! ```
//!
//! Opening ledgers, running the EKOS pipeline and building the model stay the CLI's job
//! (`crates/cli/src/commands/eval_bench.rs`), as for `ekos eval run`.

pub mod bird;
pub mod context;
pub mod data_signals;
pub mod fixture;
pub mod leakage;
pub mod report;
pub mod split;
pub mod sqlexec;
pub mod stats;
pub mod suite;

use anyhow::Result;
use async_trait::async_trait;
use context::{DbPack, SchemaInfo, ValueIndex, ValueMatch};
use ekos_recovery::{LlmError, LlmProvider};
use serde::{Deserialize, Serialize};
use sqlexec::Db;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// One benchmark question.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BenchScenario {
    pub id: String,
    pub db_id: String,
    pub question: String,
    /// The benchmark's hint ("oracle knowledge"). A prompt input only, and only when the
    /// experiment turns it on; never written to a project or a ledger.
    pub evidence: String,
    pub gold_sql: String,
    pub difficulty: String,
}

/// Which questions a run uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Split {
    Tune,
    Hold,
    All,
}

impl std::str::FromStr for Split {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Split> {
        match s {
            "tune" => Ok(Split::Tune),
            "hold" => Ok(Split::Hold),
            "all" => Ok(Split::All),
            other => anyhow::bail!("unknown split {other:?} (want tune, hold or all)"),
        }
    }
}

impl std::fmt::Display for Split {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Split::Tune => "tune",
            Split::Hold => "hold",
            Split::All => "all",
        })
    }
}

/// What the model is shown as the schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ContextKind {
    /// The database's own `CREATE TABLE` statements.
    RawDdl,
    /// Rendered from the project's compiled ledger.
    EkosPack,
}

/// One experiment cell: `configs/<cell>.toml` (RFC 0240 §2.4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ExperimentConfig {
    /// The cell's name: `A`, `B`, `C`, `D`, `C-prime`.
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub context: ContextKind,
    #[serde(default)]
    pub evidence: bool,
    #[serde(default)]
    pub value_lookup: bool,
    /// Few-shot examples, from the benchmark's train split only. No suite here has one, so
    /// anything but 0 is refused.
    #[serde(default)]
    pub few_shot: usize,
    /// Candidates per question; with more than one, the answer whose result most candidates
    /// share wins.
    #[serde(default = "one")]
    pub n: usize,
    /// Repair rounds: a candidate that does not run is sent back with the error, this many times.
    #[serde(default = "one")]
    pub r: usize,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    /// Timed runs per correct answer for R-VES (the official scorer repeats more; see the RFC).
    #[serde(default = "default_ves_iterations")]
    pub ves_iterations: usize,
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
    /// RFC 0241: column profiles (rows, nulls, distinct, examples, list-valued) in the pack.
    #[serde(default, skip_serializing_if = "is_false")]
    pub profiles: bool,
    /// RFC 0241: `2` also shows each column's empty-string count and that `''` means missing
    /// (formula_1 stores "no Q3 time" as `''`, and a profile saying "4,935 null" misled).
    #[serde(default = "one_u32", skip_serializing_if = "is_one")]
    pub profile_version: u32,
    /// RFC 0241: `1` — the RFC 0240 lookup; `2` — one line per value with all its columns,
    /// list columns marked for `LIKE`, one-word values only when capitalised or quoted.
    #[serde(default = "one_u32", skip_serializing_if = "is_one")]
    pub value_lookup_version: u32,
    /// RFC 0241: the prompt template.
    #[serde(default, skip_serializing_if = "PromptKind::is_baseline")]
    pub prompt: PromptKind,
    /// RFC 0241: a query returning no rows or a single NULL goes back through a repair round.
    #[serde(default, skip_serializing_if = "is_false")]
    pub result_checks: bool,
    /// RFC 0241: with `n > 1`, candidate i gets a different schema rendering (temperature is 0).
    #[serde(default, skip_serializing_if = "is_false")]
    pub vary_context: bool,
    /// RFC 0242: how an answer is reached — one prompt with RFC 0241's repair rounds, the
    /// detector loop, or the tool-using agent.
    #[serde(default, skip_serializing_if = "Strategy::is_single")]
    pub strategy: Strategy,
    /// RFC 0242: model replies in total for `loop` / `agent` (the first answer included).
    #[serde(
        default = "default_max_turns",
        skip_serializing_if = "is_default_max_turns"
    )]
    pub max_turns: usize,
    /// RFC 0242: show the first clean answer's result once and ask whether it answers exactly
    /// what was asked (cell K2).
    #[serde(default, skip_serializing_if = "is_false")]
    pub verify: bool,
    /// RFC 0252 §8: the pack also lists the references and keys found in the data
    /// (`data/values/<db>.signals.json`, built by `prepare`).
    #[serde(default, skip_serializing_if = "is_false")]
    pub data_signals: bool,
    /// RFC 0242: the finding codes in use (`L001`…, `P001`…, `E000`); empty means all.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub checks: Vec<String>,
    /// The model route: the same keys as `ekos.toml`'s `[llm]`, read by the CLI.
    #[serde(default)]
    pub llm: serde_json::Value,
}

/// How a cell reaches its answer (RFC 0242 §2.3).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Strategy {
    /// One prompt, `n` candidates, `r` repair rounds (RFC 0240/0241).
    #[default]
    Single,
    /// Each answer is checked (lint + probes) and sent back with the findings and the context
    /// they name, up to `max-turns` replies; no tools.
    Loop,
    /// The model may call schema, value and SQL tools; every submission is checked.
    Agent,
}

impl Strategy {
    fn is_single(&self) -> bool {
        *self == Strategy::Single
    }
}

fn default_max_turns() -> usize {
    6
}
fn is_default_max_turns(v: &usize) -> bool {
    *v == 6
}

/// The prompt template a cell uses (RFC 0241 §2.3).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PromptKind {
    /// BIRD's published Mini-Dev baseline template, byte for byte.
    #[default]
    Baseline,
    /// The baseline plus column, grain and answer-order instructions.
    EkosV1,
}

impl PromptKind {
    fn is_baseline(&self) -> bool {
        *self == PromptKind::Baseline
    }
}

// Fields added after RFC 0240 are left out of a cell's serialisation at their defaults, so the
// configuration hash of an existing cell does not change.
fn is_false(b: &bool) -> bool {
    !*b
}
fn one_u32() -> u32 {
    1
}
fn is_one(v: &u32) -> bool {
    *v == 1
}

fn one() -> usize {
    1
}
fn default_max_tokens() -> u32 {
    1024
}
fn default_ves_iterations() -> usize {
    3
}
fn default_timeout() -> u64 {
    30
}

impl ExperimentConfig {
    pub fn load(path: &Path) -> Result<ExperimentConfig> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("reading {}: {e}", path.display()))?;
        let cfg: ExperimentConfig = toml::from_str(&text)
            .map_err(|e| anyhow::anyhow!("parsing {}: {e}", path.display()))?;
        if cfg.n == 0 {
            anyhow::bail!("{}: n must be at least 1", path.display());
        }
        Ok(cfg)
    }

    /// SHA-256 of the whole cell, model route included.
    pub fn hash(&self) -> String {
        suite::sha256_hex(serde_json::to_string(self).unwrap_or_default().as_bytes())
    }

    pub fn limits(&self) -> sqlexec::Limits {
        sqlexec::Limits {
            timeout: std::time::Duration::from_secs(self.timeout_seconds.max(1)),
            ..Default::default()
        }
    }
}

/// One EKOS project `prepare` asks the CLI to build: one per database.
#[derive(Debug, Clone)]
pub struct ProjectSpec {
    /// The project's name in the scratch home (the database id).
    pub name: String,
    pub db_id: String,
    /// Its only source directory.
    pub source_dir: PathBuf,
    /// The files in `source_dir`, relative — everything else there fails the allowlist.
    pub files: BTreeSet<String>,
    /// Appended to the project's `ekos.toml`.
    pub config: String,
    /// SHA-256 over the source files: an unchanged digest and a built ledger mean "up to date".
    pub source_digest: String,
    /// Normalised text of the shipped dictionary, so an evidence sentence the benchmark itself
    /// put there is reported as such rather than as a leak.
    pub dictionary_text: String,
}

/// What `prepare` is given.
pub struct PrepareCtx<'a> {
    pub dirs: &'a suite::SuiteDirs,
    /// Rebuild value indexes even when present.
    pub force: bool,
}

/// One database as a run sees it.
pub struct DbHandle {
    pub db: Db,
    pub schema: SchemaInfo,
    /// From the project's ledger; required by `ekos-pack` cells.
    pub pack: Option<DbPack>,
    pub values: Option<ValueIndex>,
    /// RFC 0241: column profiles from `data/values/<db>.profile.json`.
    pub profile: Option<context::Profile>,
    pub fingerprint: Option<String>,
    /// RFC 0242: what the checks and agent tools know; built for `loop` / `agent` cells.
    pub sql_schema: Option<ekos_sqlgen::Schema>,
}

/// What a run is given.
pub struct RunCtx<'a> {
    pub llm: &'a dyn LlmProvider,
    pub dbs: &'a BTreeMap<String, DbHandle>,
    /// USD per million (input, output) tokens, when the model has a known price.
    pub price: Option<(f64, f64)>,
}

/// A model's answer to one question.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prediction {
    pub sql: Option<String>,
    pub model: String,
    pub prompt_hash: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub latency_ms: u64,
    pub calls: usize,
    pub repairs: usize,
    /// Why there is no runnable SQL, when there is none.
    pub error: Option<String>,
    pub context_tables: BTreeSet<String>,
    pub context_columns: BTreeSet<String>,
    pub context_joins: Vec<String>,
    pub values: Vec<ValueMatch>,
    /// RFC 0241: calls the LLM cache answered (not billed).
    pub cached_calls: usize,
    /// RFC 0241: tokens of the calls that reached the provider.
    pub billed_input_tokens: u64,
    pub billed_output_tokens: u64,
    /// RFC 0241: no SQL, and the last answer used the whole output allowance.
    pub truncated: bool,
    /// RFC 0242: model replies (`loop` / `agent`).
    #[serde(skip_serializing_if = "is_zero")]
    pub turns: usize,
    /// RFC 0242: tools called, in order.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<String>,
    /// RFC 0242: finding codes raised on every submission, in order.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub findings: Vec<String>,
    /// RFC 0242: finding codes still open on the final SQL.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub open_findings: Vec<String>,
}

fn is_zero(v: &usize) -> bool {
    *v == 0
}

/// Why a question was scored as it was (RFC 0241 §2.1).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Correct,
    /// No SQL in the answer (see `Prediction::truncated`).
    NoSql,
    ExecError,
    WrongColumns,
    WrongRows,
    WrongValues,
    /// Not scored (the gold SQL failed, or an older report).
    #[default]
    Unscored,
}

impl Category {
    pub fn as_str(self) -> &'static str {
        match self {
            Category::Correct => "correct",
            Category::NoSql => "no_sql",
            Category::ExecError => "exec_error",
            Category::WrongColumns => "wrong_columns",
            Category::WrongRows => "wrong_rows",
            Category::WrongValues => "wrong_values",
            Category::Unscored => "unscored",
        }
    }
}

/// One question's grades.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Score {
    pub ex: bool,
    pub soft_f1: f64,
    pub rves_reward: f64,
    /// The predicted SQL ran.
    pub executed: bool,
    /// Ran, and names only tables the model was shown (RFC 0239 §2.2 step 6). An unparseable
    /// or failing answer is ungrounded — never dropped from the denominator (RFC 0139).
    pub grounded: bool,
    pub exec_error: Option<String>,
    pub gold_error: Option<String>,
    pub schema_recall: Option<f64>,
    pub join_recall: Option<f64>,
    pub value_hit: Option<f64>,
    pub category: Category,
}

/// Where `export` writes for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportTarget {
    BirdSubmission,
}

impl std::str::FromStr for ExportTarget {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<ExportTarget> {
        match s {
            "bird-submission" => Ok(ExportTarget::BirdSubmission),
            other => anyhow::bail!("unknown export target {other:?} (want bird-submission)"),
        }
    }
}

/// One benchmark (RFC 0240 §2.1).
#[async_trait(?Send)]
pub trait BenchAdapter {
    fn id(&self) -> &'static str;
    /// Data → one project spec per database: checks archives, extracts the allowlisted files,
    /// writes each project's sources and value index. Building the projects is the CLI's.
    fn prepare(&self, ctx: &PrepareCtx) -> Result<Vec<ProjectSpec>>;
    fn scenarios(&self, split: Split) -> Result<Vec<BenchScenario>>;
    /// The database file for `db_id`.
    fn database_path(&self, db_id: &str) -> PathBuf;
    async fn run_one(
        &self,
        s: &BenchScenario,
        cfg: &ExperimentConfig,
        rt: &RunCtx<'_>,
    ) -> Result<Prediction>;
    /// The schema text `cfg` shows the model for this database, before value lookup.
    fn schema_text(&self, cfg: &ExperimentConfig, h: &DbHandle) -> Result<String>;
    fn score(
        &self,
        s: &BenchScenario,
        p: &Prediction,
        cfg: &ExperimentConfig,
        rt: &RunCtx<'_>,
    ) -> Result<Score>;
    fn export(
        &self,
        run: &report::BenchReport,
        target: ExportTarget,
        out_dir: &Path,
    ) -> Result<PathBuf>;
}

/// The adapter a suite names.
pub fn adapter_for(suite: suite::Suite, dirs: suite::SuiteDirs) -> Result<Box<dyn BenchAdapter>> {
    match suite.adapter.as_str() {
        "bird" => Ok(Box::new(bird::BirdAdapter::new(suite, dirs))),
        other => anyhow::bail!("suite {:?} names unknown adapter {other:?}", suite.name),
    }
}

/// Adapters this build knows, for `ekos eval bench list`.
pub const ADAPTERS: &[&str] = &["bird"];

/// One answered question in a run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuestionResult {
    pub id: String,
    pub db_id: String,
    pub difficulty: String,
    pub prediction: Prediction,
    pub score: Score,
}

/// What [`run`] produced.
pub struct RunOutcome {
    pub results: Vec<QuestionResult>,
    /// Set when the model route refused a call for budget; the run stopped there.
    pub budget_stopped: Option<String>,
}

/// Answer and grade every scenario in order. A budget refusal stops the run; any other failure
/// of one question is recorded on that question (as incorrect and ungrounded) and the run goes on.
pub async fn run(
    adapter: &dyn BenchAdapter,
    scenarios: &[BenchScenario],
    cfg: &ExperimentConfig,
    rt: &RunCtx<'_>,
    progress: &mut dyn FnMut(usize, usize, &QuestionResult),
) -> RunOutcome {
    run_from(adapter, scenarios, cfg, rt, Vec::new(), progress).await
}

/// [`run`], starting from results a checkpoint already holds (RFC 0244): their questions are not
/// asked again, and the outcome lists every question in `scenarios` order. `progress` is called
/// for newly answered questions only — the caller appends them to its checkpoint.
pub async fn run_from(
    adapter: &dyn BenchAdapter,
    scenarios: &[BenchScenario],
    cfg: &ExperimentConfig,
    rt: &RunCtx<'_>,
    done: Vec<QuestionResult>,
    progress: &mut dyn FnMut(usize, usize, &QuestionResult),
) -> RunOutcome {
    let mut done: BTreeMap<String, QuestionResult> =
        done.into_iter().map(|r| (r.id.clone(), r)).collect();
    let mut results = Vec::with_capacity(scenarios.len());
    let mut budget_stopped = None;
    for (i, s) in scenarios.iter().enumerate() {
        if let Some(r) = done.remove(&s.id) {
            results.push(r);
            continue;
        }
        let prediction = match adapter.run_one(s, cfg, rt).await {
            Ok(p) => p,
            Err(e) => {
                if let Some(LlmError::Budget(msg)) = e.downcast_ref::<LlmError>() {
                    budget_stopped = Some(msg.clone());
                    break;
                }
                Prediction {
                    error: Some(format!("{e:#}")),
                    ..Default::default()
                }
            }
        };
        // RFC 0244: a question with no gold SQL (a test set) is predicted, not scored.
        let score = if s.gold_sql.trim().is_empty() {
            Score::default()
        } else {
            adapter
                .score(s, &prediction, cfg, rt)
                .unwrap_or_else(|e| Score {
                    gold_error: Some(format!("{e:#}")),
                    ..Default::default()
                })
        };
        let result = QuestionResult {
            id: s.id.clone(),
            db_id: s.db_id.clone(),
            difficulty: s.difficulty.clone(),
            prediction,
            score,
        };
        progress(i + 1, scenarios.len(), &result);
        results.push(result);
    }
    RunOutcome {
        results,
        budget_stopped,
    }
}

/// SHA-256 over a ledger's objects and relationships, independent of read order: what a report
/// records as the knowledge a run was answered from.
pub fn ledger_fingerprint(
    objects: &[ekos_kir::KirObject],
    relationships: &[ekos_kir::KirRelationship],
) -> String {
    let mut lines: Vec<String> = objects
        .iter()
        .map(|o| {
            let props: BTreeMap<_, _> = o.properties.iter().collect();
            format!(
                "o\t{}\t{:?}\t{}\t{}",
                o.id,
                o.kind,
                o.name,
                serde_json::to_string(&props).unwrap_or_default()
            )
        })
        .chain(relationships.iter().map(|r| {
            let props: BTreeMap<_, _> = r.properties.iter().collect();
            format!(
                "r\t{}\t{:?}\t{}\t{}\t{}",
                r.id,
                r.kind,
                r.from,
                r.to,
                serde_json::to_string(&props).unwrap_or_default()
            )
        }))
        .collect();
    lines.sort_unstable();
    suite::sha256_hex(lines.join("\n").as_bytes())
}
