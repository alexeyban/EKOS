//! RFC 0233 — `ekos check refs`, and the integrity index `ekos commit` writes.
//!
//! The check itself is `ekos_semantic::xref` (pure). This file supplies what it cannot know —
//! the files the last commit observed, a `JavaMethod`'s file from its evidence, `[xref]` — and
//! does the rest outside the engine: suppression (`xref-ignore.toml`, a reason per rule), the
//! confidence and scope filters, baselines and the table / JSON / SARIF output.

use super::store::open_store_read_only;
use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use ekos_compiler_core::EkosConfig;
use ekos_kir::KirObject;
use ekos_ledger::KnowledgeReader;
use ekos_semantic::xref::{self, Diagnostic, DiagnosticKind, Report, Severity};
use ekos_semantic::xref_triage;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// The integrity index, under the state directory.
pub const INDEX: &str = "integrity/current.json";
const DEFAULT_IGNORE: &str = "xref-ignore.toml";

/// Which references a run reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Scope {
    /// Code, ETL and dbt references into a schema
    CrossSystem,
    /// References that stay in one system (SQL routines and views to tables)
    Code,
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Format {
    Table,
    Json,
    Sarif,
}

/// `ekos check refs`' arguments.
pub struct RefsArgs {
    pub scope: Scope,
    pub min_confidence: Option<f64>,
    pub format: Format,
    pub fail_on: Option<Severity>,
    pub baseline: Option<String>,
    pub show_suppressed: bool,
    /// Show reachability findings too (they are always in the metrics and the JSON index).
    pub reachability: bool,
    /// Read the commit-time integrity index instead of recomputing.
    pub from_index: bool,
}

/// One `[[ignore]]` rule of `xref-ignore.toml`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct IgnoreRule {
    #[serde(default)]
    fingerprint: Option<String>,
    /// A glob (`*`) over the normalized target name.
    #[serde(default)]
    target: Option<String>,
    /// A glob (`*`) over the source file.
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    reason: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct IgnoreFile {
    #[serde(default)]
    ignore: Vec<IgnoreRule>,
}

impl IgnoreRule {
    fn matches(&self, d: &Diagnostic) -> bool {
        self.fingerprint
            .as_ref()
            .is_none_or(|f| *f == d.fingerprint)
            && self
                .target
                .as_ref()
                .is_none_or(|g| glob_match(&g.to_lowercase(), &d.target_spec.normalized))
            && self
                .source
                .as_ref()
                .is_none_or(|g| d.source.file.as_deref().is_some_and(|f| glob_match(g, f)))
    }
}

/// `*` matches any run of characters; everything else matches itself.
fn glob_match(pattern: &str, text: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == text;
    }
    let (first, last) = (parts[0], parts[parts.len() - 1]);
    if !text.starts_with(first) || text.len() < first.len() + last.len() || !text.ends_with(last) {
        return false;
    }
    let mut rest = &text[first.len()..text.len() - last.len()];
    for p in &parts[1..parts.len() - 1] {
        match rest.find(p) {
            Some(i) => rest = &rest[i + p.len()..],
            None => return false,
        }
    }
    true
}

fn ignore_path(config: &EkosConfig, cwd: &Path) -> PathBuf {
    let state = config.ekos_dir(cwd);
    match &config.xref.ignore_file {
        Some(p) if p.is_absolute() => p.clone(),
        Some(p) => state.join(p),
        None => state.join(DEFAULT_IGNORE),
    }
}

/// The suppression rules; a rule without a `reason` or without anything to match is an error.
fn load_ignores(path: &Path) -> Result<Vec<IgnoreRule>> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Ok(Vec::new());
    };
    let file: IgnoreFile =
        toml::from_str(&text).with_context(|| format!("reading {}", path.display()))?;
    for (i, r) in file.ignore.iter().enumerate() {
        if r.reason.trim().is_empty() {
            bail!(
                "{}: rule {} has no `reason` — every suppression must say why",
                path.display(),
                i + 1
            );
        }
        if r.fingerprint.is_none() && r.target.is_none() && r.source.is_none() {
            bail!(
                "{}: rule {} names no `fingerprint`, `target` or `source`, so it would hide everything",
                path.display(),
                i + 1
            );
        }
    }
    Ok(file.ignore)
}

