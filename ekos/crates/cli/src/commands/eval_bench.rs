//! `ekos eval bench …` (RFC 0240) — external benchmarks through `ekos-evals`'s adapter layer.
//!
//! The adapter turns a benchmark's data into project sources; this file builds those projects
//! in the suite's scratch EKOS home (RFC 0178) by running the normal pipeline as a child
//! process, opens their ledgers, builds the model route and saves reports — the same split of
//! duties as `ekos eval run`.

use super::eval::{agent_label, check_not_mock};
use super::recover::resolved_key_env;
use super::store::open_store_read_only;
use anyhow::{Context, Result, bail};
use ekos_compiler_core::EkosConfig;
use ekos_compiler_core::config::LlmConfig;
use ekos_evals::bench::context::{self, DbPack, Profile, SchemaInfo, ValueIndex};
use ekos_evals::bench::report::{self, BenchReport, RunMeta};
use ekos_evals::bench::sqlexec::Db;
use ekos_evals::bench::suite::{Suite, SuiteDirs};
use ekos_evals::bench::{
    self, BenchAdapter, DbHandle, ExperimentConfig, ExportTarget, PrepareCtx, ProjectSpec, RunCtx,
    Split, leakage,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// This build's version, recorded per prepared project.
const EKOS_VERSION: &str = env!("CARGO_PKG_VERSION");

/// The tenant every benchmark project lives in, inside the suite's own scratch home.
const TENANT: &str = "bench";

/// API-key variables a pipeline child never sees: building a benchmark project calls no model.
const KEY_VARS: &[&str] = &["ANTHROPIC_API_KEY", "OPENAI_API_KEY", "OPENCODE_API_KEY"];

fn bench_root(cwd: &Path, bench_dir: Option<&Path>) -> PathBuf {
    bench_dir
        .map(Path::to_path_buf)
        .unwrap_or_else(|| cwd.join("evals").join("bench"))
}

fn open_suite(root: &Path, name: &str) -> Result<(Suite, SuiteDirs, Box<dyn BenchAdapter>)> {
    let dir = root.join(name);
    let suite = Suite::load(&dir)?;
    let dirs = SuiteDirs::new(&dir);
    let adapter = bench::adapter_for(suite.clone(), dirs.clone())?;
    Ok((suite, dirs, adapter))
}

/// What `prepare` built, per database.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Prepared {
    pub suite: String,
    pub version: String,
    pub prepared_at: String,
    pub databases: BTreeMap<String, PreparedDb>,
    /// Texts searched for in every ledger (questions, gold SQL, evidence).
    pub scanned_texts: usize,
    pub leak_hits: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PreparedDb {
    pub project: String,
    pub source_digest: String,
    /// The EKOS that compiled it: another version may compile the same sources differently
    /// (RFC 0241 §6 — the SQL analyzer fix), so it is rebuilt.
    #[serde(default)]
    pub ekos_version: String,
    pub ledger_fingerprint: String,
    pub objects: usize,
    pub relationships: usize,
    pub tables: usize,
}

fn read_prepared(dirs: &SuiteDirs) -> Option<Prepared> {
    serde_json::from_str(&std::fs::read_to_string(dirs.prepared()).ok()?).ok()
}

// ── list ─────────────────────────────────────────────────────────────────────────────────────

pub fn list(cwd: &Path, bench_dir: Option<&Path>, json: bool) -> Result<()> {
    let root = bench_root(cwd, bench_dir);
    let mut rows = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&root) {
        let mut dirs: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        dirs.sort();
        for dir in dirs {
            let Ok(suite) = Suite::load(&dir) else {
                continue;
            };
            let sdirs = SuiteDirs::new(&dir);
            let mut configs: Vec<String> = std::fs::read_dir(dir.join("configs"))
                .into_iter()
                .flatten()
                .flatten()
                .filter_map(|e| {
                    let p = e.path();
                    if p.extension()? == "toml" {
                        Some(p.file_stem()?.to_string_lossy().to_string())
                    } else {
                        None
                    }
                })
                .collect();
            configs.sort();
            let prepared = read_prepared(&sdirs);
            rows.push(serde_json::json!({
                "suite": suite.name, "adapter": suite.adapter, "version": suite.version,
                "dialect": suite.dialect, "new_dev": suite.new_dev, "configs": configs,
                "prepared": prepared.as_ref().map(|p| p.prepared_at.clone()),
                "databases": prepared.as_ref().map(|p| p.databases.len()),
            }));
        }
    }
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "adapters": bench::ADAPTERS, "suites": rows,
            }))?
        );
        return Ok(());
    }
    println!("Adapters: {}", bench::ADAPTERS.join(", "));
    if rows.is_empty() {
        println!("No suites under {}", root.display());
    }
    for r in rows {
        println!(
            "  {:<20} {} {} ({}){}  configs: {}  prepared: {}",
            r["suite"].as_str().unwrap_or(""),
            r["adapter"].as_str().unwrap_or(""),
            r["version"].as_str().unwrap_or(""),
            r["dialect"].as_str().unwrap_or(""),
            if r["new_dev"] == true { " New Dev" } else { "" },
            r["configs"]
                .as_array()
                .map(|c| c
                    .iter()
                    .filter_map(|v| v.as_str())
                    .collect::<Vec<_>>()
                    .join(", "))
                .unwrap_or_default(),
            r["prepared"].as_str().unwrap_or("no"),
        );
    }
    Ok(())
}

