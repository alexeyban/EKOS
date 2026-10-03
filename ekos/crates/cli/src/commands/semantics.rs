//! `ekos semantics …` and the `[semantics]` step of `ekos commit` — RFC 0170.
//!
//! The commit step reads the committed ledger, runs `ekos_semantic::business_semantics::synthesize`
//! with a `git blame` [`RationaleSource`] when the workspace is a git repository, and appends the
//! resulting hypotheses. It also writes `.ekos/semantics/current.json`: the ids the latest run
//! derived. The ledger is append-only, so an item whose traces disappeared from the sources is
//! still in it; the manifest is how `list`/`gaps`/`export linkml` show only what the current
//! sources support (RFC 0170 Phase 2 turns this into a `needs_review` status).
//!
//! Read commands never write. Nothing here confirms anything: every item is a hypothesis.

use super::store::{open_store, open_store_read_only};
use anyhow::{Context, Result};
use ekos_compiler_core::EkosConfig;
use ekos_kir::{KirGraph, KirId, KirObject};
use ekos_ledger::KnowledgeStore;
use ekos_semantic::business_semantics::{
    self, BlameHit, CONCEPT, CONFLICT, CONSTRAINT, ENUM_MEANING, GAP, KINDS, RATIONALE,
    RationaleSource, SemanticsConfig, SemanticsStats,
};
use ekos_semantic::semantics_review::{self, Decision};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::process::Command;

// ── Commit step ──────────────────────────────────────────────────────────────────────────────

fn manifest_path(config: &EkosConfig, cwd: &Path) -> PathBuf {
    config.ekos_dir(cwd).join("semantics").join("current.json")
}

/// RFC 0170: synthesize and append business-semantics hypotheses. `None` when `[semantics]` is off.
pub async fn commit_step(
    config: &EkosConfig,
    cwd: &Path,
    ledger: &dyn KnowledgeStore,
    yes: bool,
) -> Result<Option<(SemanticsStats, usize)>> {
    if !config.semantics.enabled {
        return Ok(None);
    }
    let graph = KirGraph {
        objects: ledger.all_objects()?,
        relationships: ledger.all_relationships()?,
        events: Vec::new(),
        evidence: Vec::new(),
    };
    let cfg = SemanticsConfig {
        min_sites: config.semantics.min_sites.max(1),
        max_enum_values: config.semantics.max_enum_values,
        ontology: load_vocabulary(config, cwd)?,
    };
    let blame = (config.semantics.rationale)
        .then(|| GitBlame::new(cwd))
        .flatten();
    let mut out = business_semantics::synthesize(
        &graph,
        &cfg,
        blame.as_ref().map(|b| b as &dyn RationaleSource),
    );
    // RFC 0170 Phase 2: a human decision holds while what it was about is unchanged.
    let current: HashMap<KirId, &KirObject> = graph
        .objects
        .iter()
        .filter(|o| kind_of(o).is_some())
        .map(|o| (o.id, o))
        .collect();
    for o in &mut out.objects {
        semantics_review::carry_forward(o, current.get(&o.id).copied());
    }
    // Opt-in, cited plain-language text for undocumented concepts (after carry-forward, so an
    // expert's description already present wins and nothing is asked for it).
    if config.semantics.llm_definitions {
        describe_with_llm(config, cwd, &mut out, yes).await?;
    }
    // A reviewed item the sources no longer support is not silently dropped: it is flagged.
    let fresh: std::collections::HashSet<KirId> = out.objects.iter().map(|o| o.id).collect();
    let mut stale: Vec<KirObject> = current
        .values()
        .filter(|o| !fresh.contains(&o.id))
        .filter(|o| semantics_review::is_reviewed(o) && s(o, "status") != "rejected")
        .map(|o| semantics_review::stale_version(o).unwrap_or_else(|| (*o).clone()))
        .collect();
    stale.sort_by(|a, b| a.name.cmp(&b.name));
    for ev in &out.evidence {
        ledger.append_evidence(ev)?;
    }
    // New ledger versions written: 0 on a re-run over unchanged sources (deterministic ids).
    let mut written = 0usize;
    for o in out.objects.iter().chain(&stale) {
        let w = ledger.append_object(o)?;
        written += usize::from(w);
    }
    for r in &out.relationships {
        written += usize::from(ledger.append_relationship(r)?);
    }
    let ids: Vec<String> = out
        .objects
        .iter()
        .chain(&stale)
        .map(|o| o.id.to_string())
        .collect();
    let path = manifest_path(config, cwd);
    std::fs::create_dir_all(path.parent().unwrap())?;
    std::fs::write(
        &path,
        serde_json::to_vec_pretty(&json!({
            "rfc": "0170",
            "stats": stats_json(&out.stats),
            "rationale": blame.is_some(),
            "stale_reviewed": stale.len(),
            "ids": ids,
        }))?,
    )
    .with_context(|| format!("writing {}", path.display()))?;
    Ok(Some((out.stats, written)))
}

/// RFC 0170 Phase 4: the user's ontology vocabulary (`[semantics] ontology`), or an empty one.
pub fn load_vocabulary(
    config: &EkosConfig,
    cwd: &Path,
) -> Result<ekos_semantic::ontology::Vocabulary> {
    let Some(rel) = &config.semantics.ontology else {
        return Ok(Default::default());
    };
    let path = cwd.join(rel);
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("[semantics] ontology: reading {}", path.display()))?;
    serde_yaml::from_str(&text)
        .with_context(|| format!("[semantics] ontology: parsing {}", path.display()))
}

/// RFC 0170 Phase 3: `llm_definition` on undocumented concepts, cite-or-drop
/// (`ekos_recovery::semantics_llm`). Asks before a metered provider runs, like `[llm-description]`.
async fn describe_with_llm(
    config: &EkosConfig,
    cwd: &Path,
    out: &mut business_semantics::SemanticsOutput,
    yes: bool,
) -> Result<()> {
    let undocumented = out
        .objects
        .iter()
        .filter(|o| kind_of(o) == Some(CONCEPT))
        .filter(|o| s(o, "expert_description").is_empty() && s(o, "description").is_empty())
        .count();
    if undocumented == 0 {
        return Ok(());
    }
    let n = undocumented.min(config.semantics.llm_max_definitions);
    let local = config.llm.provider.as_deref() == Some("ollama");
    if !local {
        println!(
            "[semantics] llm-definitions: up to {n} LLM call(s) to {} for concept text (cached answers are free).",
            config.llm.provider.as_deref().unwrap_or("anthropic")
        );
        if !super::commit::confirm_description_spend(yes)? {
            println!("  skipped.");
            return Ok(());
        }
    }
    let provider = super::commit::select_llm_provider_for_description(
        config,
        &config.ekos_dir(cwd).join("artifacts"),
    )?;
    let evidence: HashMap<KirId, ekos_kir::KirEvidence> =
        out.evidence.iter().map(|e| (e.id, e.clone())).collect();
    let mut concepts: Vec<KirObject> = Vec::new();
    let mut slots = Vec::new();
    for (i, o) in out.objects.iter().enumerate() {
        if kind_of(o) == Some(CONCEPT) {
            slots.push(i);
            concepts.push(o.clone());
        }
    }
    // What EKOS already knows the concept's codes mean is evidence too: the model must not have to
    // guess that `category = 'Q'` is equity when a column comment says so.
    let meanings: Vec<(String, String, String, ekos_kir::KirEvidence)> = out
        .objects
        .iter()
        .filter(|o| kind_of(o) == Some(ENUM_MEANING))
        .filter_map(|o| {
            let label = match s(o, "expert_label") {
                l if l.is_empty() => s(o, "label"),
                l => l,
            };
            if label.is_empty() {
                return None;
            }
            let (table, column, value) = (s(o, "table"), s(o, "column"), s(o, "value"));
            let source = p(o, "meanings")
                .as_array()
                .and_then(|a| a.first())
                .cloned()
                .unwrap_or_default();
            let ev = ekos_kir::KirEvidence::new(
                ekos_kir::SourceLocation {
                    path: source["path"].as_str().unwrap_or_default().to_string(),
                    line: source["line"].as_u64().map(|l| l as u32),
                    column: None,
                },
                format!(
                    "{table}.{column} = {value} means \"{label}\" ({})",
                    source["source"].as_str().unwrap_or("recovered")
                ),
            );
            Some((table, column, value, ev))
        })
        .collect();
    let stats = ekos_recovery::semantics_llm::describe_concepts(
        provider.as_ref(),
        &mut concepts,
        |c| {
            let mut evs: Vec<ekos_kir::KirEvidence> = c
                .evidence
                .iter()
                .filter_map(|id| evidence.get(id).cloned())
                .take(8)
                .collect();
            let def = s(c, "definition");
            evs.extend(
                meanings
                    .iter()
                    .filter(|(t, col, v, _)| {
                        def.contains(&format!("{t}.{col} ")) && def.contains(v.as_str())
                    })
                    .map(|(_, _, _, e)| e.clone())
                    .take(6),
            );
            evs
        },
        config.semantics.llm_max_definitions,
    )
    .await;
    for (slot, c) in slots.into_iter().zip(concepts) {
        out.objects[slot] = c;
    }
    println!(
        "  concept text: {} described, {} uncited sentence(s) dropped, {} error(s)",
        stats.described, stats.sentences_dropped, stats.errors
    );
    Ok(())
}

