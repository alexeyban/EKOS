//! `ekos review …` and `ekos signoff …` — RFC 0213.
//!
//! The subjects, items and digests are `ekos_semantic::review` (pure); this file loads the ledger,
//! prints, and writes the two human-only records: an item decision ([`decide`]) and a sign-off
//! ([`approve`] / [`revoke`]). **Human-only:** `commands/mcp.rs` must never reach either
//! (`review_guard::HUMAN_ONLY`); agents read review state, they never decide it.

use super::store::{open_store, open_store_read_only};
use anyhow::{Result, anyhow, bail};
use ekos_compiler_core::EkosConfig;
use ekos_kir::{KirObject, KirRelationship};
use ekos_ledger::KnowledgeReader;
use ekos_semantic::review::{self, Inputs, Item, ScopeRules, SignState, Subject};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::path::Path;

/// Everything the subjects are computed from, read once.
struct Loaded {
    objects: Vec<KirObject>,
    relationships: Vec<KirRelationship>,
    conflicts: Option<BTreeSet<String>>,
    semantics: Option<BTreeSet<String>>,
    rules: ScopeRules,
}

impl Loaded {
    fn read(config: &EkosConfig, cwd: &Path, ledger: &dyn KnowledgeReader) -> Result<Self> {
        Ok(Self {
            objects: ledger.all_objects()?,
            relationships: ledger.all_relationships()?,
            conflicts: super::conflicts::current_ids(config, cwd),
            semantics: ids_in(&config.ekos_dir(cwd).join("semantics").join("current.json")),
            rules: ScopeRules::from_config(&config.review.scope),
        })
    }

    fn items(&self, subject: Subject) -> Vec<Item> {
        review::items(
            subject,
            &Inputs {
                objects: &self.objects,
                relationships: &self.relationships,
                conflicts_current: self.conflicts.as_ref(),
                semantics_current: self.semantics.as_ref(),
                rules: &self.rules,
            },
        )
    }

    fn state(&self, subject: Subject, items: &[Item]) -> SignState {
        let signed = review::signoffs(&self.objects);
        review::signoff_state(subject, signed.get(subject.as_str()).copied(), items)
    }
}

fn ids_in(path: &Path) -> Option<BTreeSet<String>> {
    let v: Value = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    Some(
        v["ids"]
            .as_array()?
            .iter()
            .filter_map(|x| x.as_str().map(String::from))
            .collect(),
    )
}

fn subject(name: &str) -> Result<Subject> {
    Subject::parse(name).ok_or_else(|| {
        anyhow!(
            "no review subject `{name}`; one of: {}",
            Subject::ALL.map(Subject::as_str).join(", ")
        )
    })
}

fn reviewer(by: Option<String>) -> Result<String> {
    by.or_else(|| std::env::var("USER").ok())
        .filter(|b| !b.trim().is_empty())
        .ok_or_else(|| anyhow!("who is deciding? Pass --as <you>"))
}

fn counts(items: &[Item]) -> Value {
    json!({
        "items": items.len(),
        "decided": items.iter().filter(|i| i.decision.is_some()).count(),
        "outdated": items.iter().filter(|i| i.outdated).count(),
        "blocking": items.iter().filter(|i| i.blocking).count(),
    })
}

fn summary(l: &Loaded, s: Subject) -> Value {
    let items = l.items(s);
    let state = l.state(s, &items);
    let mut v = counts(&items);
    v["subject"] = json!(s.as_str());
    v["digest"] = json!(review::subject_digest(s, &items));
    v["signoff"] = serde_json::to_value(&state).unwrap_or(Value::Null);
    v
}

/// `ekos review list`
pub fn list(config: &EkosConfig, cwd: &Path, json_out: bool) -> Result<()> {
    let ledger = open_store_read_only(config, cwd)?;
    let l = Loaded::read(config, cwd, &*ledger)?;
    let rows: Vec<Value> = Subject::ALL.iter().map(|s| summary(&l, *s)).collect();
    if json_out {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({ "subjects": rows }))?
        );
        return Ok(());
    }
    println!(
        "{:<16} {:>7} {:>8} {:>9} {:>9}  sign-off",
        "subject", "items", "decided", "outdated", "blocking"
    );
    for r in rows {
        println!(
            "{:<16} {:>7} {:>8} {:>9} {:>9}  {}",
            r["subject"].as_str().unwrap_or(""),
            r["items"].as_u64().unwrap_or(0),
            r["decided"].as_u64().unwrap_or(0),
            r["outdated"].as_u64().unwrap_or(0),
            r["blocking"].as_u64().unwrap_or(0),
            r["signoff"]["state"].as_str().unwrap_or("none")
        );
    }
    Ok(())
}