// ── prepare ──────────────────────────────────────────────────────────────────────────────────

pub struct PrepareOpts<'a> {
    pub suite: &'a str,
    pub bench_dir: Option<&'a Path>,
    /// Never download; fail when an archive is missing.
    pub offline: bool,
    /// Rebuild every project and value index.
    pub force: bool,
    pub json: bool,
}

pub async fn prepare(cwd: &Path, opts: PrepareOpts<'_>) -> Result<()> {
    let root = bench_root(cwd, opts.bench_dir);
    let (suite, dirs, adapter) = open_suite(&root, opts.suite)?;
    for source in [&suite.questions, &suite.databases] {
        let path = dirs.locate(source);
        if let Some(url) = &source.url
            && !path.exists()
        {
            if opts.offline {
                bail!("{} is missing and --offline was given", path.display());
            }
            download(url, &path).await?;
        }
    }
    eprintln!("Preparing {} ({})…", suite.name, suite.version);
    let specs = adapter.prepare(&PrepareCtx {
        dirs: &dirs,
        force: opts.force,
    })?;
    let scenarios = adapter.scenarios(Split::All)?;
    let needles = leakage::Needles::from_scenarios(&scenarios);
    let previous = read_prepared(&dirs).unwrap_or_default();
    let mut prepared = Prepared {
        suite: suite.name.clone(),
        version: suite.version.clone(),
        prepared_at: chrono::Utc::now().to_rfc3339(),
        scanned_texts: needles.len(),
        ..Default::default()
    };
    for spec in &specs {
        let location = crate::home::location(&dirs.home(), TENANT, &spec.name)?;
        let up_to_date = !opts.force
            && previous.databases.get(&spec.db_id).is_some_and(|p| {
                p.source_digest == spec.source_digest && p.ekos_version == EKOS_VERSION
            })
            && location.state_dir().join("ckm").exists();
        if up_to_date {
            eprintln!("  {:<28} up to date", spec.db_id);
        } else {
            eprintln!("  {:<28} building…", spec.db_id);
            build_project(&dirs.home(), &location, spec)?;
        }
        let (objects, relationships) = read_ledger(&location)?;
        let hits = leakage::without_shipped(
            leakage::scan_ledger(&objects, &relationships, &needles),
            &scenarios,
            &spec.dictionary_text,
        );
        if !hits.is_empty() {
            prepared.leak_hits += hits.len();
            for h in hits.iter().take(5) {
                eprintln!(
                    "  LEAK: question {} ({:?}) in {}",
                    h.question_id, h.kind, h.found_in
                );
            }
            bail!(
                "leakage: {} benchmark text(s) found in the ledger of project {} — prepare failed",
                hits.len(),
                spec.name
            );
        }
        let tables = objects
            .iter()
            .filter(|o| o.kind == ekos_kir::ObjectKind::Table)
            .count();
        if tables == 0 {
            bail!(
                "project {} compiled no Table objects from {} — check its SQL dialect rules",
                spec.name,
                spec.source_dir.display()
            );
        }
        let record = PreparedDb {
            project: spec.name.clone(),
            source_digest: spec.source_digest.clone(),
            ekos_version: EKOS_VERSION.to_string(),
            ledger_fingerprint: bench::ledger_fingerprint(&objects, &relationships),
            objects: objects.len(),
            relationships: relationships.len(),
            tables,
        };
        eprintln!(
            "  {:<28} {} tables, {} objects, {} relationships, fingerprint {}",
            spec.db_id,
            record.tables,
            record.objects,
            record.relationships,
            &record.ledger_fingerprint[..12]
        );
        prepared.databases.insert(spec.db_id.clone(), record);
    }
    tokio::fs::write(dirs.prepared(), serde_json::to_string_pretty(&prepared)?).await?;
    if opts.json {
        println!("{}", serde_json::to_string_pretty(&prepared)?);
    } else {
        println!(
            "Prepared {} databases for {}; leakage scan clean ({} texts searched in every ledger).",
            prepared.databases.len(),
            suite.name,
            prepared.scanned_texts
        );
    }
    Ok(())
}