/// `[xref.severity]` as the engine's types.
fn severity_overrides(config: &EkosConfig) -> Result<BTreeMap<DiagnosticKind, Severity>> {
    config
        .xref
        .severity
        .iter()
        .map(|(k, v)| {
            let kind = DiagnosticKind::parse(k)
                .with_context(|| format!("[xref.severity]: unknown kind `{k}`"))?;
            let sev = Severity::parse(v).with_context(|| {
                format!("[xref.severity] {k}: `{v}` is not error, warning or info")
            })?;
            Ok((kind, sev))
        })
        .collect()
}

/// The files the last commit observed, when a manifest exists.
fn committed_files(config: &EkosConfig, cwd: &Path) -> Option<BTreeSet<String>> {
    crate::freshness::committed_manifest(config, cwd).map(|m| m.files.into_keys().collect())
}

/// Directories never walked for unobserved files: dependencies, build output, VCS and EKOS state.
const SKIP_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    ".ekos",
    "__pycache__",
    ".venv",
    "venv",
];
/// A walk stops here; the check then simply knows fewer excluded files.
const MAX_WALK: usize = 300_000;

/// Files under the observed directories that observation left out (`ignore-patterns`), keyed
/// like the manifest (workspace-relative). Names only: no file is opened.
fn unobserved_files(
    config: &EkosConfig,
    cwd: &Path,
    observed: &BTreeSet<String>,
) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut seen = 0usize;
    for base in config.observe_bases(cwd) {
        let walker = walkdir::WalkDir::new(&base.path)
            .into_iter()
            .filter_entry(|e| {
                !(e.file_type().is_dir() && SKIP_DIRS.iter().any(|d| e.file_name() == *d))
            });
        for entry in walker.filter_map(std::result::Result::ok) {
            seen += 1;
            if seen > MAX_WALK {
                return out;
            }
            if !entry.file_type().is_file() {
                continue;
            }
            let Ok(rel) = entry.path().strip_prefix(cwd) else {
                continue;
            };
            let key = rel.to_string_lossy().replace('\\', "/");
            if !observed.contains(&key) {
                out.insert(key);
            }
        }
    }
    out
}

/// Run the engine over `objects`, with a `JavaMethod`'s file and line read from its evidence.
fn compute(
    config: &EkosConfig,
    reader: &dyn KnowledgeReader,
    objects: &[KirObject],
    relationships: &[ekos_kir::KirRelationship],
    live_files: Option<&BTreeSet<String>>,
    unobserved: Option<&BTreeSet<String>>,
) -> Result<Report> {
    let locate = |id: &ekos_kir::KirId| -> Option<(String, Option<u32>)> {
        let ev = reader.get_evidence(id).ok()??;
        Some((ev.location.path, ev.location.line))
    };
    Ok(xref::check(
        objects,
        &xref::Options {
            live_files,
            locate: Some(&locate),
            relationships,
            unobserved_files: unobserved,
            entry_points: &config.xref.entry_points,
            severity: severity_overrides(config)?,
        },
    ))
}

/// What `ekos commit` records: the report and the revision it describes.
#[derive(Debug, Serialize, Deserialize)]
pub struct Index {
    pub computed_at: DateTime<Utc>,
    pub objects: usize,
    pub report: Report,
}

/// `ekos commit`: recompute the integrity index. Returns (findings by severity, resolution rate).
pub fn commit_step(
    config: &EkosConfig,
    cwd: &Path,
    reader: &dyn KnowledgeReader,
) -> Result<(BTreeMap<Severity, usize>, Option<f64>)> {
    let objects = reader.all_objects()?;
    // The manifest this commit is about to promote: what was observed for it.
    let live = crate::freshness::built_manifest(config, cwd)
        .map(|m| m.files.into_keys().collect::<BTreeSet<String>>())
        .or_else(|| committed_files(config, cwd));
    let relationships = reader.all_relationships()?;
    let unobserved = live.as_ref().map(|l| unobserved_files(config, cwd, l));
    let report = compute(
        config,
        reader,
        &objects,
        &relationships,
        live.as_ref(),
        unobserved.as_ref(),
    )?;
    let mut counts = BTreeMap::new();
    for d in &report.diagnostics {
        *counts.entry(d.severity).or_insert(0) += 1;
    }
    let rate = report.metrics.resolution_rate;
    let path = config.ekos_dir(cwd).join(INDEX);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let index = Index {
        computed_at: Utc::now(),
        objects: objects.len(),
        report,
    };
    std::fs::write(&path, serde_json::to_vec(&index)?)?;
    Ok((counts, rate))
}

