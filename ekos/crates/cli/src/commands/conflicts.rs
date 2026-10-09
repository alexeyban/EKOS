//! `ekos conflicts …` and the `[conflicts]` step of `ekos commit` — RFC 0172.
//!
//! `ekos compile` records duplicate definitions and merge losses as `ConflictingEvidence` items in
//! the CKM; `ekos commit` carries each one's review forward ([`carry_review`]), adds label
//! mismatches between business-semantics sources ([`commit_step`]), and writes
//! `.ekos/conflicts/current.json`, the ids the latest commit derived. The ledger is append-only, so
//! a disagreement that went away is simply no longer current.
//!
//! **Human-only:** [`resolve`] is the only caller of `conflicts::resolve`; `commands/mcp.rs` must
//! never reach it (a test below scans for it). Agents read conflicts; they never settle them.

use super::store::{open_store, open_store_read_only};
use anyhow::Result;
use ekos_compiler_core::EkosConfig;
use ekos_kir::{KirId, KirObject};
use ekos_ledger::KnowledgeStore;
use ekos_semantic::conflicts::{self, DISMISSED, OPEN, RESOLVED, Resolution};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

fn manifest_path(config: &EkosConfig, cwd: &Path) -> PathBuf {
    config.ekos_dir(cwd).join("conflicts").join("current.json")
}

/// Phase 3: doc-vs-data conflicts belong to `ekos migrate assess`, not to `commit`, so they live in
/// their own manifest (`table → ids`) that a commit never overwrites.
fn migrate_manifest_path(config: &EkosConfig, cwd: &Path) -> PathBuf {
    config.ekos_dir(cwd).join("conflicts").join("migrate.json")
}

/// `ekos migrate assess`: these are now the doc-vs-data conflicts for `table` (replacing what an
/// earlier assessment of that table recorded), and the counts are refreshed.
pub fn record_migrate(
    config: &EkosConfig,
    cwd: &Path,
    ledger: &dyn KnowledgeStore,
    table: &str,
    ids: &[KirId],
) -> Result<()> {
    let path = migrate_manifest_path(config, cwd);
    let mut map: BTreeMap<String, Vec<String>> = std::fs::read(&path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    let mut v: Vec<String> = ids.iter().map(|i| i.to_string()).collect();
    v.sort();
    if v.is_empty() {
        map.remove(table);
    } else {
        map.insert(table.to_string(), v);
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, serde_json::to_string_pretty(&map)?)?;
    let commit_ids = commit_ids(config, cwd).unwrap_or_default();
    write_manifest(
        config,
        cwd,
        &commit_ids,
        count(&items_in(config, cwd, ledger)?),
    )
}

fn migrate_ids(config: &EkosConfig, cwd: &Path) -> BTreeSet<String> {
    std::fs::read(migrate_manifest_path(config, cwd))
        .ok()
        .and_then(|b| serde_json::from_slice::<BTreeMap<String, Vec<String>>>(&b).ok())
        .map(|m| m.into_values().flatten().collect())
        .unwrap_or_default()
}

fn s<'a>(o: &'a KirObject, key: &str) -> &'a str {
    o.properties.get(key).and_then(Value::as_str).unwrap_or("")
}

/// Commit: keep the ledger's review on a freshly compiled conflict while its claims are unchanged.
pub fn carry_review(ledger: &dyn KnowledgeStore, fresh: &mut KirObject) -> Result<()> {
    if conflicts::is_conflict(fresh) {
        let current = ledger.get_object(&fresh.id)?;
        conflicts::carry_forward(fresh, current.as_ref());
    }
    Ok(())
}

/// What one commit derived.
#[derive(Debug, Default, Clone, Copy)]
pub struct ConflictStats {
    pub open: usize,
    pub resolved: usize,
    pub dismissed: usize,
    pub written: usize,
}

impl ConflictStats {
    pub fn total(&self) -> usize {
        self.open + self.resolved + self.dismissed
    }

    pub fn summary_line(&self) -> String {
        format!(
            "{} item(s): {} open, {} resolved, {} dismissed — `ekos conflicts list`",
            self.total(),
            self.open,
            self.resolved,
            self.dismissed
        )
    }
}