async fn download(url: &str, path: &Path) -> Result<()> {
    eprintln!("Downloading {url}…");
    use tokio::io::AsyncWriteExt;
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let part = path.with_extension("part");
    let mut resp = reqwest::get(url)
        .await
        .and_then(reqwest::Response::error_for_status)
        .with_context(|| format!("downloading {url}"))?;
    let mut file = tokio::fs::File::create(&part).await?;
    while let Some(chunk) = resp.chunk().await? {
        file.write_all(&chunk).await?;
    }
    file.flush().await?;
    drop(file);
    tokio::fs::rename(&part, path).await?;
    Ok(())
}

/// A fresh project for `spec` in the scratch home, then build → recover → resolve → compile →
/// commit, each as `ekos --home <home> --tenant bench --project <name> <stage>`.
fn build_project(
    home: &Path,
    location: &ekos_compiler_core::ProjectLocation,
    spec: &ProjectSpec,
) -> Result<()> {
    let dir = location.dir();
    if dir.exists() {
        // Only ever a project this command created, inside the suite's own data/home.
        if !dir.starts_with(home) {
            bail!(
                "refusing to replace {} outside {}",
                dir.display(),
                home.display()
            );
        }
        std::fs::remove_dir_all(&dir)?;
    }
    super::project::create_quiet(location, &spec.source_dir, &spec.config)?;
    let exe = std::env::current_exe().context("locating the ekos binary")?;
    for stage in [
        &["build"][..],
        &["recover"],
        &["resolve"],
        &["compile"],
        &["commit", "--yes"],
    ] {
        let mut cmd = std::process::Command::new(&exe);
        cmd.arg("--home")
            .arg(home)
            .args(["--tenant", TENANT, "--project", &spec.name])
            .args(stage)
            .current_dir(&spec.source_dir);
        for var in KEY_VARS {
            cmd.env_remove(var);
        }
        let out = cmd
            .output()
            .with_context(|| format!("running {}", exe.display()))?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let tail: Vec<&str> = stderr.lines().rev().take(15).collect();
            bail!(
                "`ekos {}` failed for project {}:\n{}",
                stage.join(" "),
                spec.name,
                tail.into_iter().rev().collect::<Vec<_>>().join("\n")
            );
        }
    }
    Ok(())
}

fn read_ledger(
    location: &ekos_compiler_core::ProjectLocation,
) -> Result<(Vec<ekos_kir::KirObject>, Vec<ekos_kir::KirRelationship>)> {
    let (config, root) = crate::home::load_project(location)?;
    let store = open_store_read_only(&config, &root)?;
    Ok((store.all_objects()?, store.all_relationships()?))
}

// ── run ──────────────────────────────────────────────────────────────────────────────────────