/// MCP `ekos_xref_check` (read-only): the findings the last commit's integrity index holds,
/// filtered, and — with `name` — what that one name resolves to now.
pub fn agent_check(
    config: &EkosConfig,
    cwd: &Path,
    reader: &dyn KnowledgeReader,
    args: &Value,
    limit: usize,
) -> Result<Value> {
    let mut out = json!({});
    if let Some(name) = args.get("name").and_then(Value::as_str) {
        let n = xref::normalize(name);
        let tail = n.rsplit('.').next().unwrap_or(&n);
        let mut candidates = Vec::new();
        for (id, found) in reader.find_objects(tail)?.into_iter().take(200) {
            if xref::normalize(&found).rsplit('.').next() == Some(tail)
                && let Some(o) = reader.get_object(&id)?
            {
                candidates.push(o);
            }
        }
        out["name_check"] = serde_json::to_value(xref::check_name(&candidates, name))?;
    }
    let Some(index) = read_index(config, cwd) else {
        out["note"] = json!("No integrity index yet: it is written by `ekos commit`.");
        return Ok(out);
    };
    let kind = args.get("kind").and_then(Value::as_str);
    let min = args
        .get("min_confidence")
        .and_then(Value::as_f64)
        .unwrap_or(config.xref.min_confidence);
    let cross_only = args.get("scope").and_then(Value::as_str) == Some("cross-system");
    let needle = args
        .get("subject")
        .and_then(Value::as_str)
        .map(str::to_lowercase);
    let rules = load_ignores(&ignore_path(config, cwd))?;
    let shown: Vec<&Diagnostic> = index
        .report
        .diagnostics
        .iter()
        .filter(|d| kind.is_none_or(|k| d.kind.as_str() == k))
        .filter(|d| d.confidence >= min && (!cross_only || d.cross_system))
        .filter(|d| !rules.iter().any(|r| r.matches(d)))
        .filter(|d| {
            needle.as_ref().is_none_or(|n| {
                d.target_spec.normalized.contains(n.as_str())
                    || d.source.object_name.to_lowercase().contains(n.as_str())
                    || d.source
                        .file
                        .as_deref()
                        .is_some_and(|f| f.to_lowercase().contains(n.as_str()))
            })
        })
        .collect();
    out["computed_at"] = json!(index.computed_at);
    out["metrics"] = serde_json::to_value(&index.report.metrics)?;
    out["total"] = json!(shown.len());
    out["truncated"] = json!(shown.len() > limit);
    out["diagnostics"] = serde_json::to_value(shown.into_iter().take(limit).collect::<Vec<_>>())?;
    out["note"] = json!(
        "Findings are evidence-backed but not verdicts: an `info` coverage gap means EKOS did not \
         compile where the target lives. `DynamicReferenceUnknown` is not dead code. Report the \
         reason and file:line with each finding."
    );
    Ok(out)
}

/// What a model is shown for one finding: an ambiguous reference's candidates, or — for a
/// routine nothing provably reaches — the objects whose indexed text names it.
fn triage_context(
    reader: &dyn KnowledgeReader,
    d: &Diagnostic,
) -> Result<Vec<xref_triage::ContextItem>> {
    if d.kind == DiagnosticKind::AmbiguousReference {
        return Ok(d
            .candidates
            .iter()
            .map(|c| xref_triage::ContextItem {
                id: c.id.to_string(),
                kind: c.kind.clone(),
                name: c.name.clone(),
                file: c.file.clone(),
            })
            .collect());
    }
    let mut out = Vec::new();
    for (id, _) in reader
        .find_objects(&d.source.object_name)?
        .into_iter()
        .take(40)
    {
        if id == d.source.object_id {
            continue;
        }
        let Some(o) = reader.get_object(&id)? else {
            continue;
        };
        out.push(xref_triage::ContextItem {
            id: id.to_string(),
            kind: o.kind.to_string(),
            name: o.name.clone(),
            file: o
                .properties
                .get("source_path")
                .and_then(Value::as_str)
                .map(str::to_string),
        });
        if out.len() == 8 {
            break;
        }
    }
    Ok(out)
}