fn stats_json(s: &SemanticsStats) -> Value {
    json!({
        "sites": s.sites, "sites_resolved": s.sites_resolved, "concepts": s.concepts,
        "concepts_from_views": s.concepts_from_views, "coded_columns": s.coded_columns,
        "enum_values": s.enum_values, "enum_values_explained": s.enum_values_explained,
        "key_like_columns": s.key_like_columns, "constraints": s.constraints, "gaps": s.gaps,
        "conflicts": s.conflicts,
        "mapping_suggestions": s.mapping_suggestions,
        "rationale_links": s.rationale_links,
    })
}

/// The one-line summary `ekos commit` prints.
pub fn summary_line(s: &SemanticsStats, written: usize) -> String {
    format!(
        "{} concept(s), {} coded value(s) in {} column(s) ({} explained), {} constraint(s), {} gap(s), {} conflict(s), {} rationale link(s), {} mapping suggestion(s) — all hypotheses ({} of {} predicate sites resolved; {written} new ledger entries)",
        s.concepts,
        s.enum_values,
        s.coded_columns,
        s.enum_values_explained,
        s.constraints,
        s.gaps,
        s.conflicts,
        s.rationale_links,
        s.mapping_suggestions,
        s.sites_resolved,
        s.sites
    )
}

// ── git blame ────────────────────────────────────────────────────────────────────────────────

/// `git blame --porcelain` over the workspace's files. Evidence paths are workspace-relative
/// (`recover.rs` keys SQL passes by `path.strip_prefix(cwd)`), so a path resolves against `cwd`;
/// git runs in the file's own directory, so an observe path holding a different repository works.
pub struct GitBlame {
    cwd: PathBuf,
}

impl GitBlame {
    /// `None` when `git` is unavailable or `cwd` is not inside a repository.
    pub fn new(cwd: &Path) -> Option<Self> {
        let ok = Command::new("git")
            .arg("-C")
            .arg(cwd)
            .args(["rev-parse", "--is-inside-work-tree"])
            .output()
            .ok()?
            .status
            .success();
        ok.then(|| Self {
            cwd: cwd.to_path_buf(),
        })
    }
}

impl RationaleSource for GitBlame {
    fn blame(&self, path: &str, lines: &[u32]) -> Vec<BlameHit> {
        // A project-qualified key (`proj:path`) is not a file path; nothing to blame.
        let file = self.cwd.join(path);
        let (Some(dir), Some(name)) = (file.parent(), file.file_name()) else {
            return Vec::new();
        };
        if !file.is_file() || lines.is_empty() {
            return Vec::new();
        }
        let mut cmd = Command::new("git");
        // `-w`: a whitespace-only commit ("remove trailing spaces") explains nothing.
        cmd.arg("-C").arg(dir).args(["blame", "--porcelain", "-w"]);
        for l in lines {
            cmd.arg("-L").arg(format!("{l},{l}"));
        }
        cmd.arg("--").arg(name);
        match cmd.output() {
            Ok(o) if o.status.success() => parse_porcelain(&String::from_utf8_lossy(&o.stdout)),
            _ => Vec::new(),
        }
    }
}

/// Parse `git blame --porcelain`: a header `<sha> <orig> <final> [<n>]` per line, with the
/// commit's `author`/`author-time`/`summary` only the first time that commit appears.
pub fn parse_porcelain(out: &str) -> Vec<BlameHit> {
    #[derive(Default, Clone)]
    struct Info {
        author: String,
        time: i64,
        summary: String,
    }
    let mut info: HashMap<String, Info> = HashMap::new();
    let mut entries: Vec<(String, u32)> = Vec::new();
    let mut current: Option<String> = None;
    for line in out.lines() {
        if line.starts_with('\t') {
            current = None;
            continue;
        }
        let mut words = line.split(' ');
        let first = words.next().unwrap_or_default();
        if current.is_none() && first.len() == 40 && first.chars().all(|c| c.is_ascii_hexdigit()) {
            let final_line = words.nth(1).and_then(|w| w.parse().ok()).unwrap_or(0);
            entries.push((first.to_string(), final_line));
            info.entry(first.to_string()).or_default();
            current = Some(first.to_string());
            continue;
        }
        let Some(sha) = &current else { continue };
        let rest = line.split_once(' ').map(|(_, r)| r).unwrap_or_default();
        let i = info.entry(sha.clone()).or_default();
        match first {
            "author" => i.author = rest.to_string(),
            "author-time" => i.time = rest.parse().unwrap_or(0),
            "summary" => i.summary = rest.to_string(),
            _ => {}
        }
    }
    entries
        .into_iter()
        // Uncommitted lines blame to the all-zero sha: no rationale to cite.
        .filter(|(sha, _)| sha.chars().any(|c| c != '0'))
        .map(|(sha, line)| {
            let i = info.get(&sha).cloned().unwrap_or_default();
            BlameHit {
                line,
                date: chrono::DateTime::from_timestamp(i.time, 0)
                    .map(|d| d.to_rfc3339())
                    .unwrap_or_default(),
                sha,
                summary: i.summary,
                author: i.author,
            }
        })
        .collect()
}

// ── Reading ──────────────────────────────────────────────────────────────────────────────────

fn kind_of(o: &KirObject) -> Option<&'static str> {
    business_semantics::kind_name(o)
}

/// The RFC 0170 items the latest synthesis run derived — every one ever written when no manifest
/// exists yet (a ledger committed before the manifest, or by another tool). Opened read-only, so
/// it runs beside a serving `ekos mcp serve` or the web console; see [`current_items_for_write`].
pub fn current_items(
    config: &EkosConfig,
    cwd: &Path,
) -> Result<(Box<dyn KnowledgeStore>, Vec<KirObject>)> {
    items_from(config, cwd, false)
}

/// [`current_items`] on a writable store, for a review that appends a decision.
pub fn current_items_for_write(
    config: &EkosConfig,
    cwd: &Path,
) -> Result<(Box<dyn KnowledgeStore>, Vec<KirObject>)> {
    items_from(config, cwd, true)
}