/// Commit, after business semantics: label mismatches between `EnumMeaning` sources, then the
/// manifest of every current conflict (`compiled` = the ones the CKM brought).
pub fn commit_step(
    config: &EkosConfig,
    cwd: &Path,
    ledger: &dyn KnowledgeStore,
    compiled: &[KirId],
) -> Result<ConflictStats> {
    let mut ids: BTreeSet<String> = compiled.iter().map(|i| i.to_string()).collect();
    let mut written = 0usize;
    if config.semantics.enabled {
        let enums: Vec<KirObject> = super::semantics::items_in(config, cwd, ledger)?
            .into_iter()
            .filter(|o| matches!(&o.kind, ekos_kir::ObjectKind::Custom(k) if k == "EnumMeaning"))
            .collect();
        let g = conflicts::label_mismatches(&enums);
        for ev in &g.evidence {
            ledger.append_evidence(ev)?;
        }
        for o in &g.objects {
            let mut o = o.clone();
            carry_review(ledger, &mut o)?;
            if ledger.append_object(&o)? {
                written += 1;
            }
            ids.insert(o.id.to_string());
        }
        for r in &g.relationships {
            if ledger.append_relationship(r)? {
                written += 1;
            }
        }
    }
    write_manifest(config, cwd, &ids, ConflictStats::default())?;
    let mut stats = count(&items_in(config, cwd, ledger)?);
    write_manifest(config, cwd, &ids, stats)?;
    stats.written = written;
    Ok(stats)
}

fn count(items: &[KirObject]) -> ConflictStats {
    let mut stats = ConflictStats::default();
    for o in items {
        match s(o, "status") {
            RESOLVED => stats.resolved += 1,
            DISMISSED => stats.dismissed += 1,
            _ => stats.open += 1,
        }
    }
    stats
}

/// `.ekos/conflicts/current.json`: the current ids, plus counts so `ekos status` never has to read
/// the ledger for them.
fn write_manifest(
    config: &EkosConfig,
    cwd: &Path,
    ids: &BTreeSet<String>,
    stats: ConflictStats,
) -> Result<()> {
    let path = manifest_path(config, cwd);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&json!({
            "version": 1,
            "ids": ids,
            "open": stats.open,
            "resolved": stats.resolved,
            "dismissed": stats.dismissed,
        }))?,
    )?;
    Ok(())
}

/// `ekos status`: the counts the last commit (or decision) recorded; `None` before the first.
pub fn recorded_counts(config: &EkosConfig, cwd: &Path) -> Option<ConflictStats> {
    let v: Value = serde_json::from_slice(&std::fs::read(manifest_path(config, cwd)).ok()?).ok()?;
    let n = |k: &str| v[k].as_u64().unwrap_or(0) as usize;
    Some(ConflictStats {
        open: n("open"),
        resolved: n("resolved"),
        dismissed: n("dismissed"),
        written: 0,
    })
}

/// The current conflicts in an open store — every one ever written when no manifest exists yet.
/// Sorted by subject, then attribute.
pub fn items_in(
    config: &EkosConfig,
    cwd: &Path,
    ledger: &dyn KnowledgeStore,
) -> Result<Vec<KirObject>> {
    let current = current_ids(config, cwd);
    let mut items: Vec<KirObject> = ledger
        .all_objects()?
        .into_iter()
        .filter(conflicts::is_conflict)
        .filter(|o| {
            current
                .as_ref()
                .is_none_or(|c| c.contains(&o.id.to_string()))
        })
        .collect();
    items.sort_by(|a, b| {
        (s(a, "subject_name"), s(a, "attribute")).cmp(&(s(b, "subject_name"), s(b, "attribute")))
    });
    Ok(items)
}

/// The open conflicts about one object — for `ekos_state` and `ekos conflicts show`.
pub fn open_about(items: &[KirObject], subject: &KirId) -> Vec<Value> {
    let id = subject.to_string();
    items
        .iter()
        .filter(|o| s(o, "subject_id") == id && s(o, "status") == OPEN)
        .map(brief)
        .collect()
}

/// What is current: the commit's conflicts plus Migrate's. `None` when neither manifest exists yet
/// (then every conflict in the ledger counts).
fn current_ids(config: &EkosConfig, cwd: &Path) -> Option<BTreeSet<String>> {
    let migrate = migrate_ids(config, cwd);
    match commit_ids(config, cwd) {
        Some(mut c) => {
            c.extend(migrate);
            Some(c)
        }
        None if !migrate.is_empty() => Some(migrate),
        None => None,
    }
}