/// `ekos check explain` (RFC 0233 Phase 4): suggestions from the configured model for the
/// findings the graph cannot settle. Printed only — nothing is written, and a suggestion never
/// changes a finding.
pub async fn explain(
    config: &EkosConfig,
    cwd: &Path,
    fingerprints: &[String],
    limit: usize,
    allow_metered: bool,
    as_json: bool,
) -> Result<()> {
    let index = read_index(config, cwd).context("no integrity index — run `ekos commit` first")?;
    let store = open_store_read_only(config, cwd)?;
    let reader: &dyn KnowledgeReader = &*store;
    let picked: Vec<&Diagnostic> = index
        .report
        .diagnostics
        .iter()
        .filter(|d| xref_triage::eligible(d))
        .filter(|d| {
            fingerprints.is_empty()
                || fingerprints
                    .iter()
                    .any(|f| d.fingerprint.starts_with(f.as_str()))
        })
        .take(limit)
        .collect();
    if picked.is_empty() {
        println!("No ambiguous or dynamic findings to explain.");
        return Ok(());
    }
    // Decided once, before the first call: a local model always, anything else only when asked.
    super::migrate_logic::model_call_allowed(config, allow_metered)?;
    let llm = super::recover::build_llm_provider_strict_for(
        config,
        ekos_recovery::Task::Triage,
        &config.artifact_dir(cwd),
    )?;
    let mut results = Vec::new();
    for d in picked {
        let context = triage_context(reader, d)?;
        let outcome = if context.is_empty() {
            xref_triage::Triage::Undecided {
                rationale: "nothing compiled names it, so there is nothing to suggest from".into(),
            }
        } else {
            let user = xref_triage::prompt(d, &context);
            match llm
                .complete(&ekos_recovery::LlmRequest {
                    system: xref_triage::SYSTEM,
                    user: &user,
                    prompt_version: xref_triage::PROMPT_VERSION,
                    max_tokens: 512,
                    history: &[],
                })
                .await
            {
                Ok(answer) => xref_triage::judge(&context, &answer.content),
                Err(e) => xref_triage::Triage::Dropped {
                    why: format!("the model call failed: {e}"),
                },
            }
        };
        results.push(json!({
            "fingerprint": d.fingerprint,
            "kind": d.kind.as_str(),
            "finding": format!("{} — {}", d.target_spec.written, d.reason),
            "where": location(d),
            "triage": outcome,
        }));
    }
    if as_json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "model": llm.model_name(),
                "prompt_version": xref_triage::PROMPT_VERSION,
                "note": "Suggestions from a model, not findings or facts; each is kept only when it names an object it was shown and cites it.",
                "results": results,
            }))?
        );
        return Ok(());
    }
    println!(
        "Suggestions from {} — unconfirmed; a finding's kind and confidence are unchanged.\n",
        llm.model_name()
    );
    for r in &results {
        println!(
            "{}  {}",
            r["where"].as_str().unwrap_or(""),
            r["finding"].as_str().unwrap_or("")
        );
        let t = &r["triage"];
        match t["outcome"].as_str() {
            Some("suggested") => {
                let cited: Vec<String> = t["cites"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|c| {
                        let file = c["file"]
                            .as_str()
                            .map(|f| format!(" in {f}"))
                            .unwrap_or_default();
                        format!(
                            "{} {}{file}",
                            c["kind"].as_str().unwrap_or(""),
                            c["name"].as_str().unwrap_or("")
                        )
                    })
                    .collect();
                println!(
                    "    suggests {} `{}` (cites {}): {}",
                    t["role"].as_str().unwrap_or(""),
                    t["suggestion"].as_str().unwrap_or(""),
                    cited.join("; "),
                    t["rationale"].as_str().unwrap_or("")
                );
            }
            Some("undecided") => println!(
                "    no suggestion: {}",
                t["rationale"].as_str().unwrap_or("")
            ),
            _ => println!("    dropped: {}", t["why"].as_str().unwrap_or("")),
        }
    }
    Ok(())
}

/// The index the last commit wrote, if any.
pub fn read_index(config: &EkosConfig, cwd: &Path) -> Option<Index> {
    serde_json::from_slice(&std::fs::read(config.ekos_dir(cwd).join(INDEX)).ok()?).ok()
}