/// The current RFC 0170 items in an already-open store (the MCP server's cached read-only handle,
/// or one of the openers above), sorted by kind then name.
pub fn items_in(
    config: &EkosConfig,
    cwd: &Path,
    ledger: &dyn KnowledgeStore,
) -> Result<Vec<KirObject>> {
    let current: Option<BTreeSet<String>> = std::fs::read(manifest_path(config, cwd))
        .ok()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .and_then(|v| {
            v["ids"].as_array().map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
        });
    let mut items: Vec<KirObject> = ledger
        .all_objects()?
        .into_iter()
        .filter(|o| kind_of(o).is_some())
        .filter(|o| {
            current
                .as_ref()
                .is_none_or(|c| c.contains(&o.id.to_string()))
        })
        .collect();
    items.sort_by(|a, b| (kind_rank(a), a.name.as_str()).cmp(&(kind_rank(b), b.name.as_str())));
    Ok(items)
}

fn items_from(
    config: &EkosConfig,
    cwd: &Path,
    writable: bool,
) -> Result<(Box<dyn KnowledgeStore>, Vec<KirObject>)> {
    let opened = if writable {
        open_store(config, cwd)
    } else {
        open_store_read_only(config, cwd)
    };
    let ledger = opened.map_err(|e| {
        anyhow::anyhow!("{e}\nRun the pipeline with `[semantics] enabled = true` first.")
    })?;
    let items = items_in(config, cwd, &*ledger)?;
    Ok((ledger, items))
}

fn kind_rank(o: &KirObject) -> usize {
    kind_of(o)
        .and_then(|k| KINDS.iter().position(|x| *x == k))
        .unwrap_or(99)
}

fn kind_filter(kind: &str) -> Result<&'static str> {
    Ok(match kind.to_ascii_lowercase().as_str() {
        "concept" | "concepts" | "businessconcept" => CONCEPT,
        "enum" | "enums" | "enummeaning" => ENUM_MEANING,
        "constraint" | "constraints" | "constraintcandidate" => CONSTRAINT,
        "gap" | "gaps" | "semanticgap" => GAP,
        "conflict" | "conflicts" | "conceptconflict" => CONFLICT,
        "rationale" | "rationalelink" => RATIONALE,
        other => anyhow::bail!(
            "unknown kind `{other}` — expected concept, enum, constraint, gap, conflict or rationale"
        ),
    })
}

fn p<'a>(o: &'a KirObject, k: &str) -> &'a Value {
    o.properties.get(k).unwrap_or(&Value::Null)
}

fn s(o: &KirObject, k: &str) -> String {
    match p(o, k) {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        v => v.to_string(),
    }
}

/// One line describing an item.
/// The name a human gave the item, else the recovered one (with the recovered one alongside).
fn display_name(o: &KirObject) -> String {
    match s(o, "expert_name") {
        n if n.is_empty() => o.name.clone(),
        n => format!("{n} (recovered as {})", o.name),
    }
}

fn line_for(o: &KirObject) -> String {
    let status = s(o, "status");
    match kind_of(o).unwrap_or_default() {
        k if k == CONCEPT => format!(
            "[{status}] {}  — {}  ({} site(s), {}, confidence {})",
            display_name(o),
            s(o, "definition"),
            s(o, "sites"),
            s(o, "origin"),
            s(o, "confidence")
        ),
        k if k == ENUM_MEANING => {
            let label = match (s(o, "expert_label"), s(o, "label")) {
                (e, _) if !e.is_empty() => format!("{e} (expert)"),
                (_, l) if !l.is_empty() => l,
                _ => "?".into(),
            };
            let sources: Vec<String> = p(o, "meanings")
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|m| m["source"].as_str().map(str::to_string))
                        .collect::<BTreeSet<_>>()
                        .into_iter()
                        .collect()
                })
                .unwrap_or_default();
            format!(
                "[{status}] {}  → {label}  ({} usage site(s){})",
                o.name,
                s(o, "usage_sites"),
                if sources.is_empty() {
                    String::new()
                } else {
                    format!("; from {}", sources.join(", "))
                }
            )
        }
        k if k == CONSTRAINT => format!("[{status}] {}  ({})", o.name, s(o, "constraint_type")),
        k if k == GAP => format!("[{}] {}", s(o, "gap_type"), s(o, "question")),
        k if k == CONFLICT => format!("[{status}] {}", s(o, "question")),
        _ => format!(
            "[{status}] {}  ({}, {})",
            o.name,
            s(o, "path"),
            s(o, "date")
        ),
    }
}