fn commit_ids(config: &EkosConfig, cwd: &Path) -> Option<BTreeSet<String>> {
    std::fs::read(manifest_path(config, cwd))
        .ok()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .and_then(|v| {
            v["ids"].as_array().map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
        })
}

/// The open conflicts about one object, found through its incoming `Disputes` links — cheap
/// enough for every `ekos_state` call.
pub fn open_for(
    config: &EkosConfig,
    cwd: &Path,
    ledger: &dyn KnowledgeStore,
    subject: &KirId,
) -> Result<Vec<Value>> {
    let current = current_ids(config, cwd);
    let mut out = Vec::new();
    for rel in ledger.relationships_for(subject)? {
        if rel.to != *subject
            || !matches!(&rel.kind, ekos_kir::RelationshipKind::Custom(k) if k == conflicts::DISPUTES)
        {
            continue;
        }
        if current
            .as_ref()
            .is_some_and(|c| !c.contains(&rel.from.to_string()))
        {
            continue;
        }
        if let Some(o) = ledger.get_object(&rel.from)?
            && s(&o, "status") == OPEN
        {
            out.push(brief(&o));
        }
    }
    out.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    Ok(out)
}

/// MCP `ekos_conflicts` — read-only. `subject` narrows to conflicts whose subject or name contains
/// it; `status` defaults to `open`.
pub fn agent_list(
    config: &EkosConfig,
    cwd: &Path,
    ledger: &dyn KnowledgeStore,
    subject: Option<&str>,
    status: &str,
    limit: usize,
) -> Result<Value> {
    if !["open", "resolved", "dismissed", "all"].contains(&status) {
        anyhow::bail!("unknown status `{status}` — open, resolved, dismissed or all");
    }
    let needle = subject.map(str::to_lowercase);
    let items: Vec<KirObject> = items_in(config, cwd, ledger)?
        .into_iter()
        .filter(|o| status == "all" || s(o, "status") == status)
        .filter(|o| {
            needle.as_ref().is_none_or(|n| {
                o.name.to_lowercase().contains(n)
                    || s(o, "subject_name").to_lowercase().contains(n)
                    || s(o, "subject_id") == n
            })
        })
        .collect();
    let total = items.len();
    Ok(json!({
        "total": total,
        "truncated": total > limit,
        "conflicts": items.iter().take(limit).map(brief).collect::<Vec<_>>(),
        "note": "Each conflict is two or more sources disagreeing about one fact. An `open` one is unresolved: report the disagreement with its claims, never just one side. Only a person can resolve it, on the CLI.",
    }))
}

/// The agent-facing shape of one conflict.
pub fn brief(o: &KirObject) -> Value {
    json!({
        "id": o.id.to_string(),
        "name": o.name,
        "subject": { "id": s(o, "subject_id"), "name": s(o, "subject_name"), "kind": s(o, "subject_kind") },
        "attribute": s(o, "attribute"),
        "conflict_type": s(o, "conflict_type"),
        "status": s(o, "status"),
        "status_meaning": match s(o, "status") {
            RESOLVED => "a person decided which claim is right",
            DISMISSED => "a person decided both claims are valid",
            _ => "unresolved: sources disagree and nobody has decided — do not present either claim as settled",
        },
        "claims": o.properties.get("claims"),
        "chosen_by_ekos": o.properties.get("chosen"),
        "picked_claim": o.properties.get("picked_claim"),
        "review_note": o.properties.get("review_note"),
        "measured": o.properties.get("measured"),
    })
}