pub struct RunOpts<'a> {
    pub suite: &'a str,
    pub bench_dir: Option<&'a Path>,
    /// `configs/<name>.toml` in the suite, or a path.
    pub config: &'a str,
    pub split: Split,
    pub limit: Option<usize>,
    /// A deterministic hash-ordered subset of the split (see `split::sample`).
    pub sample: Option<usize>,
    /// Only these question ids of the split (empty: all).
    pub questions: &'a [String],
    /// RFC 0244: a checkpoint (`*.partial.jsonl`) of an interrupted run of the same cell; its
    /// questions are not asked again.
    pub resume: Option<PathBuf>,
    /// Replaces the cell's `[llm] model` (recorded in the report's config and hash).
    pub model: Option<&'a str>,
    /// Required for any route that is not a local Ollama model.
    pub allow_metered: bool,
    pub reports_dir: Option<PathBuf>,
    pub json: bool,
}

fn resolve_config(dirs: &SuiteDirs, config: &str) -> PathBuf {
    let direct = PathBuf::from(config);
    if direct.is_file() {
        return direct;
    }
    let name = config.trim_end_matches(".toml");
    dirs.suite.join("configs").join(format!("{name}.toml"))
}

pub async fn run(base: &EkosConfig, cwd: &Path, opts: RunOpts<'_>) -> Result<()> {
    let root = bench_root(cwd, opts.bench_dir);
    let (suite, dirs, adapter) = open_suite(&root, opts.suite)?;
    let mut cfg = ExperimentConfig::load(&resolve_config(&dirs, opts.config))?;
    if let Some(model) = opts.model {
        cfg.llm["model"] = serde_json::Value::String(model.to_string());
    }
    let llm_config: LlmConfig = serde_json::from_value(cfg.llm.clone())
        .context("the cell's [llm] table is not a valid ekos.toml [llm] section")?;
    let provider = llm_config
        .provider
        .clone()
        .unwrap_or_else(|| "anthropic".to_string());
    if provider != "ollama" && !opts.allow_metered {
        bail!(
            "cell {} routes to {provider:?}, a metered provider — pass --allow-metered to spend \
             on it (a local Ollama model needs no flag)",
            cfg.name
        );
    }
    let mut run_config = base.clone();
    run_config.llm = llm_config;
    let llm_dir = dirs.data.join("llm").join("artifacts");
    tokio::fs::create_dir_all(&llm_dir).await?;
    let llm =
        super::recover::build_llm_provider_for(&run_config, ekos_recovery::Task::NlSql, &llm_dir);
    check_not_mock(
        llm.as_ref(),
        &agent_label(&run_config),
        resolved_key_env(&run_config),
    )?;
    let model = run_config
        .llm
        .model
        .clone()
        .unwrap_or_else(|| llm.model_name().to_string());
    let price = if provider == "ollama" {
        Some((0.0, 0.0))
    } else {
        ekos_recovery::pricing::price(&model, &run_config.llm.prices).map(|p| (p.input, p.output))
    };

    let mut planned = adapter.scenarios(opts.split)?;
    if let Some(n) = opts.sample {
        planned = bench::split::sample(&planned, suite.split.seed, n);
    }
    if let Some(limit) = opts.limit {
        planned.truncate(limit);
    }
    if !opts.questions.is_empty() {
        planned = select_questions(planned, opts.questions, opts.split)?;
    }
    if ekos_recovery::pricing::collects_data(&model) {
        eprintln!(
            "warning: {model} is a free model whose provider may keep prompts — fine for a public \
             benchmark, never for a client's schema"
        );
    }
    if planned.is_empty() {
        bail!("no questions in split {} of {}", opts.split, suite.name);
    }
    let hold_hash = bench::split::ids_hash(&adapter.scenarios(Split::Hold)?);
    let prepared = read_prepared(&dirs);
    let mut dbs = BTreeMap::new();
    for db_id in planned
        .iter()
        .map(|s| s.db_id.clone())
        .collect::<std::collections::BTreeSet<_>>()
    {
        let handle = open_db(
            &dirs,
            adapter.as_ref(),
            &suite,
            &cfg,
            prepared.as_ref(),
            &db_id,
        )?;
        dbs.insert(db_id, handle);
    }
    let rt = RunCtx {
        llm: llm.as_ref(),
        dbs: &dbs,
        price,
    };
    let reports_dir = opts.reports_dir.clone().unwrap_or_else(|| {
        cwd.join("evals")
            .join("reports")
            .join("bench")
            .join(&suite.name)
    });
    let earlier = report::load_all(&reports_dir);
    eprintln!(
        "Running cell {} on {} ({} questions, split {}, model {model})…",
        cfg.name,
        suite.name,
        planned.len(),
        opts.split
    );
    let started_at = chrono::Utc::now().to_rfc3339();
    let (checkpoint_path, done) = match &opts.resume {
        Some(path) => (path.clone(), read_checkpoint(path, &cfg.hash())?),
        None => {
            tokio::fs::create_dir_all(&reports_dir).await?;
            let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%S");
            let name = format!(
                "{stamp}-{}-{}.partial.jsonl",
                cfg.name.to_lowercase(),
                opts.split
            );
            (reports_dir.join(name), Vec::new())
        }
    };
    let mut checkpoint = open_checkpoint(&checkpoint_path, &cfg.hash())?;
    if !done.is_empty() {
        eprintln!(
            "Resuming from {}: {} questions already answered.",
            checkpoint_path.display(),
            done.len()
        );
    }
    let mut correct = 0usize;
    let outcome = bench::run_from(
        adapter.as_ref(),
        &planned,
        &cfg,
        &rt,
        done,
        &mut |i, total, r| {
            correct += r.score.ex as usize;
            if let Err(e) = append_checkpoint(&mut checkpoint, r) {
                eprintln!(
                    "warning: checkpoint {} not written: {e}",
                    checkpoint_path.display()
                );
            }
            eprintln!(
                "  [{i}/{total}] q{} {:<26} {} {:>6} ms  running EX {:.1}%",
                r.id,
                r.db_id,
                if r.score.ex { "✓" } else { "✗" },
                r.prediction.latency_ms,
                correct as f64 * 100.0 / i as f64
            );
        },
    )
    .await;
    eprintln!("Checkpoint: {}", checkpoint_path.display());
    let report = BenchReport::assemble(
        RunMeta {
            suite: &suite,
            config: &cfg,
            split: opts.split,
            planned: &planned,
            hold_ids_hash: &hold_hash,
            model,
            ledger_fingerprints: dbs
                .iter()
                .filter_map(|(k, v)| Some((k.clone(), v.fingerprint.clone()?)))
                .collect(),
            started_at,
            finished_at: chrono::Utc::now().to_rfc3339(),
            price,
            earlier: &earlier,
        },
        outcome,
    );
    let path = report.save(&reports_dir)?;
    if opts.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print!("{}", report.render());
        println!("Report: {}", path.display());
    }
    Ok(())
}