/// The one current item `target` names — by id, recovered name or expert name.
fn find_item<'a>(items: &'a [KirObject], target: &str) -> Result<&'a KirObject> {
    let matches: Vec<&KirObject> = items
        .iter()
        .filter(|o| o.id.to_string() == target || o.name == target || s(o, "expert_name") == target)
        .collect();
    match matches.as_slice() {
        [one] => Ok(*one),
        [] => {
            let near: Vec<&str> = items
                .iter()
                .filter(|o| o.name.to_lowercase().contains(&target.to_lowercase()))
                .map(|o| o.name.as_str())
                .take(10)
                .collect();
            anyhow::bail!(
                "no business-semantics item named `{target}`{}",
                if near.is_empty() {
                    String::new()
                } else {
                    format!(" — did you mean: {}", near.join(", "))
                }
            )
        }
        many => anyhow::bail!(
            "`{target}` names {} items; pass an id: {}",
            many.len(),
            many.iter()
                .map(|o| o.id.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// `ekos semantics confirm|reject|edit` — RFC 0170 Phase 2. **Human-only:** the CLI is the only
/// caller of `semantics_review::apply_review`; `commands/mcp.rs` must never reach it (a test below
/// scans for it). Writes one new version of the item, attributed to `by`.
pub fn review(
    config: &EkosConfig,
    cwd: &Path,
    target: &str,
    decision: Decision,
    by: Option<String>,
    note: Option<String>,
) -> Result<()> {
    review_many(config, cwd, &[target.to_string()], decision, by, note)
}

/// Several decisions of one kind (`confirm`/`reject` with many targets), **all or nothing**:
/// every target is resolved and every transition checked before anything is written.
pub fn review_many(
    config: &EkosConfig,
    cwd: &Path,
    targets: &[String],
    decision: Decision,
    by: Option<String>,
    note: Option<String>,
) -> Result<()> {
    let by = by
        .or_else(|| std::env::var("USER").ok())
        .filter(|b| !b.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("who is reviewing? Pass --as <you>"))?;
    if targets.is_empty() {
        anyhow::bail!("nothing to review");
    }
    let (ledger, items) = current_items_for_write(config, cwd)?;
    let at = chrono::Utc::now().to_rfc3339();
    let mut next = Vec::new();
    let mut errors = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for target in targets {
        match find_item(&items, target) {
            Ok(current) if !seen.insert(current.id) => {
                errors.push(format!("{target}: named twice"));
            }
            Ok(current) => {
                match semantics_review::apply_review(current, &decision, &by, &at, note.as_deref())
                {
                    Ok(o) => next.push(o),
                    Err(e) => errors.push(format!("{target}: {e}")),
                }
            }
            Err(e) => errors.push(format!("{target}: {e}")),
        }
    }
    if !errors.is_empty() {
        anyhow::bail!(
            "{} of {} target(s) cannot be reviewed; nothing was written:\n  {}",
            errors.len(),
            targets.len(),
            errors.join("\n  ")
        );
    }
    ledger.set_write_context(Some(ekos_ledger::provenance::WriteContext {
        run_id: ekos_ledger::provenance::new_run_id(),
        stage: "semantics-review".into(),
        source_artifact_id: None,
    }));
    for o in &next {
        ledger.append_object(o)?;
        println!(
            "{} {} — {} by {by}{}",
            kind_of(o).unwrap_or_default(),
            o.name,
            s(o, "status"),
            note.as_ref().map(|n| format!(" ({n})")).unwrap_or_default()
        );
    }
    if next.len() > 1 {
        println!("{} item(s) reviewed.", next.len());
    }
    Ok(())
}

/// `ekos semantics list`.
pub fn list(
    config: &EkosConfig,
    cwd: &Path,
    kind: Option<&str>,
    status: Option<&str>,
    json_out: bool,
) -> Result<()> {
    let want = kind.map(kind_filter).transpose()?;
    if let Some(st) = status
        && !["hypothesis", "confirmed", "rejected", "needs_review"].contains(&st)
    {
        anyhow::bail!("unknown status `{st}` — hypothesis, confirmed, rejected or needs_review");
    }
    let (_, items) = current_items(config, cwd)?;
    let items: Vec<&KirObject> = items
        .iter()
        .filter(|o| want.is_none_or(|w| kind_of(o) == Some(w)))
        .filter(|o| status.is_none_or(|st| s(o, "status") == st))
        .collect();
    if json_out {
        let rows: Vec<Value> = items.iter().map(|o| item_json(o)).collect();
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }
    if items.is_empty() {
        if kind.is_some() || status.is_some() {
            println!("No business-semantics items match.");
        } else {
            println!(
                "No business-semantics items. Set `[semantics] enabled = true` in ekos.toml and \
                 re-run `ekos commit` (RFC 0170)."
            );
        }
        return Ok(());
    }
    let mut current = "";
    for o in &items {
        let k = kind_of(o).unwrap_or_default();
        if k != current {
            let n = items.iter().filter(|x| kind_of(x) == Some(k)).count();
            println!("\n{k} ({n})");
            current = k;
        }
        println!("  {}", line_for(o));
    }
    let confirmed = items
        .iter()
        .filter(|o| s(o, "status") == "confirmed")
        .count();
    println!(
        "\n{confirmed} of {} confirmed by a human; everything else is a hypothesis recovered from \
         code traces, or waiting for review.",
        items.len()
    );
    Ok(())
}

fn item_json(o: &KirObject) -> Value {
    let props: BTreeMap<&String, &Value> = o.properties.iter().collect();
    json!({"id": o.id.to_string(), "kind": kind_of(o), "name": o.name, "properties": props})
}

/// `ekos semantics show <name-or-id>`.
pub fn show(config: &EkosConfig, cwd: &Path, target: &str, json_out: bool) -> Result<()> {
    let (ledger, items) = current_items(config, cwd)?;
    let o = find_item(&items, target)?;
    let evidence: Vec<Value> = o
        .evidence
        .iter()
        .filter_map(|id| ledger.get_evidence(id).ok().flatten())
        .map(|e| json!({"path": e.location.path, "line": e.location.line, "fragment": e.fragment}))
        .collect();
    let by_id: HashMap<KirId, String> = ledger
        .all_objects()?
        .into_iter()
        .map(|x| (x.id, format!("{} ({})", x.name, x.kind)))
        .collect();
    let links: Vec<Value> = ledger
        .relationships_for(&o.id)?
        .into_iter()
        .filter(|r| r.from == o.id)
        .map(|r| {
            json!({"kind": r.kind.to_string(),
                   "to": by_id.get(&r.to).cloned().unwrap_or_else(|| r.to.to_string())})
        })
        .collect();
    if json_out {
        let mut v = item_json(o);
        v["evidence"] = json!(evidence);
        v["links"] = json!(links);
        println!("{}", serde_json::to_string_pretty(&v)?);
        return Ok(());
    }
    println!("{} — {}", o.name, kind_of(o).unwrap_or_default());
    println!("  id: {}", o.id);
    let mut keys: Vec<&String> = o.properties.keys().collect();
    keys.sort();
    for k in keys {
        println!("  {k}: {}", o.properties[k]);
    }
    println!("  evidence:");
    for e in &evidence {
        let line = e["line"]
            .as_u64()
            .map(|l| format!(":{l}"))
            .unwrap_or_default();
        println!(
            "    {}{line}  {}",
            e["path"].as_str().unwrap_or_default(),
            e["fragment"].as_str().unwrap_or_default()
        );
    }
    if !links.is_empty() {
        println!("  links:");
        for l in &links {
            println!(
                "    {} → {}",
                l["kind"].as_str().unwrap_or_default(),
                l["to"].as_str().unwrap_or_default()
            );
        }
    }
    Ok(())
}

/// `ekos semantics gaps` — the gap report: the questions only a human can answer.
pub fn gaps(config: &EkosConfig, cwd: &Path, json_out: bool) -> Result<()> {
    let (_, items) = current_items(config, cwd)?;
    // A gap a human rejected ("not a real unknown") is closed.
    let gaps: Vec<&KirObject> = items
        .iter()
        .filter(|o| matches!(kind_of(o), Some(k) if k == GAP || k == CONFLICT))
        .filter(|o| s(o, "status") != "rejected")
        .collect();
    let waiting: Vec<&KirObject> = items
        .iter()
        .filter(|o| s(o, "status") == "needs_review")
        .collect();
    if json_out {
        let rows: Vec<Value> = gaps.iter().chain(&waiting).map(|o| item_json(o)).collect();
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }
    let mut by_type: BTreeMap<String, Vec<&KirObject>> = BTreeMap::new();
    for g in &gaps {
        let t = match kind_of(g) {
            Some(k) if k == CONFLICT => {
                format!("conflicting definitions ({})", s(g, "conflict_type"))
            }
            _ => s(g, "gap_type"),
        };
        by_type.entry(t).or_default().push(g);
    }
    println!("Semantic gap report — {} open question(s)", gaps.len());
    for (t, gs) in by_type {
        println!("\n{t} ({})", gs.len());
        let mut gs = gs;
        // Most-used first: the gaps that matter most to the code.
        gs.sort_by_key(|g| {
            (
                std::cmp::Reverse(p(g, "usage_sites").as_u64().unwrap_or(0)),
                g.name.clone(),
            )
        });
        for g in gs {
            println!("  - {}", s(g, "question"));
        }
    }
    if !waiting.is_empty() {
        println!(
            "\nneeds_review ({}) — reviewed once, but the evidence changed or vanished",
            waiting.len()
        );
        for o in &waiting {
            println!("  - {}: {}", display_name(o), s(o, "review_reason"));
        }
    }
    println!(
        "\nNo trace, no recovery: these are the places where the code depends on a meaning no \
         source states. Answer them, then `ekos semantics confirm|reject|edit` the hypotheses."
    );
    Ok(())
}

// ── Agents (RFC 0170 Phase 4) — read-only, status on every answer ───────────────────────────

/// What an agent may say about an item's status, in words it cannot misread.
fn status_statement(o: &KirObject) -> &'static str {
    match s(o, "status").as_str() {
        "confirmed" => "CONFIRMED by a human reviewer — usable as the business definition",
        "rejected" => "REJECTED by a human reviewer — do not use",
        "needs_review" => {
            "NEEDS REVIEW — was reviewed, but its evidence changed; treat as unconfirmed"
        }
        _ => {
            "HYPOTHESIS — recovered from code traces, not confirmed by anyone; say so if you use it"
        }
    }
}

fn evidence_refs(ledger: &dyn KnowledgeStore, o: &KirObject, max: usize) -> Vec<String> {
    let mut out = Vec::new();
    for id in &o.evidence {
        if let Ok(Some(e)) = ledger.get_evidence(id) {
            out.push(match e.location.line {
                Some(l) => format!("{}:{l} — {}", e.location.path, e.fragment),
                None => format!("{} — {}", e.location.path, e.fragment),
            });
        }
        if out.len() >= max {
            break;
        }
    }
    out
}

fn agent_view(ledger: &dyn KnowledgeStore, o: &KirObject) -> Value {
    let kind = kind_of(o).unwrap_or_default();
    let mut v = json!({
        "id": o.id.to_string(),
        "kind": kind,
        "name": match s(o, "expert_name") {
            n if n.is_empty() => o.name.clone(),
            n => n,
        },
        "status": s(o, "status"),
        "status_means": status_statement(o),
        "evidence": evidence_refs(ledger, o, 5),
    });
    // Each output field from the first non-empty property: what a human set wins.
    for (k, from) in [
        ("definition", &["definition"][..]),
        ("description", &["expert_description", "description"]),
        ("table", &["table"]),
        ("column", &["column"]),
        ("value", &["value"]),
        ("meaning", &["expert_label", "label"]),
        ("expression", &["expression"]),
        ("ai_summary", &["llm_definition"]),
        ("question", &["question"]),
        ("confidence", &["confidence"]),
        ("reviewed_by", &["reviewed_by"]),
        ("review_note", &["review_note"]),
    ] {
        if let Some(val) = from.iter().map(|f| s(o, f)).find(|x| !x.is_empty()) {
            v[k] = json!(val);
        }
    }
    if !s(o, "expert_name").is_empty() {
        v["recovered_name"] = json!(o.name);
    }
    v
}

/// How well `o` matches `term` (lower-cased): 0 = exact name, 1 = name contains, 2 = a field
/// contains, `None` = no match.
fn match_rank(o: &KirObject, term: &str) -> Option<u8> {
    let names = [o.name.to_lowercase(), s(o, "expert_name").to_lowercase()];
    if names.iter().any(|n| !n.is_empty() && n == term) {
        return Some(0);
    }
    if names.iter().any(|n| n.contains(term)) {
        return Some(1);
    }
    let fields = [
        "definition",
        "table",
        "column",
        "label",
        "expert_label",
        "description",
        "expert_description",
        "expression",
    ];
    fields
        .iter()
        .any(|f| s(o, f).to_lowercase().contains(term))
        .then_some(2)
}

/// `ekos_semantics_lookup`: business definitions matching `term`, confirmed ones first, each with
/// its status spelled out. Rejected items are left out unless asked for.
pub fn agent_lookup(
    config: &EkosConfig,
    cwd: &Path,
    ledger: &dyn KnowledgeStore,
    term: &str,
    limit: usize,
    include_rejected: bool,
) -> Result<Value> {
    let term = term.trim().to_lowercase();
    if term.is_empty() {
        anyhow::bail!("term must not be empty");
    }
    let items = items_in(config, cwd, ledger)?;
    let status_rank = |o: &KirObject| match s(o, "status").as_str() {
        "confirmed" => 0,
        "needs_review" => 1,
        "hypothesis" => 2,
        _ => 3,
    };
    let mut hits: Vec<(u8, u8, &KirObject)> = items
        .iter()
        .filter(|o| matches!(kind_of(o), Some(k) if k == CONCEPT || k == ENUM_MEANING || k == CONSTRAINT))
        .filter(|o| include_rejected || s(o, "status") != "rejected")
        .filter_map(|o| match_rank(o, &term).map(|r| (r, status_rank(o), o)))
        .collect();
    hits.sort_by(|a, b| (a.0, a.1, &a.2.name).cmp(&(b.0, b.1, &b.2.name)));
    let total = hits.len();
    let results: Vec<Value> = hits
        .iter()
        .take(limit)
        .map(|(_, _, o)| agent_view(ledger, o))
        .collect();
    // Open questions touching the same thing: an agent should know what nobody has answered.
    let open: Vec<Value> = items
        .iter()
        .filter(|o| matches!(kind_of(o), Some(k) if k == GAP || k == CONFLICT))
        .filter(|o| s(o, "status") != "rejected")
        .filter(|o| {
            s(o, "question").to_lowercase().contains(&term) || o.name.to_lowercase().contains(&term)
        })
        .take(5)
        .map(
            |o| json!({"kind": kind_of(o), "question": s(o, "question"), "status": s(o, "status")}),
        )
        .collect();
    if results.is_empty() {
        return Ok(json!({
            "untrusted": true,
            "no_semantics_found": true,
            "term": term,
            "note": "No recovered business definition matches. Do not invent one; say the meaning is not recorded.",
            "open_questions": open,
        }));
    }
    Ok(json!({
        "untrusted": true,
        "term": term,
        "matches": total,
        "results": results,
        "open_questions": open,
        "note": "Each result carries its status. Only CONFIRMED results are reviewed business definitions; present anything else as a hypothesis from code, with its evidence.",
    }))
}

/// `ekos_semantics_gaps`: the open questions — unexplained codes, undocumented concepts, conflicting
/// definitions, and reviewed items whose evidence changed — optionally scoped to a table or term.
pub fn agent_gaps(
    config: &EkosConfig,
    cwd: &Path,
    ledger: &dyn KnowledgeStore,
    scope: Option<&str>,
    limit: usize,
) -> Result<Value> {
    let scope = scope
        .map(|x| x.trim().to_lowercase())
        .filter(|x| !x.is_empty());
    let items = items_in(config, cwd, ledger)?;
    let in_scope = |o: &KirObject| {
        scope.as_ref().is_none_or(|sc| {
            [
                o.name.clone(),
                s(o, "table"),
                s(o, "question"),
                s(o, "definition"),
            ]
            .iter()
            .any(|f| f.to_lowercase().contains(sc.as_str()))
        })
    };
    let mut questions: Vec<Value> = items
        .iter()
        .filter(|o| matches!(kind_of(o), Some(k) if k == GAP || k == CONFLICT))
        .filter(|o| s(o, "status") != "rejected")
        .filter(|o| in_scope(o))
        .map(|o| {
            json!({
                "kind": kind_of(o),
                "type": if kind_of(o) == Some(CONFLICT) { s(o, "conflict_type") } else { s(o, "gap_type") },
                "question": s(o, "question"),
                "status": s(o, "status"),
                "usage_sites": o.properties.get("usage_sites"),
                "evidence": evidence_refs(ledger, o, 3),
            })
        })
        .collect();
    questions.sort_by_key(|q| std::cmp::Reverse(q["usage_sites"].as_u64().unwrap_or(0)));
    let stale: Vec<Value> = items
        .iter()
        .filter(|o| s(o, "status") == "needs_review" && in_scope(o))
        .map(|o| json!({"name": display_name(o), "kind": kind_of(o), "reason": s(o, "review_reason")}))
        .collect();
    let total = questions.len();
    questions.truncate(limit);
    Ok(json!({
        "untrusted": true,
        "open_questions": questions,
        "total_open_questions": total,
        "needs_review": stale,
        "note": "These are questions for a human. Do not answer them by guessing; surface them.",
    }))
}

// ── Evaluation ───────────────────────────────────────────────────────────────────────────────

/// An expert-written gold set (RFC 0170 §5). Written **before** looking at EKOS's output.
///
/// ```yaml
/// concepts:                 # business concepts the code should reveal, as predicates
///   - name: Unapproved transaction
///     predicate: transactions.approved IS FALSE     # `a AND b` for a conjunction
/// enums:                    # coded columns and what each code means
///   - table: entity_credit_account
///     column: entity_class
///     values: {"1": Vendor, "2": Customer}
/// known_unknowns:           # values the expert agrees no source explains
///   - {table: acc_trans, column: status, value: "3"}
/// ```
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct GoldSet {
    #[serde(default)]
    pub concepts: Vec<GoldConcept>,
    #[serde(default)]
    pub enums: Vec<GoldEnum>,
    #[serde(default)]
    pub known_unknowns: Vec<GoldValue>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct GoldConcept {
    pub name: String,
    pub predicate: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct GoldEnum {
    pub table: String,
    pub column: String,
    pub values: BTreeMap<String, String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct GoldValue {
    pub table: String,
    pub column: String,
    pub value: String,
}

/// Atoms of a predicate, normalized for comparison: case, spacing and `AND` order do not matter.
pub fn predicate_atoms(p: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut rest = p.trim();
    loop {
        let upper = rest.to_ascii_uppercase();
        let (atom, tail) = match upper.find(" AND ") {
            // `BETWEEN a AND b` is one atom.
            Some(at)
                if !upper[..at].contains(" BETWEEN ")
                    || upper[..at].matches(" AND ").count() > 0 =>
            {
                (&rest[..at], Some(&rest[at + 5..]))
            }
            Some(at) => match upper[at + 5..].find(" AND ") {
                Some(next) => (&rest[..at + 5 + next], Some(&rest[at + 10 + next..])),
                None => (rest, None),
            },
            None => (rest, None),
        };
        let norm = atom
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase();
        if !norm.is_empty() {
            out.insert(norm);
        }
        match tail {
            Some(t) => rest = t.trim(),
            None => break,
        }
    }
    out
}

/// The code a gold value names, in EKOS's literal form, matched against the recovered values.
fn same_value(gold: &str, recovered: &str) -> bool {
    let g = gold.trim().trim_matches('\'');
    let r = recovered.trim_matches('\'');
    g == r
}

fn norm_label(l: &str) -> String {
    l.to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect()
}

fn ratio(n: usize, d: usize) -> Value {
    if d == 0 {
        Value::Null
    } else {
        json!((n as f64 / d as f64 * 1000.0).round() / 1000.0)
    }
}

/// `ekos semantics eval --gold <file>`.
pub fn eval(config: &EkosConfig, cwd: &Path, gold_path: &Path, json_out: bool) -> Result<()> {
    let gold: GoldSet = serde_yaml::from_str(
        &std::fs::read_to_string(gold_path)
            .with_context(|| format!("reading {}", gold_path.display()))?,
    )
    .with_context(|| format!("parsing {}", gold_path.display()))?;
    let (ledger, items) = current_items(config, cwd)?;
    let report = score(
        &gold,
        &items,
        |o| {
            o.evidence
                .iter()
                .filter_map(|id| ledger.get_evidence(id).ok().flatten())
                .map(|e| (e.location.path, e.location.line))
                .collect()
        },
        cwd,
    );
    if json_out {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }
    println!("RFC 0170 evaluation against {}", gold_path.display());
    for (k, v) in report["metrics"].as_object().into_iter().flatten() {
        println!("  {k:28} {}", v["value"]);
        if let Some(d) = v["detail"].as_str() {
            println!("  {:28} {d}", "");
        }
    }
    for (k, v) in report["misses"].as_object().into_iter().flatten() {
        let list = v.as_array().cloned().unwrap_or_default();
        if !list.is_empty() {
            println!("\n  {k} ({}):", list.len());
            for m in list {
                println!("    - {}", m.as_str().unwrap_or_default());
            }
        }
    }
    Ok(())
}

/// Score `items` against `gold`. `evidence_of` yields an item's `(path, line)` evidence; `cwd`
/// is where those paths are checked for evidence validity.
pub fn score(
    gold: &GoldSet,
    items: &[KirObject],
    evidence_of: impl Fn(&KirObject) -> Vec<(String, Option<u32>)>,
    cwd: &Path,
) -> Value {
    let concepts: Vec<&KirObject> = items
        .iter()
        .filter(|o| kind_of(o) == Some(CONCEPT))
        .collect();
    let enums: Vec<&KirObject> = items
        .iter()
        .filter(|o| kind_of(o) == Some(ENUM_MEANING))
        .collect();
    let gaps: Vec<&KirObject> = items.iter().filter(|o| kind_of(o) == Some(GAP)).collect();

    // Concepts: a gold concept is found when a recovered definition has exactly its atoms.
    let recovered_defs: Vec<BTreeSet<String>> = concepts
        .iter()
        .map(|c| predicate_atoms(&s(c, "definition")))
        .collect();
    let mut concept_hits = 0;
    let mut concept_misses = Vec::new();
    let mut matched_recovered: BTreeSet<usize> = BTreeSet::new();
    for g in &gold.concepts {
        let atoms = predicate_atoms(&g.predicate);
        match recovered_defs.iter().position(|d| *d == atoms) {
            Some(i) => {
                concept_hits += 1;
                matched_recovered.insert(i);
            }
            None => concept_misses.push(format!("{} — {}", g.name, g.predicate)),
        }
    }

    // Enum meanings: coverage = gold codes with any recovered label; accuracy = labels that agree.
    let mut gold_codes = 0;
    let mut covered = 0;
    let mut correct = 0;
    let mut enum_misses = Vec::new();
    let mut wrong = Vec::new();
    for g in &gold.enums {
        for (code, label) in &g.values {
            gold_codes += 1;
            let found = enums.iter().find(|e| {
                s(e, "table").eq_ignore_ascii_case(&g.table)
                    && s(e, "column").eq_ignore_ascii_case(&g.column)
                    && same_value(code, &s(e, "value"))
            });
            match found.map(|e| s(e, "label")).filter(|l| !l.is_empty()) {
                Some(l) => {
                    covered += 1;
                    let (a, b) = (norm_label(&l), norm_label(label));
                    if a == b || a.contains(&b) || b.contains(&a) {
                        correct += 1;
                    } else {
                        wrong.push(format!(
                            "{}.{} = {code}: gold `{label}`, recovered `{l}`",
                            g.table, g.column
                        ));
                    }
                }
                None => enum_misses.push(format!("{}.{} = {code} ({label})", g.table, g.column)),
            }
        }
    }

    // Gaps: known unknowns reported as gaps.
    let mut gap_hits = 0;
    let mut gap_misses = Vec::new();
    for g in &gold.known_unknowns {
        let hit = gaps.iter().any(|x| {
            s(x, "table").eq_ignore_ascii_case(&g.table)
                && s(x, "column").eq_ignore_ascii_case(&g.column)
                && same_value(&g.value, &s(x, "value"))
        });
        if hit {
            gap_hits += 1;
        } else {
            gap_misses.push(format!("{}.{} = {}", g.table, g.column, g.value));
        }
    }

    // Evidence validity (no gold needed): does the cited line exist and mention the column or the
    // view/table it is about?
    let mut refs = 0;
    let mut valid = 0;
    let mut invalid: Vec<String> = Vec::new();
    let mut files: HashMap<String, Option<Vec<String>>> = HashMap::new();
    for o in concepts.iter().chain(enums.iter()) {
        let needles: Vec<String> = match kind_of(o) {
            Some(k) if k == CONCEPT => p(o, "predicates")
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str())
                        .filter_map(|x| x.split_whitespace().next())
                        .map(|t| t.rsplit('.').next().unwrap_or(t).to_lowercase())
                        .collect()
                })
                .unwrap_or_default(),
            // A meaning is cited where it is stated: the column's comment or the lookup table's
            // seed row, which names the label, not necessarily the referencing column.
            _ => [s(o, "column"), s(o, "label")]
                .into_iter()
                .filter(|n| !n.is_empty())
                .map(|n| n.to_lowercase())
                .collect(),
        };
        // A multi-row `VALUES` list is cited at its `INSERT` line; its rows follow.
        let span = if kind_of(o) == Some(CONCEPT) { 4 } else { 12 };
        for (path, line) in evidence_of(o) {
            let Some(line) = line else { continue };
            refs += 1;
            let lines = files.entry(path.clone()).or_insert_with(|| {
                std::fs::read_to_string(cwd.join(&path))
                    .ok()
                    .map(|t| t.lines().map(str::to_lowercase).collect())
            });
            let Some(lines) = lines else {
                if invalid.len() < 25 {
                    invalid.push(format!("{path}:{line} — file not found"));
                }
                continue;
            };
            // Seed rows and comments cite their statement's first line; allow a short window.
            let window = lines
                .iter()
                .skip(line.saturating_sub(1) as usize)
                .take(span)
                .cloned()
                .collect::<Vec<_>>()
                .join(" ");
            // Alphanumerics only: `EC_HOT_LEAD` cites the label "hot lead".
            let squash = |t: &str| {
                t.chars()
                    .filter(|c| c.is_alphanumeric())
                    .collect::<String>()
            };
            let window_sq = squash(&window);
            if needles.is_empty()
                || needles.iter().any(|n| {
                    window.contains(n.as_str())
                        || (!squash(n).is_empty() && window_sq.contains(&squash(n)))
                })
            {
                valid += 1;
            } else if invalid.len() < 25 {
                invalid.push(format!(
                    "{path}:{line} cited for {} ({})",
                    o.name,
                    needles.join("/")
                ));
            }
        }
    }

    let explained = enums.iter().filter(|e| !s(e, "label").is_empty()).count();

    // RFC 0170 §5 metrics that need no gold set, only human reviews (Phase 2).
    let reviewed = |kind: &str| -> (usize, usize, usize) {
        let of_kind = items.iter().filter(|o| kind_of(o) == Some(kind));
        let (mut ok, mut edited, mut rejected) = (0, 0, 0);
        for o in of_kind {
            match s(o, "status").as_str() {
                "confirmed" if o.properties.keys().any(|k| k.starts_with("expert_")) => edited += 1,
                "confirmed" => ok += 1,
                "rejected" => rejected += 1,
                _ => {}
            }
        }
        (ok, edited, rejected)
    };
    let (c_ok, c_edit, c_rej) = reviewed(CONCEPT);
    let (g_ok, g_edit, g_rej) = reviewed(GAP);
    json!({
        "gold": {"concepts": gold.concepts.len(), "enum_codes": gold_codes, "known_unknowns": gold.known_unknowns.len()},
        "recovered": {"concepts": concepts.len(), "enum_values": enums.len(), "gaps": gaps.len()},
        "metrics": {
            "concept_recall": {"value": ratio(concept_hits, gold.concepts.len()),
                "detail": format!("{concept_hits} of {} gold concepts recovered with exactly their predicate", gold.concepts.len())},
            "concept_precision_vs_gold": {"value": ratio(matched_recovered.len(), concepts.len()),
                "detail": format!("{} of {} recovered concepts are in the gold set (a lower bound unless the gold set is exhaustive)", matched_recovered.len(), concepts.len())},
            "enum_coverage_gold": {"value": ratio(covered, gold_codes),
                "detail": format!("{covered} of {gold_codes} gold codes have a meaning hypothesis")},
            "enum_label_accuracy": {"value": ratio(correct, covered),
                "detail": format!("{correct} of {covered} recovered labels agree with the gold label")},
            "enum_coverage_all": {"value": ratio(explained, enums.len()),
                "detail": format!("{explained} of {} recovered coded values have at least one meaning", enums.len())},
            "gap_recall": {"value": ratio(gap_hits, gold.known_unknowns.len()),
                "detail": format!("{gap_hits} of {} known unknowns reported as gaps", gold.known_unknowns.len())},
            "definition_precision_reviewed": {"value": ratio(c_ok, c_ok + c_edit + c_rej),
                "detail": format!("{c_ok} of {} reviewed concepts accepted without edits ({c_edit} edited, {c_rej} rejected)", c_ok + c_edit + c_rej)},
            "gap_usefulness_reviewed": {"value": ratio(g_ok + g_edit, g_ok + g_edit + g_rej),
                "detail": format!("{} of {} reviewed gaps confirmed as real unknowns", g_ok + g_edit, g_ok + g_edit + g_rej)},
            "evidence_validity": {"value": ratio(valid, refs),
                "detail": format!("{valid} of {refs} cited lines exist and mention what they are cited for")},
        },
        "misses": {"concepts": concept_misses, "enum_codes": enum_misses, "wrong_labels": wrong, "known_unknowns": gap_misses, "invalid_evidence (first 25)": invalid},
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn porcelain_reports_each_line_with_its_commit_details_once_given() {
        let sha_a = "a".repeat(40);
        let sha_b = "b".repeat(40);
        let out = format!(
            "{sha_a} 10 12 1\nauthor Ann\nauthor-mail <a@x>\nauthor-time 1600000000\nauthor-tz +0000\n\
             summary exclude suspended accounts\nfilename x.sql\n\tWHERE status <> 3\n\
             {sha_b} 3 40 1\nauthor Bob\nauthor-time 1500000000\nsummary initial schema\nfilename x.sql\n\t  AND x\n\
             {sha_a} 11 13 1\nfilename x.sql\n\t  AND y\n\
             {zero} 1 50 1\nauthor Not Committed Yet\nauthor-time 1700000000\nsummary Version of x.sql from x.sql\nfilename x.sql\n\tz\n",
            zero = "0".repeat(40)
        );
        let hits = parse_porcelain(&out);
        assert_eq!(hits.len(), 3, "{hits:?}");
        assert_eq!(hits[0].line, 12);
        assert_eq!(hits[0].summary, "exclude suspended accounts");
        assert_eq!(hits[0].author, "Ann");
        assert!(hits[0].date.starts_with("2020-09-13"));
        assert_eq!(hits[1].sha, sha_b);
        // The repeated commit carries the details from its first appearance.
        assert_eq!(hits[2].line, 13);
        assert_eq!(hits[2].summary, "exclude suspended accounts");
    }

    #[test]
    fn predicate_atoms_ignore_case_spacing_and_order() {
        assert_eq!(
            predicate_atoms("ar.approved IS TRUE AND  ar.amount_bc > 0"),
            predicate_atoms("ar.amount_bc > 0 and ar.approved is true")
        );
        assert_eq!(
            predicate_atoms("t.d BETWEEN 1 AND 5 AND t.x IS NULL").len(),
            2,
            "{:?}",
            predicate_atoms("t.d BETWEEN 1 AND 5 AND t.x IS NULL")
        );
    }

    #[test]
    fn scoring_counts_hits_and_reports_misses() {
        let mk = |kind: &str, name: &str, props: Value| {
            let mut o = KirObject::new(name, business_semantics::object_kind(kind));
            for (k, v) in props.as_object().unwrap() {
                o.properties.insert(k.clone(), v.clone());
            }
            o
        };
        let items = vec![
            mk(
                CONCEPT,
                "PartsNotObsolete",
                json!({"definition": "parts.obsolete IS FALSE",
                "predicates": ["parts.obsolete IS FALSE"]}),
            ),
            mk(
                ENUM_MEANING,
                "e1",
                json!({"table": "ec", "column": "cls", "value": "2", "label": "Customer"}),
            ),
            mk(
                ENUM_MEANING,
                "e2",
                json!({"table": "ec", "column": "cls", "value": "1", "label": "Seller"}),
            ),
            mk(
                ENUM_MEANING,
                "e3",
                json!({"table": "a", "column": "k", "value": "'Z'", "label": null}),
            ),
            mk(
                GAP,
                "g",
                json!({"table": "a", "column": "k", "value": "'Z'"}),
            ),
        ];
        let gold: GoldSet = serde_yaml::from_str(
            "concepts:\n  - {name: Active part, predicate: parts.obsolete is false}\n  - {name: Open, predicate: oe.closed IS FALSE}\n\
             enums:\n  - {table: ec, column: cls, values: {'1': Vendor, '2': customer, '3': Employee}}\n\
             known_unknowns:\n  - {table: a, column: k, value: Z}\n",
        )
        .unwrap();
        let r = score(&gold, &items, |_| Vec::new(), Path::new("."));
        assert_eq!(r["metrics"]["concept_recall"]["value"], json!(0.5));
        assert_eq!(r["metrics"]["enum_coverage_gold"]["value"], json!(0.667));
        assert_eq!(r["metrics"]["enum_label_accuracy"]["value"], json!(0.5));
        assert_eq!(r["metrics"]["gap_recall"]["value"], json!(1.0));
        assert_eq!(r["misses"]["wrong_labels"].as_array().unwrap().len(), 1);
    }

    /// RFC 0170 Phase 2: promotion stays human-only.
    #[test]
    fn no_mcp_code_can_reach_the_review_lifecycle() {
        let mcp = include_str!("mcp.rs");
        assert!(
            !mcp.contains("semantics_review"),
            "semantics review must stay CLI-only"
        );
        assert!(!mcp.contains("semantics::review"));
        assert!(
            !mcp.contains("import::linkml"),
            "LinkML import records human decisions"
        );
    }

    /// The commit step end to end over a real fact ledger: a review survives an unchanged re-run
    /// (which writes nothing), and a changed source flips it to `needs_review`.
    #[tokio::test]
    async fn a_review_holds_until_the_evidence_changes() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = EkosConfig::default();
        config.semantics.enabled = true;
        config.semantics.rationale = false;
        let ledger = ekos_ledger::FactLedger::open(&dir.path().join(".ekos/ledger/facts")).unwrap();

        let mut parts = KirObject::new("parts", ekos_kir::ObjectKind::Table);
        parts.properties.insert(
            "columns".into(),
            json!([{"name": "obsolete", "data_type": "BOOLEAN"}]),
        );
        ledger.append_object(&parts).unwrap();
        let carrier = |name: &str, op: &str| {
            let mut o = KirObject::new(
                name,
                ekos_kir::ObjectKind::Custom("ProcedureStatement".into()),
            );
            o.id = KirId(uuid::Uuid::new_v5(
                &uuid::Uuid::NAMESPACE_URL,
                name.as_bytes(),
            ));
            o.properties
                .insert("source_path".into(), json!("sql/x.sql"));
            o.properties.insert(
                "predicates".into(),
                json!([{"relation": "parts", "column": "obsolete", "op": op,
                        "clause": "where", "top_level": true, "line": 3}]),
            );
            o
        };
        ledger.append_object(&carrier("a#1", "is_false")).unwrap();
        ledger.append_object(&carrier("b#1", "is_false")).unwrap();

        let (_, first) = commit_step(&config, dir.path(), &ledger, true)
            .await
            .unwrap()
            .unwrap();
        assert!(first > 0);
        let items: Vec<KirObject> = ledger
            .all_objects()
            .unwrap()
            .into_iter()
            .filter(|o| kind_of(o) == Some(CONCEPT))
            .collect();
        assert_eq!(items.len(), 1);
        let reviewed =
            semantics_review::apply_review(&items[0], &Decision::Confirm, "ann", "t", None)
                .unwrap();
        ledger.append_object(&reviewed).unwrap();

        let (_, again) = commit_step(&config, dir.path(), &ledger, true)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(again, 0, "an unchanged re-run writes nothing");
        assert_eq!(
            ledger.get_object(&reviewed.id).unwrap().unwrap().properties["status"],
            json!("confirmed")
        );

        // One routine changes its filter: the concept rests on different evidence now.
        ledger
            .append_object(&carrier("b#1", "is_not_true"))
            .unwrap();
        ledger.append_object(&carrier("c#1", "is_false")).unwrap();
        commit_step(&config, dir.path(), &ledger, true)
            .await
            .unwrap();
        let after = ledger.get_object(&reviewed.id).unwrap().unwrap();
        assert_eq!(after.properties["status"], json!("needs_review"));
        assert_eq!(
            after.properties["previous_review"]["status"],
            json!("confirmed")
        );
    }

    /// RFC 0170 Phase 4: agents get status in words, confirmed first, and an explicit "nothing
    /// found" instead of an empty list to improvise from.
    #[tokio::test]
    async fn agent_lookup_states_the_status_and_never_returns_rejected_by_default() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = EkosConfig::default();
        config.semantics.enabled = true;
        config.semantics.rationale = false;
        let ledger = ekos_ledger::FactLedger::open(&dir.path().join(".ekos/ledger/facts")).unwrap();
        let mut parts = KirObject::new("parts", ekos_kir::ObjectKind::Table);
        parts.properties.insert(
            "columns".into(),
            json!([{"name": "obsolete", "data_type": "BOOLEAN"},
                   {"name": "assembly", "data_type": "BOOLEAN"}]),
        );
        ledger.append_object(&parts).unwrap();
        for (name, col) in [
            ("a#1", "obsolete"),
            ("b#1", "obsolete"),
            ("c#1", "assembly"),
            ("d#1", "assembly"),
        ] {
            let mut o = KirObject::new(
                name,
                ekos_kir::ObjectKind::Custom("ProcedureStatement".into()),
            );
            o.id = KirId(uuid::Uuid::new_v5(
                &uuid::Uuid::NAMESPACE_URL,
                name.as_bytes(),
            ));
            o.properties
                .insert("source_path".into(), json!("sql/x.sql"));
            o.properties.insert(
                "predicates".into(),
                json!([{"relation": "parts", "column": col, "op": "is_false",
                        "clause": "where", "top_level": true, "line": 3}]),
            );
            ledger.append_object(&o).unwrap();
        }
        commit_step(&config, dir.path(), &ledger, true)
            .await
            .unwrap();
        let items = items_in(&config, dir.path(), &ledger).unwrap();
        let find = |n: &str| items.iter().find(|o| o.name == n).unwrap().clone();
        let confirmed = semantics_review::apply_review(
            &find("PartsNotObsolete"),
            &Decision::Edit {
                name: Some("ActivePart".into()),
                description: None,
                label: None,
            },
            "ann",
            "t",
            None,
        )
        .unwrap();
        ledger.append_object(&confirmed).unwrap();
        let rejected = semantics_review::apply_review(
            &find("PartsNotAssembly"),
            &Decision::Reject,
            "ann",
            "t",
            Some("plumbing"),
        )
        .unwrap();
        ledger.append_object(&rejected).unwrap();

        let r = agent_lookup(&config, dir.path(), &ledger, "Parts", 10, false).unwrap();
        let results = r["results"].as_array().unwrap();
        assert_eq!(results.len(), 1, "{r}");
        assert_eq!(results[0]["name"], json!("ActivePart"));
        assert_eq!(results[0]["recovered_name"], json!("PartsNotObsolete"));
        assert!(
            results[0]["status_means"]
                .as_str()
                .unwrap()
                .starts_with("CONFIRMED")
        );
        assert_eq!(r["untrusted"], json!(true));

        let all = agent_lookup(&config, dir.path(), &ledger, "parts", 10, true).unwrap();
        assert_eq!(all["results"].as_array().unwrap().len(), 2);

        let none = agent_lookup(&config, dir.path(), &ledger, "invoice", 10, false).unwrap();
        assert_eq!(none["no_semantics_found"], json!(true));
        assert!(agent_lookup(&config, dir.path(), &ledger, "  ", 10, false).is_err());

        let g = agent_gaps(&config, dir.path(), &ledger, Some("parts"), 20).unwrap();
        assert!(g["open_questions"].is_array());
    }

    #[test]
    fn kind_names_are_forgiving() {
        assert_eq!(kind_filter("concepts").unwrap(), CONCEPT);
        assert_eq!(kind_filter("Enum").unwrap(), ENUM_MEANING);
        assert!(kind_filter("tables").is_err());
    }
}