/// The fingerprints a baseline holds: a past ledger time, or a saved `--format json` report.
fn baseline_fingerprints(
    config: &EkosConfig,
    cwd: &Path,
    reader: &dyn KnowledgeReader,
    spec: &str,
) -> Result<BTreeSet<String>> {
    if let Ok(at) = DateTime::parse_from_rfc3339(spec) {
        let at = at.with_timezone(&Utc);
        let objects = reader.all_objects_at(at)?;
        let relationships = reader.all_relationships_at(at)?;
        // The manifest describes today's files, not that time's, so it is not applied.
        let report = compute(config, reader, &objects, &relationships, None, None)?;
        return Ok(report
            .diagnostics
            .into_iter()
            .map(|d| d.fingerprint)
            .collect());
    }
    let path = cwd.join(spec);
    let text = std::fs::read_to_string(&path).with_context(|| {
        format!(
            "--baseline {spec}: neither an RFC 3339 time nor a readable report file ({})",
            path.display()
        )
    })?;
    let v: Value = serde_json::from_str(&text)
        .with_context(|| format!("{}: not a JSON report", path.display()))?;
    let mut out = BTreeSet::new();
    for list in ["diagnostics", "suppressed"] {
        for d in v[list].as_array().into_iter().flatten() {
            if let Some(f) = d["fingerprint"].as_str() {
                out.insert(f.to_string());
            }
        }
    }
    Ok(out)
}

/// The findings a run shows, and the ones its rules hide.
struct Selection {
    shown: Vec<Diagnostic>,
    suppressed: Vec<(Diagnostic, String)>,
    below_confidence: usize,
    out_of_scope: usize,
    in_baseline: usize,
}

fn select(
    report: &Report,
    args: &RefsArgs,
    min_confidence: f64,
    rules: &[IgnoreRule],
    baseline: Option<&BTreeSet<String>>,
) -> Selection {
    let mut s = Selection {
        shown: Vec::new(),
        suppressed: Vec::new(),
        below_confidence: 0,
        out_of_scope: 0,
        in_baseline: 0,
    };
    for d in &report.diagnostics {
        let reach_kind = matches!(
            d.kind,
            DiagnosticKind::UnreachableSymbol | DiagnosticKind::DynamicReferenceUnknown
        );
        let in_scope = (args.reachability || !reach_kind)
            && match args.scope {
                Scope::All => true,
                Scope::CrossSystem => d.cross_system,
                Scope::Code => !d.cross_system,
            };
        if !in_scope {
            s.out_of_scope += 1;
        } else if d.confidence < min_confidence {
            s.below_confidence += 1;
        } else if let Some(r) = rules.iter().find(|r| r.matches(d)) {
            s.suppressed.push((d.clone(), r.reason.clone()));
        } else if baseline.is_some_and(|b| b.contains(&d.fingerprint)) {
            s.in_baseline += 1;
        } else {
            s.shown.push(d.clone());
        }
    }
    s
}

/// `ekos check refs`. Exits 1 when a shown finding is at or above `--fail-on`.
pub fn refs(config: &EkosConfig, cwd: &Path, args: &RefsArgs) -> Result<()> {
    let store = open_store_read_only(config, cwd)?;
    let reader: &dyn KnowledgeReader = &*store;
    let mut computed_at = None;
    let report = if args.from_index {
        let index = read_index(config, cwd).with_context(|| {
            format!(
                "no integrity index at {} — run `ekos commit`, or drop --from-index",
                config.ekos_dir(cwd).join(INDEX).display()
            )
        })?;
        if index.report.version != xref::XREF_VERSION {
            bail!(
                "the integrity index was written by an older check (version {}); run `ekos commit` \
                 again, or drop --from-index",
                index.report.version
            );
        }
        computed_at = Some(index.computed_at);
        index.report
    } else {
        let objects = reader.all_objects()?;
        let relationships = reader.all_relationships()?;
        let live = committed_files(config, cwd);
        let unobserved = live.as_ref().map(|l| unobserved_files(config, cwd, l));
        compute(
            config,
            reader,
            &objects,
            &relationships,
            live.as_ref(),
            unobserved.as_ref(),
        )?
    };
    let rules = load_ignores(&ignore_path(config, cwd))?;
    let min_confidence = args.min_confidence.unwrap_or(config.xref.min_confidence);
    if !(0.0..=1.0).contains(&min_confidence) {
        bail!("--min-confidence must be between 0 and 1, got {min_confidence}");
    }
    let baseline = match &args.baseline {
        Some(spec) => Some(baseline_fingerprints(config, cwd, reader, spec)?),
        None => None,
    };
    let sel = select(&report, args, min_confidence, &rules, baseline.as_ref());

    match args.format {
        Format::Table => print_table(&report, &sel, args, min_confidence),
        Format::Json => {
            let mut v = json_report(&report, &sel, args);
            v["computed_at"] = json!(computed_at.unwrap_or_else(Utc::now));
            v["from_index"] = json!(computed_at.is_some());
            println!("{}", serde_json::to_string_pretty(&v)?);
        }
        Format::Sarif => println!("{}", serde_json::to_string_pretty(&sarif(&sel.shown))?),
    }

    if let Some(threshold) = args.fail_on
        && sel.shown.iter().any(|d| d.severity >= threshold)
    {
        std::process::exit(1);
    }
    Ok(())
}