/// The first line of a checkpoint: which cell it belongs to.
const CHECKPOINT_VERSION: u32 = 1;

/// Opens a checkpoint for appending, writing its header when it is new.
fn open_checkpoint(path: &Path, config_hash: &str) -> Result<std::fs::File> {
    use std::io::Write;
    let new = !path.is_file();
    // A crash can cut the last line short; the next result must not be glued onto it.
    let torn = !new
        && std::fs::read(path)
            .ok()
            .is_some_and(|b| !b.is_empty() && !b.ends_with(b"\n"));
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("opening checkpoint {}", path.display()))?;
    if new {
        writeln!(
            f,
            "{}",
            serde_json::json!({ "checkpoint": CHECKPOINT_VERSION, "config_hash": config_hash })
        )?;
    } else if torn {
        writeln!(f)?;
    }
    Ok(f)
}

fn append_checkpoint(f: &mut std::fs::File, r: &bench::QuestionResult) -> Result<()> {
    use std::io::Write;
    writeln!(f, "{}", serde_json::to_string(r)?)?;
    f.flush()?;
    Ok(())
}

/// The results a checkpoint holds; refuses one written for another cell configuration. A last
/// line cut short by a crash is ignored.
fn read_checkpoint(path: &Path, config_hash: &str) -> Result<Vec<bench::QuestionResult>> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading checkpoint {}", path.display()))?;
    let mut lines = text.lines();
    let header: serde_json::Value = lines
        .next()
        .map(serde_json::from_str)
        .transpose()?
        .unwrap_or_default();
    if header["config_hash"].as_str() != Some(config_hash) {
        bail!(
            "checkpoint {} was written by another cell configuration (hash {}); resume with the \
             same cell, or start a new run",
            path.display(),
            header["config_hash"].as_str().unwrap_or("?")
        );
    }
    Ok(lines
        .filter_map(|l| serde_json::from_str::<bench::QuestionResult>(l).ok())
        .collect())
}