/// `ekos review show <subject>`
pub fn show(
    config: &EkosConfig,
    cwd: &Path,
    name: &str,
    undecided: bool,
    limit: Option<usize>,
    json_out: bool,
) -> Result<()> {
    let s = subject(name)?;
    let ledger = open_store_read_only(config, cwd)?;
    let l = Loaded::read(config, cwd, &*ledger)?;
    let items = l.items(s);
    let digest = review::subject_digest(s, &items);
    let state = l.state(s, &items);
    let shown: Vec<&Item> = items
        .iter()
        .filter(|i| !undecided || i.decision.is_none())
        .take(limit.unwrap_or(usize::MAX))
        .collect();
    if json_out {
        let mut v = counts(&items);
        v["subject"] = json!(s.as_str());
        v["digest"] = json!(digest);
        v["signoff"] = serde_json::to_value(&state)?;
        v["rows"] = serde_json::to_value(&shown)?;
        println!("{}", serde_json::to_string_pretty(&v)?);
        return Ok(());
    }
    println!(
        "{} — {} item(s), digest {}\nsign-off: {}",
        s.as_str(),
        items.len(),
        digest,
        state_line(&state)
    );
    for i in shown {
        let facts: Vec<String> = i
            .facts
            .iter()
            .chain(i.info.iter())
            .filter(|(_, v)| !v.is_null())
            .map(|(k, v)| format!("{k}={}", v.as_str().map_or(v.to_string(), String::from)))
            .collect();
        let decision = match (&i.decision, i.outdated, &i.proposed) {
            (Some(d), _, _) => format!("decided {}", review::canonical(d)),
            (None, true, _) => "decision outdated".into(),
            (None, false, Some(p)) => format!("proposed {p}"),
            _ => String::new(),
        };
        println!(
            "  {}{}  {}  {}",
            if i.blocking { "! " } else { "" },
            i.label,
            facts.join(" "),
            decision
        );
    }
    Ok(())
}

fn state_line(state: &SignState) -> String {
    match state {
        SignState::None => "none".into(),
        SignState::Valid {
            signed_by,
            signed_at,
        } => format!("valid ({signed_by}, {signed_at})"),
        SignState::Stale {
            signed_by,
            signed_at,
            added,
            removed,
            changed,
        } => format!(
            "STALE ({signed_by}, {signed_at}): {} added, {} removed, {} changed{}",
            added.len(),
            removed.len(),
            changed.len(),
            changed
                .first()
                .or(added.first())
                .map(|x| format!(" — e.g. {x}"))
                .unwrap_or_default()
        ),
        SignState::Revoked {
            signed_by, reason, ..
        } => format!("revoked by {signed_by}: {reason}"),
    }
}

/// The item a target names: its id, its label, or its id without the `table:`/`column:` prefix,
/// case-insensitive.
fn find<'a>(items: &'a [Item], target: &str) -> Result<&'a Item> {
    let t = target.to_ascii_lowercase();
    items
        .iter()
        .find(|i| {
            i.id.to_ascii_lowercase() == t
                || i.label.to_ascii_lowercase() == t
                || i.id.split_once(':').is_some_and(|(_, rest)| rest == t)
        })
        .ok_or_else(|| anyhow!("no item `{target}` in this review"))
}