fn json_report(report: &Report, sel: &Selection, args: &RefsArgs) -> Value {
    let mut v = json!({
        "version": report.version,
        "metrics": report.metrics,
        "diagnostics": sel.shown,
        "filtered": {
            "below_confidence": sel.below_confidence,
            "out_of_scope": sel.out_of_scope,
            "in_baseline": sel.in_baseline,
            "suppressed": sel.suppressed.len(),
        },
    });
    if args.show_suppressed {
        v["suppressed"] = sel
            .suppressed
            .iter()
            .map(|(d, reason)| {
                let mut d = serde_json::to_value(d).unwrap_or(Value::Null);
                d["suppressed_because"] = json!(reason);
                d
            })
            .collect();
    }
    v
}

fn rate(r: Option<f64>) -> String {
    r.map_or_else(|| "n/a".to_string(), |r| format!("{:.1}%", r * 100.0))
}

fn location(d: &Diagnostic) -> String {
    match (&d.source.file, d.source.line) {
        (Some(f), Some(l)) => format!("{f}:{l}"),
        (Some(f), None) => f.clone(),
        _ => format!("{} {}", d.source.object_kind, d.source.object_name),
    }
}

fn print_table(report: &Report, sel: &Selection, args: &RefsArgs, min_confidence: f64) {
    let m = &report.metrics;
    println!(
        "References: {} checked, {} resolved, {} unresolved, {} ambiguous — resolution {}",
        m.all.resolved + m.all.unresolved + m.all.ambiguous,
        m.all.resolved,
        m.all.unresolved,
        m.all.ambiguous,
        rate(m.resolution_rate)
    );
    println!(
        "Cross-system: {} checked — resolution {}",
        m.cross_system.resolved + m.cross_system.unresolved + m.cross_system.ambiguous,
        rate(m.cross_system_resolution_rate)
    );
    for (extractor, c) in &m.by_extractor {
        println!(
            "  {extractor:<20} {:>6} resolved {:>5} unresolved {:>5} ambiguous  {}",
            c.resolved,
            c.unresolved,
            c.ambiguous,
            rate(c.resolution_rate())
        );
    }
    println!(
        "Not checked: {} dynamic names, {} built-in or system-catalog names, {} third-party imports, \
         {} from files no longer observed",
        m.dynamic, m.system, m.external, m.stale_sources
    );
    let orphans: Vec<String> = m.orphans.iter().map(|(k, n)| format!("{n} {k}")).collect();
    println!(
        "Cycles: {} between modules; orphans: {}",
        m.cycles,
        if orphans.is_empty() {
            "none".to_string()
        } else {
            orphans.join(", ")
        }
    );
    for (family, r) in &m.reachability {
        match &r.unsupported_because {
            Some(why) => println!(
                "  reachability {family:<11} unsupported ({} symbols): {why}",
                r.symbols
            ),
            None => println!(
                "  reachability {family:<11} {} symbols: {} reachable ({}), {} unreachable, {} dynamic ({} dynamic share)",
                r.symbols,
                r.reachable,
                rate(r.reachability_coverage),
                r.unreachable,
                r.dynamic,
                rate(r.dynamic_share)
            ),
        }
    }
    println!();
    if sel.shown.is_empty() {
        println!("No findings.");
    }
    for d in &sel.shown {
        println!(
            "{:<7} {:.1} {:<20} {}  {} `{}`{}",
            d.severity.as_str(),
            d.confidence,
            d.kind.as_str(),
            location(d),
            d.source.access,
            d.target_spec.written,
            if d.coverage_gap {
                "  [coverage gap]"
            } else {
                ""
            }
        );
        println!(
            "        {} — from {} {} ({})",
            d.reason, d.source.object_kind, d.source.object_name, d.source.extractor
        );
        if !d.candidates.is_empty() {
            let names: Vec<&str> = d.candidates.iter().map(|c| c.name.as_str()).collect();
            let label = if d.kind == DiagnosticKind::AmbiguousReference {
                "candidates"
            } else {
                "closest"
            };
            println!("        {label}: {}", names.join(", "));
        }
    }
    if args.show_suppressed {
        for (d, reason) in &sel.suppressed {
            println!(
                "suppressed  {}  `{}` — {reason}",
                location(d),
                d.target_spec.written
            );
        }
    }
    let mut hidden = Vec::new();
    if sel.below_confidence > 0 {
        hidden.push(format!(
            "{} below confidence {min_confidence}",
            sel.below_confidence
        ));
    }
    if sel.out_of_scope > 0 {
        hidden.push(format!("{} out of scope", sel.out_of_scope));
    }
    if !sel.suppressed.is_empty() {
        hidden.push(format!("{} suppressed", sel.suppressed.len()));
    }
    if sel.in_baseline > 0 {
        hidden.push(format!("{} already in the baseline", sel.in_baseline));
    }
    if !hidden.is_empty() {
        println!("\nNot shown: {}.", hidden.join(", "));
    }
}