/// The questions with these ids, in the split's order; an id not in the split is an error.
fn select_questions(
    planned: Vec<bench::BenchScenario>,
    ids: &[String],
    split: Split,
) -> Result<Vec<bench::BenchScenario>> {
    let wanted: std::collections::BTreeSet<&str> = ids.iter().map(|s| s.trim()).collect();
    let missing: Vec<&str> = wanted
        .iter()
        .copied()
        .filter(|id| !planned.iter().any(|s| s.id == *id))
        .collect();
    if !missing.is_empty() {
        bail!("question ids not in split {split}: {}", missing.join(", "));
    }
    Ok(planned
        .into_iter()
        .filter(|s| wanted.contains(s.id.as_str()))
        .collect())
}

/// One database as a run sees it: the file, its catalogue, the pack and fingerprint from its
/// project's ledger, and the value index and profile `prepare` wrote.
fn open_db(
    dirs: &SuiteDirs,
    adapter: &dyn BenchAdapter,
    suite: &Suite,
    cfg: &ExperimentConfig,
    prepared: Option<&Prepared>,
    db_id: &str,
) -> Result<DbHandle> {
    let db = Db::open(&adapter.database_path(db_id))?;
    let schema = SchemaInfo::read(&db)?;
    let location = crate::home::location(&dirs.home(), TENANT, db_id)?;
    let (pack, fingerprint) = if location.config_path().is_file() {
        let (objects, relationships) = read_ledger(&location)?;
        (
            Some(DbPack::from_ledger(&objects, &relationships)),
            Some(bench::ledger_fingerprint(&objects, &relationships)),
        )
    } else if cfg.context == bench::ContextKind::EkosPack {
        bail!(
            "no compiled project for {db_id} — run `ekos eval bench prepare --suite {}` first",
            suite.name
        );
    } else {
        (None, None)
    };
    if let (Some(fp), Some(p)) = (&fingerprint, prepared)
        && p.databases
            .get(db_id)
            .is_some_and(|d| &d.ledger_fingerprint != fp)
    {
        eprintln!("warning: the {db_id} ledger changed since prepare");
    }
    let values_path = dirs.values().join(format!("{db_id}.json"));
    let values = match std::fs::read_to_string(&values_path) {
        Ok(text) => Some(serde_json::from_str::<ValueIndex>(&text)?),
        Err(_) if cfg.value_lookup => bail!(
            "no value index {} — run `ekos eval bench prepare` first",
            values_path.display()
        ),
        Err(_) => None,
    };
    let profile_path = dirs.values().join(format!("{db_id}.profile.json"));
    let profile = match std::fs::read_to_string(&profile_path) {
        Ok(text) => Some(serde_json::from_str::<Profile>(&text)?),
        Err(_) if cfg.profiles => bail!(
            "no column profile {} — run `ekos eval bench prepare` first",
            profile_path.display()
        ),
        Err(_) => None,
    };
    // RFC 0242: the checks' view is built only for cells that use it (it copies the value index).
    let sql_schema = if cfg.strategy == bench::Strategy::Single {
        None
    } else {
        Some(context::sqlgen_schema(
            &db,
            &schema,
            pack.as_ref(),
            profile.as_ref(),
            values.as_ref(),
        )?)
    };
    Ok(DbHandle {
        db,
        schema,
        pack,
        values,
        profile,
        fingerprint,
        sql_schema,
    })
}