/// `ekos review scope|pii|write-behaviour …` — human-only. One new decision per target.
pub fn decide(
    config: &EkosConfig,
    cwd: &Path,
    s: Subject,
    targets: &[String],
    decision: Value,
    reason: Option<String>,
    by: Option<String>,
) -> Result<()> {
    if !s.decided_here() {
        bail!("{} items are decided with their own command", s.as_str());
    }
    let decision = review::check_decision(s, &decision).map_err(|e| anyhow!(e))?;
    let by = reviewer(by)?;
    let ledger = open_store(config, cwd)?;
    let l = Loaded::read(config, cwd, &*ledger)?;
    let items = l.items(s);
    let at = chrono::Utc::now().to_rfc3339();
    let mut next = Vec::new();
    for t in targets {
        let item = find(&items, t)?;
        next.push(
            review::decision_object(s, item, decision.clone(), reason.as_deref(), &by, &at)
                .map_err(|e| anyhow!("{t}: {e}"))?,
        );
    }
    ledger.set_write_context(Some(ekos_ledger::provenance::WriteContext {
        run_id: ekos_ledger::provenance::new_run_id(),
        stage: "review".into(),
        source_artifact_id: None,
    }));
    for o in &next {
        ledger.append_object(o)?;
    }
    println!(
        "{} {} decision(s) by {by}: {}",
        next.len(),
        s.as_str(),
        review::canonical(&decision)
    );
    Ok(())
}

/// `ekos signoff show <subject>`
pub fn signoff_show(config: &EkosConfig, cwd: &Path, name: &str, json_out: bool) -> Result<()> {
    let s = subject(name)?;
    let ledger = open_store_read_only(config, cwd)?;
    let l = Loaded::read(config, cwd, &*ledger)?;
    let v = summary(&l, s);
    if json_out {
        println!("{}", serde_json::to_string_pretty(&v)?);
        return Ok(());
    }
    let items = l.items(s);
    println!(
        "{}: {} item(s), {} blocking\ndigest {}\nsign-off: {}",
        s.as_str(),
        v["items"],
        v["blocking"],
        v["digest"].as_str().unwrap_or(""),
        state_line(&l.state(s, &items))
    );
    Ok(())
}

/// `ekos signoff approve <subject> --digest <hex>` — human-only.
pub fn approve(
    config: &EkosConfig,
    cwd: &Path,
    name: &str,
    digest: &str,
    note: Option<String>,
    by: Option<String>,
) -> Result<()> {
    let s = subject(name)?;
    let by = reviewer(by)?;
    let ledger = open_store(config, cwd)?;
    let l = Loaded::read(config, cwd, &*ledger)?;
    let items = l.items(s);
    let digest = review::approve_check(s, &items, digest).map_err(|e| anyhow!(e))?;
    let at = chrono::Utc::now().to_rfc3339();
    let o = review::signoff_object(s, &digest, &items, &by, &at, note.as_deref(), None)
        .map_err(|e| anyhow!(e))?;
    write_signoff(&*ledger, &o)?;
    println!(
        "{} signed off by {by} on {} item(s), digest {digest}",
        s.as_str(),
        items.len()
    );
    Ok(())
}

/// `ekos signoff revoke <subject> --reason` — human-only.
pub fn revoke(
    config: &EkosConfig,
    cwd: &Path,
    name: &str,
    reason: &str,
    by: Option<String>,
) -> Result<()> {
    let s = subject(name)?;
    if reason.trim().is_empty() {
        bail!("say why: --reason");
    }
    let by = reviewer(by)?;
    let ledger = open_store(config, cwd)?;
    let l = Loaded::read(config, cwd, &*ledger)?;
    let items = l.items(s);
    if matches!(l.state(s, &items), SignState::None) {
        bail!("{} has no sign-off to revoke", s.as_str());
    }
    let digest = review::subject_digest(s, &items);
    let at = chrono::Utc::now().to_rfc3339();
    let o = review::signoff_object(s, &digest, &items, &by, &at, None, Some(reason))
        .map_err(|e| anyhow!(e))?;
    write_signoff(&*ledger, &o)?;
    println!("{} sign-off revoked by {by}: {reason}", s.as_str());
    Ok(())
}

fn write_signoff(ledger: &dyn ekos_ledger::KnowledgeStore, o: &KirObject) -> Result<()> {
    ledger.set_write_context(Some(ekos_ledger::provenance::WriteContext {
        run_id: ekos_ledger::provenance::new_run_id(),
        stage: "signoff".into(),
        source_artifact_id: None,
    }));
    ledger.append_object(o)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn no_mcp_code_can_reach_the_decision_or_sign_off_paths() {
        let mcp = include_str!("mcp.rs");
        for path in [
            "review::decide",
            "review::approve",
            "review::revoke",
            "decision_object",
            "signoff_object",
        ] {
            assert!(!mcp.contains(path), "commands/mcp.rs names {path}");
        }
    }
}