/// SARIF 2.1.0: one rule per kind; the location is the reference's own file and line.
fn sarif(diagnostics: &[Diagnostic]) -> Value {
    let kinds: BTreeSet<DiagnosticKind> = diagnostics.iter().map(|d| d.kind).collect();
    let rules: Vec<Value> = kinds
        .iter()
        .map(|k| {
            let text = match k {
                DiagnosticKind::UnresolvedReference => {
                    "A reference names an object no compiled source defines"
                }
                DiagnosticKind::AmbiguousReference => "A reference names several compiled objects",
                DiagnosticKind::ReferenceCycle => "Modules depend on each other in a cycle",
                DiagnosticKind::OrphanArtifact => "Nothing compiled uses this object",
                DiagnosticKind::UnreachableSymbol => "No entry point reaches this symbol",
                DiagnosticKind::DynamicReferenceUnknown => {
                    "Reachability cannot be proven: the symbol may be called dynamically"
                }
            };
            json!({"id": k.as_str(), "shortDescription": {"text": text}})
        })
        .collect();
    let results: Vec<Value> = diagnostics
        .iter()
        .map(|d| {
            let level = match d.severity {
                Severity::Error => "error",
                Severity::Warning => "warning",
                Severity::Info => "note",
            };
            let mut physical = json!({});
            if let Some(f) = &d.source.file {
                physical["artifactLocation"] = json!({"uri": f});
                if let Some(l) = d.source.line {
                    physical["region"] = json!({"startLine": l});
                }
            }
            let locations = if d.source.file.is_some() {
                json!([{"physicalLocation": physical}])
            } else {
                json!([])
            };
            json!({
                "ruleId": d.kind.as_str(),
                "level": level,
                "message": {"text": format!(
                    "{} {} `{}`: {}",
                    d.source.object_name, d.source.access, d.target_spec.written, d.reason
                )},
                "locations": locations,
                "partialFingerprints": {"ekosXref/v1": d.fingerprint},
                "properties": {
                    "confidence": d.confidence,
                    "target": d.target_spec.normalized,
                    "extractor": d.source.extractor,
                    "crossSystem": d.cross_system,
                    "coverageGap": d.coverage_gap,
                    "candidates": d.candidates.iter().map(|c| &c.name).collect::<Vec<_>>(),
                },
            })
        })
        .collect();
    json!({
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "version": "2.1.0",
        "runs": [{
            "tool": {"driver": {
                "name": "ekos-xref",
                "version": env!("CARGO_PKG_VERSION"),
                "informationUri": "https://github.com/alexeyban/EKOS",
                "rules": rules,
            }},
            "results": results,
        }],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_kir::{KirObject, ObjectKind};

    fn report() -> Report {
        let objects = vec![
            KirObject::new("account", ObjectKind::Table),
            KirObject::new("post", ObjectKind::Custom("Procedure".into()))
                .with_property("source_path", json!("sql/a.sql"))
                .with_property("line", json!(3))
                .with_property("reads", json!(["gone", "tmp_x"])),
            KirObject::new("doIt", ObjectKind::Custom("JavaMethod".into())).with_property(
                "sql",
                json!([{"line": 9, "status": "parsed", "reads": ["c_missing"]}]),
            ),
        ];
        xref::check(&objects, &xref::Options::default())
    }

    fn args(scope: Scope) -> RefsArgs {
        RefsArgs {
            scope,
            min_confidence: None,
            format: Format::Json,
            fail_on: None,
            baseline: None,
            show_suppressed: false,
            reachability: false,
            from_index: false,
        }
    }

    #[test]
    fn globs_match_like_shell_stars() {
        assert!(glob_match("tmp_*", "tmp_lines"));
        assert!(glob_match("sql/*/old_*.sql", "sql/a/old_x.sql"));
        assert!(!glob_match("tmp_*", "atmp_lines"));
        assert!(glob_match("exact", "exact"));
    }

    #[test]
    fn scope_confidence_suppression_and_baseline_each_hide_and_count() {
        let r = report();
        assert_eq!(r.diagnostics.len(), 3);
        let all = select(&r, &args(Scope::All), 0.0, &[], None);
        assert_eq!(all.shown.len(), 3);
        let cross = select(&r, &args(Scope::CrossSystem), 0.0, &[], None);
        assert_eq!(cross.shown.len(), 1);
        assert_eq!(cross.out_of_scope, 2);
        let confident = select(&r, &args(Scope::All), 0.5, &[], None);
        assert_eq!(confident.below_confidence, 1); // the work table
        let rule = IgnoreRule {
            fingerprint: None,
            target: Some("GONE".into()),
            source: None,
            reason: "dropped on purpose".into(),
        };
        let s = select(&r, &args(Scope::All), 0.0, &[rule], None);
        assert_eq!(s.suppressed.len(), 1);
        let base: BTreeSet<String> = [r.diagnostics[0].fingerprint.clone()].into();
        let b = select(&r, &args(Scope::All), 0.0, &[], Some(&base));
        assert_eq!(b.in_baseline, 1);
        assert_eq!(b.shown.len(), 2);
    }

    #[test]
    fn a_suppression_without_a_reason_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("xref-ignore.toml");
        std::fs::write(&p, "[[ignore]]\ntarget = \"tmp_*\"\n").unwrap();
        assert!(load_ignores(&p).unwrap_err().to_string().contains("reason"));
        std::fs::write(&p, "[[ignore]]\nreason = \"x\"\n").unwrap();
        assert!(load_ignores(&p).is_err());
        std::fs::write(
            &p,
            "[[ignore]]\ntarget = \"tmp_*\"\nreason = \"work tables\"\n",
        )
        .unwrap();
        assert_eq!(load_ignores(&p).unwrap().len(), 1);
        assert!(
            load_ignores(&dir.path().join("absent.toml"))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn sarif_carries_level_location_and_fingerprint() {
        let r = report();
        let v = sarif(&r.diagnostics);
        assert_eq!(v["version"], "2.1.0");
        let results = v["runs"][0]["results"].as_array().unwrap();
        let first = &results[0];
        assert_eq!(first["level"], "error");
        assert_eq!(
            first["partialFingerprints"]["ekosXref/v1"],
            json!(r.diagnostics[0].fingerprint)
        );
        let with_file = results
            .iter()
            .find(|x| {
                x["locations"][0]["physicalLocation"]["artifactLocation"]["uri"] == "sql/a.sql"
            })
            .unwrap();
        assert_eq!(
            with_file["locations"][0]["physicalLocation"]["region"]["startLine"],
            3
        );
    }

    #[test]
    fn unknown_severity_kinds_are_refused() {
        let mut c = EkosConfig::default();
        c.xref.severity.insert("Nope".into(), "info".into());
        assert!(severity_overrides(&c).is_err());
        let mut c = EkosConfig::default();
        c.xref
            .severity
            .insert("AmbiguousReference".into(), "loud".into());
        assert!(severity_overrides(&c).is_err());
    }
}