/// `ekos eval bench context`: the schema text a cell shows the model for one database — from the
/// project's ledger for an `ekos-pack` cell — before any question-specific value lookup.
pub fn context(
    cwd: &Path,
    bench_dir: Option<&Path>,
    suite_name: &str,
    cell: &str,
    db_id: &str,
) -> Result<()> {
    let root = bench_root(cwd, bench_dir);
    let (suite, dirs, adapter) = open_suite(&root, suite_name)?;
    let cfg = ExperimentConfig::load(&resolve_config(&dirs, cell))?;
    let prepared = read_prepared(&dirs);
    let handle = open_db(
        &dirs,
        adapter.as_ref(),
        &suite,
        &cfg,
        prepared.as_ref(),
        db_id,
    )?;
    print!("{}", adapter.schema_text(&cfg, &handle)?);
    Ok(())
}

/// `ekos eval bench checks` (RFC 0242 §2.3): replays every finding over a report's final SQL,
/// with no model calls, and prints how often each code fired on answers scored right and wrong —
/// the evidence for which detectors a `loop` / `agent` cell keeps.
pub fn checks(cwd: &Path, bench_dir: Option<&Path>, report_path: &Path, json: bool) -> Result<()> {
    let run = BenchReport::load(report_path)?;
    let root = bench_root(cwd, bench_dir);
    let (suite, dirs, adapter) = open_suite(&root, &run.suite)?;
    let mut cfg = run.config.clone();
    cfg.strategy = bench::Strategy::Loop;
    let prepared = read_prepared(&dirs);
    let mut dbs: BTreeMap<String, DbHandle> = BTreeMap::new();
    let mut raised: Vec<(Vec<String>, bool)> = Vec::new();
    let mut examples: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for q in &run.questions {
        let Some(sql) = &q.prediction.sql else {
            continue;
        };
        if !dbs.contains_key(&q.db_id) {
            let h = open_db(
                &dirs,
                adapter.as_ref(),
                &suite,
                &cfg,
                prepared.as_ref(),
                &q.db_id,
            )?;
            dbs.insert(q.db_id.clone(), h);
        }
        let findings = ekos_evals::bench::bird::check_sql(&dbs[&q.db_id], &cfg, &q.db_id, sql)?;
        for f in &findings {
            examples.entry(f.code.clone()).or_default().push(format!(
                "{} {}: {}",
                q.id,
                if q.score.ex { "right" } else { "wrong" },
                f.message
            ));
        }
        raised.push((findings.into_iter().map(|f| f.code).collect(), q.score.ex));
    }
    let counts = report::finding_counts(raised.iter().map(|(c, ex)| (c, *ex)));
    let wrong_total = raised.iter().filter(|(_, ex)| !ex).count();
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "report": report_path,
                "answers": raised.len(),
                "wrong": wrong_total,
                "codes": counts,
                "examples": examples,
            }))?
        );
        return Ok(());
    }
    println!(
        "{} answers with SQL, {} scored wrong ({:.0}%). Per code: answers it fires on.",
        raised.len(),
        wrong_total,
        100.0 * wrong_total as f64 / raised.len().max(1) as f64
    );
    println!(
        "{:<6} {:>6} {:>6} {:>8}  keep (wrong ≥ 2×right and ≥ 3)",
        "code", "right", "wrong", "% wrong"
    );
    for (code, (right, wrong)) in &counts {
        let share = 100.0 * *wrong as f64 / (*right + *wrong).max(1) as f64;
        let keep = *wrong >= 2 * *right && *wrong >= 3;
        println!(
            "{code:<6} {right:>6} {wrong:>6} {share:>7.0}%  {}",
            if keep { "yes" } else { "no" }
        );
    }
    Ok(())
}

// ── compare / export ─────────────────────────────────────────────────────────────────────────

pub fn compare(a: &Path, b: &Path, json: bool) -> Result<()> {
    let (a, b) = (BenchReport::load(a)?, BenchReport::load(b)?);
    let cmp = report::compare(&a, &b);
    if json {
        println!("{}", serde_json::to_string_pretty(&cmp)?);
    } else {
        print!("{}", cmp.render());
    }
    Ok(())
}