fn find<'a>(items: &'a [KirObject], target: &str) -> Result<&'a KirObject> {
    let matches: Vec<&KirObject> = items
        .iter()
        .filter(|o| o.id.to_string() == target || o.name == target)
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
                "no conflict named `{target}`{}",
                if near.is_empty() {
                    String::new()
                } else {
                    format!(" — did you mean: {}", near.join(", "))
                }
            )
        }
        many => anyhow::bail!(
            "`{target}` names {} conflicts; pass an id: {}",
            many.len(),
            many.iter()
                .map(|o| o.id.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn claims(o: &KirObject) -> Vec<Value> {
    o.properties
        .get("claims")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn claim_line(i: usize, c: &Value) -> String {
    let at = match c["line"].as_u64() {
        Some(l) => format!("{}:{l}", c["path"].as_str().unwrap_or("?")),
        None => c["path"].as_str().unwrap_or("?").to_string(),
    };
    format!(
        "{}. {}  ({at}; {})",
        i + 1,
        c["value"],
        c["source"].as_str().unwrap_or("")
    )
}

/// `ekos conflicts list`.
pub fn list(
    config: &EkosConfig,
    cwd: &Path,
    status: Option<&str>,
    conflict_type: Option<&str>,
    json_out: bool,
) -> Result<()> {
    if let Some(st) = status
        && ![OPEN, RESOLVED, DISMISSED].contains(&st)
    {
        anyhow::bail!("unknown status `{st}` — open, resolved or dismissed");
    }
    let ledger = open_store_read_only(config, cwd)?;
    let items: Vec<KirObject> = items_in(config, cwd, &*ledger)?
        .into_iter()
        .filter(|o| status.is_none_or(|st| s(o, "status") == st))
        .filter(|o| conflict_type.is_none_or(|t| s(o, "conflict_type") == t))
        .collect();
    if json_out {
        println!(
            "{}",
            serde_json::to_string_pretty(&items.iter().map(brief).collect::<Vec<_>>())?
        );
        return Ok(());
    }
    if items.is_empty() {
        println!("No conflicting evidence: no source disagrees with another about the same fact.");
        return Ok(());
    }
    let mut by_type: BTreeMap<&str, usize> = BTreeMap::new();
    for o in &items {
        *by_type.entry(s(o, "conflict_type")).or_default() += 1;
    }
    println!(
        "Conflicting evidence ({}): {}",
        items.len(),
        by_type
            .iter()
            .map(|(t, n)| format!("{n} {t}"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    for o in &items {
        let c = claims(o);
        let values: Vec<String> = c.iter().map(|c| c["value"].to_string()).collect();
        println!(
            "  [{}] {}  — {} source(s) disagree: {}",
            s(o, "status"),
            o.name,
            c.len(),
            truncate(&values.join("  vs  "), 140)
        );
    }
    println!();
    println!("`ekos conflicts show <name>` for each claim's file and line; `resolve` to decide.");
    Ok(())
}

fn truncate(t: &str, n: usize) -> String {
    if t.chars().count() <= n {
        t.to_string()
    } else {
        format!("{}…", t.chars().take(n).collect::<String>())
    }
}

/// `ekos conflicts show`.
pub fn show(config: &EkosConfig, cwd: &Path, target: &str, json_out: bool) -> Result<()> {
    let ledger = open_store_read_only(config, cwd)?;
    let items = items_in(config, cwd, &*ledger)?;
    let o = find(&items, target)?;
    if json_out {
        println!("{}", serde_json::to_string_pretty(&brief(o))?);
        return Ok(());
    }
    println!(
        "{} — ConflictingEvidence ({})",
        o.name,
        s(o, "conflict_type")
    );
    println!("  id:        {}", o.id);
    println!(
        "  subject:   {} {} ({})",
        s(o, "subject_kind"),
        s(o, "subject_name"),
        s(o, "subject_id")
    );
    println!("  attribute: {}", s(o, "attribute"));
    println!("  status:    {}", s(o, "status"));
    println!("  claims:");
    for (i, c) in claims(o).iter().enumerate() {
        println!("    {}", claim_line(i, c));
    }
    if let Some(chosen) = o.properties.get("chosen").filter(|v| !v.is_null()) {
        println!("  EKOS kept: {chosen}");
    }
    // RFC 0172 Phase 3 — the measurement behind a doc-vs-data conflict, so it can be re-run.
    if let Some(m) = o.properties.get("measured") {
        println!(
            "  measured:  {} row(s) contradict the documentation ({})",
            m["violating_rows"],
            m["measured_at"].as_str().unwrap_or("?")
        );
        println!("  query:     {}", m["sql"].as_str().unwrap_or(""));
    }
    if let Some(p) = o.properties.get("picked_claim") {
        println!(
            "  picked:    {} ({})",
            p["value"],
            p["path"].as_str().unwrap_or("")
        );
    }
    if !s(o, "reviewed_by").is_empty() {
        println!(
            "  decided by {} at {}{}",
            s(o, "reviewed_by"),
            s(o, "reviewed_at"),
            match s(o, "review_note") {
                "" => String::new(),
                n => format!(" — {n}"),
            }
        );
    }
    if let Some(prev) = o.properties.get("previous_review") {
        println!(
            "  reopened: {} (was {} by {})",
            s(o, "review_reason"),
            prev["status"].as_str().unwrap_or("?"),
            prev["reviewed_by"].as_str().unwrap_or("?")
        );
    }
    Ok(())
}

/// `ekos conflicts resolve` — human-only. Writes one new version of the conflict.
pub fn resolve(
    config: &EkosConfig,
    cwd: &Path,
    target: &str,
    decision: Resolution,
    by: Option<String>,
    note: Option<String>,
) -> Result<()> {
    let by = by
        .or_else(|| std::env::var("USER").ok())
        .filter(|b| !b.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("who is deciding? Pass --as <you>"))?;
    let ledger = open_store(config, cwd)?;
    let items = items_in(config, cwd, &*ledger)?;
    let current = find(&items, target)?;
    let at = chrono::Utc::now().to_rfc3339();
    let next = conflicts::resolve(current, &decision, &by, &at, note.as_deref())
        .map_err(|e| anyhow::anyhow!("{target}: {e}"))?;
    ledger.set_write_context(Some(ekos_ledger::provenance::WriteContext {
        run_id: ekos_ledger::provenance::new_run_id(),
        stage: "conflicts-review".into(),
        source_artifact_id: None,
    }));
    ledger.append_object(&next)?;
    let ids = commit_ids(config, cwd).unwrap_or_default();
    write_manifest(config, cwd, &ids, count(&items_in(config, cwd, &*ledger)?))?;
    println!(
        "{} — {} by {by}{}",
        next.name,
        s(&next, "status"),
        note.map(|n| format!(" ({n})")).unwrap_or_default()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_kir::ObjectKind;

    fn enum_meaning() -> KirObject {
        let mut o = KirObject::new(
            "account.category = 'A'",
            ObjectKind::Custom("EnumMeaning".into()),
        );
        o.properties.insert("label".into(), json!("asset"));
        o.properties.insert(
            "meanings".into(),
            json!([
                {"label":"asset","source":"column_comment","path":"sql/Pg-database.sql","line":72},
                {"label":"L","source":"case_label","path":"sql/modules/FinStatements.sql","line":690}
            ]),
        );
        o
    }

    /// The commit step over a real fact ledger: a label mismatch is recorded with counts, a
    /// decision updates them, and an unchanged re-commit keeps the decision and writes nothing.
    #[test]
    fn commit_step_records_label_mismatches_and_keeps_decisions() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path();
        let mut config = EkosConfig::default();
        config.semantics.enabled = true;
        {
            let ledger = open_store(&config, cwd).unwrap();
            ledger.append_object(&enum_meaning()).unwrap();
            let stats = commit_step(&config, cwd, &*ledger, &[]).unwrap();
            assert_eq!((stats.open, stats.resolved), (1, 0));
            assert!(stats.written > 0);
        }
        resolve(
            &config,
            cwd,
            "account.category = 'A'.label",
            Resolution::Pick(1),
            Some("ann".into()),
            Some("the comment is right; 690 flips the sign".into()),
        )
        .unwrap();
        let c = recorded_counts(&config, cwd).unwrap();
        assert_eq!((c.open, c.resolved), (0, 1));

        let ledger = open_store(&config, cwd).unwrap();
        let stats = commit_step(&config, cwd, &*ledger, &[]).unwrap();
        assert_eq!((stats.open, stats.resolved, stats.written), (0, 1, 0));
        let items = items_in(&config, cwd, &*ledger).unwrap();
        assert_eq!(
            items[0].properties["picked_claim"]["path"],
            "sql/Pg-database.sql"
        );
    }

    /// RFC 0172: settling a conflict stays human-only.
    #[test]
    fn no_mcp_code_can_reach_the_resolve_path() {
        let mcp = include_str!("mcp.rs");
        assert!(
            !mcp.contains("conflicts::resolve"),
            "resolving a conflict must stay CLI-only"
        );
        assert!(!mcp.contains("Resolution::"));
    }
}