pub fn export(
    cwd: &Path,
    bench_dir: Option<&Path>,
    report_path: &Path,
    target: ExportTarget,
    out: Option<PathBuf>,
) -> Result<()> {
    let run = BenchReport::load(report_path)?;
    let root = bench_root(cwd, bench_dir);
    let (_, dirs, adapter) = open_suite(&root, &run.suite)?;
    let out = out.unwrap_or_else(|| dirs.data.join("export"));
    let path = adapter.export(&run, target, &out)?;
    println!("Wrote {}", path.display());
    println!(
        "Sending it is a human step: follow the benchmark's submission guideline (RFC 0240 §3)."
    );
    Ok(())
}

/// Bench reports for `ekos eval history`, newest last.
pub fn history_rows(reports_dir: &Path) -> Vec<(PathBuf, BenchReport)> {
    report::load_all(&reports_dir.join("bench"))
}

pub fn render_history(runs: &[(PathBuf, BenchReport)]) -> String {
    let mut out = String::from("\nBenchmark runs (RFC 0240)\n");
    out.push_str(&format!(
        "{:<26} {:<16} {:<8} {:<5} {:>5} {:>7} {:>17}  {}\n",
        "started", "suite", "cell", "split", "n", "EX%", "95% CI", "model"
    ));
    for (_, r) in runs {
        out.push_str(&format!(
            "{:<26} {:<16} {:<8} {:<5} {:>5} {:>7.2} {:>17}  {}{}{}\n",
            r.started_at.chars().take(25).collect::<String>(),
            r.suite,
            r.cell,
            r.split.to_string(),
            r.n,
            r.metrics.ex.mean,
            format!("[{:.1}, {:.1}]", r.metrics.ex.low, r.metrics.ex.high),
            r.model,
            if r.budget_stopped.is_some() {
                " (budget stop)"
            } else {
                ""
            },
            if r.contaminated.is_some() {
                " (contaminated)"
            } else {
                ""
            },
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scenario(id: &str) -> bench::BenchScenario {
        bench::BenchScenario {
            id: id.into(),
            db_id: "db".into(),
            question: String::new(),
            evidence: String::new(),
            gold_sql: String::new(),
            difficulty: String::new(),
        }
    }

    fn result(id: &str) -> bench::QuestionResult {
        bench::QuestionResult {
            id: id.into(),
            db_id: "db".into(),
            difficulty: String::new(),
            prediction: Default::default(),
            score: Default::default(),
        }
    }

    #[test]
    fn a_checkpoint_round_trips_and_refuses_another_cell() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("run.partial.jsonl");
        let mut f = open_checkpoint(&path, "hash-a").unwrap();
        append_checkpoint(&mut f, &result("1")).unwrap();
        append_checkpoint(&mut f, &result("2")).unwrap();
        drop(f);
        // A crash mid-line leaves a fragment, which is ignored.
        std::fs::write(
            &path,
            std::fs::read_to_string(&path).unwrap() + "{\"id\":\"3\",",
        )
        .unwrap();
        let done = read_checkpoint(&path, "hash-a").unwrap();
        assert_eq!(
            done.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            vec!["1", "2"]
        );
        let e = read_checkpoint(&path, "hash-b").unwrap_err();
        assert!(e.to_string().contains("another cell configuration"), "{e}");
        // Reopening appends after the existing lines; no second header.
        let mut f = open_checkpoint(&path, "hash-a").unwrap();
        append_checkpoint(&mut f, &result("4")).unwrap();
        assert_eq!(read_checkpoint(&path, "hash-a").unwrap().len(), 3);
    }

    #[test]
    fn questions_selects_exactly_those_ids_in_split_order() {
        let planned = vec![scenario("1"), scenario("5"), scenario("9")];
        let ids = vec!["9".to_string(), " 1".to_string()];
        let got = select_questions(planned.clone(), &ids, Split::Tune).unwrap();
        assert_eq!(
            got.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
            vec!["1", "9"]
        );
        let e = select_questions(planned, &["2".to_string()], Split::Tune).unwrap_err();
        assert!(e.to_string().contains("not in split tune: 2"), "{e}");
    }
}
