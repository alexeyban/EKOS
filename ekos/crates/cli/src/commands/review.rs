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
use ekos_semantic::workflow::SubjectStatus;
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

fn status(s: Subject, items: &[Item], signoff: SignState) -> SubjectStatus {
    SubjectStatus {
        subject: s.as_str().into(),
        items: items.len(),
        decided: items.iter().filter(|i| i.decision.is_some()).count(),
        outdated: items.iter().filter(|i| i.outdated).count(),
        blocking: items.iter().filter(|i| i.blocking).count(),
        digest: review::subject_digest(s, items),
        signoff,
    }
}

/// Version of the cache's layout; part of its key.
const CACHE_VERSION: u32 = 2;

/// Every subject's items and status, as computed from one state of the ledger.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct ReviewState {
    key: String,
    /// In [`Subject::ALL`] order.
    pub statuses: Vec<SubjectStatus>,
    pub items: std::collections::BTreeMap<String, Vec<Item>>,
}

impl ReviewState {
    fn compute(l: &Loaded, key: String) -> Self {
        let mut statuses = Vec::new();
        let mut items = std::collections::BTreeMap::new();
        for s in Subject::ALL {
            let it = l.items(s);
            statuses.push(status(s, &it, l.state(s, &it)));
            items.insert(s.as_str().to_string(), it);
        }
        Self {
            key,
            statuses,
            items,
        }
    }

    pub fn items(&self, s: Subject) -> &[Item] {
        self.items.get(s.as_str()).map_or(&[], Vec::as_slice)
    }
}

fn cache_path(config: &EkosConfig, cwd: &Path) -> std::path::PathBuf {
    config.ekos_dir(cwd).join("review").join("state.json")
}

/// The key over everything a [`ReviewState`] is computed from: the ledger's entry and
/// object/relationship counts (the ledger is append-only, so any write moves the entry count),
/// the current conflict and semantics ids, and `[review]`.
fn cache_key(config: &EkosConfig, cwd: &Path, ledger: &dyn KnowledgeReader) -> Result<String> {
    let conflicts = super::conflicts::current_ids(config, cwd);
    let semantics = ids_in(&config.ekos_dir(cwd).join("semantics").join("current.json"));
    Ok(ekos_common::ContentHash::of_str(&review::canonical(&json!([
        CACHE_VERSION,
        review::REVIEW_VERSION,
        ledger.entry_count()?,
        ledger.object_count()?,
        ledger.relationship_count()?,
        conflicts,
        semantics,
        serde_json::to_value(&config.review)?,
    ])))
    .0)
}

/// Best-effort: a failed write only means the next read computes again.
fn store_cache(config: &EkosConfig, cwd: &Path, state: &ReviewState) {
    let path = cache_path(config, cwd);
    let _ = std::fs::create_dir_all(path.parent().unwrap_or(cwd)).and_then(|()| {
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec(state).unwrap_or_default())?;
        std::fs::rename(&tmp, &path)
    });
}

/// Every subject's items and status for the ledger as it is now.
///
/// Computing them reads the whole ledger (about 25 s on the iDempiere testbed), so the result is
/// cached in `<state>/review/state.json` under [`cache_key`]. A cache is a memo, not stored
/// validity: a different input is a different key, and the state is recomputed. [`approve`] never
/// reads it; [`decide`], [`approve`] and [`revoke`] write it afresh after their own write.
pub fn review_state(
    config: &EkosConfig,
    cwd: &Path,
    ledger: &dyn KnowledgeReader,
) -> Result<ReviewState> {
    let key = cache_key(config, cwd, ledger)?;
    if let Some(cached) = std::fs::read(cache_path(config, cwd))
        .ok()
        .and_then(|b| serde_json::from_slice::<ReviewState>(&b).ok())
        .filter(|c| c.key == key)
    {
        return Ok(cached);
    }
    let state = ReviewState::compute(&Loaded::read(config, cwd, ledger)?, key);
    store_cache(config, cwd, &state);
    Ok(state)
}

/// Every review subject's counts and sign-off state, in [`Subject::ALL`] order ([`review_state`]).
pub fn subject_statuses(
    config: &EkosConfig,
    cwd: &Path,
    ledger: &dyn KnowledgeReader,
) -> Result<Vec<SubjectStatus>> {
    Ok(review_state(config, cwd, ledger)?.statuses)
}

/// After a write of `written`: the loaded objects with those versions in place, recomputed and
/// cached under the ledger's new key — so the page that shows the result does not read the ledger
/// again. Same function as a fresh read: `all_objects` returns the latest version of each id.
fn refresh_after_write(
    config: &EkosConfig,
    cwd: &Path,
    ledger: &dyn KnowledgeReader,
    mut l: Loaded,
    written: &[KirObject],
) -> Result<()> {
    refresh_with(config, cwd, ledger, &mut l, written, &[])
}

fn refresh_with(
    config: &EkosConfig,
    cwd: &Path,
    ledger: &dyn KnowledgeReader,
    l: &mut Loaded,
    objects: &[KirObject],
    relationships: &[KirRelationship],
) -> Result<()> {
    for o in objects {
        match l.objects.iter_mut().find(|x| x.id == o.id) {
            Some(slot) => *slot = o.clone(),
            None => l.objects.push(o.clone()),
        }
    }
    for r in relationships {
        match l.relationships.iter_mut().find(|x| x.id == r.id) {
            Some(slot) => *slot = r.clone(),
            None => l.relationships.push(r.clone()),
        }
    }
    let l = &*l;
    let state = ReviewState::compute(l, cache_key(config, cwd, ledger)?);
    store_cache(config, cwd, &state);
    Ok(())
}

fn status_json(s: &SubjectStatus) -> Value {
    serde_json::to_value(s).unwrap_or(Value::Null)
}

/// `ekos review list`
pub fn list(config: &EkosConfig, cwd: &Path, json_out: bool) -> Result<()> {
    let ledger = open_store_read_only(config, cwd)?;
    let rows: Vec<Value> = subject_statuses(config, cwd, &*ledger)?
        .iter()
        .map(status_json)
        .collect();
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
    let rs = review_state(config, cwd, &*ledger)?;
    let items = rs.items(s);
    let st = rs
        .statuses
        .iter()
        .find(|x| x.subject == s.as_str())
        .ok_or_else(|| anyhow!("no status for {}", s.as_str()))?;
    let digest = st.digest.clone();
    let state = st.signoff.clone();
    let shown: Vec<&Item> = items
        .iter()
        .filter(|i| !undecided || i.decision.is_none())
        .take(limit.unwrap_or(usize::MAX))
        .collect();
    if json_out {
        let mut v = counts(items);
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
    refresh_after_write(config, cwd, &*ledger, l, &next)?;
    println!(
        "{} {} decision(s) by {by}: {}",
        next.len(),
        s.as_str(),
        review::canonical(&decision)
    );
    Ok(())
}

/// `ekos review identity <match…> --confirm|--reject` — human-only. A match is a `SameAs`
/// candidate, named by its relationship id or its label (`A = B`). The relationship gets a new
/// version with `status`, `reviewed_by`, `reviewed_at` and the note, and an event cites evidence
/// for the decision — the same record `ekos_identity_review` writes, made by a person here.
pub fn decide_identity(
    config: &EkosConfig,
    cwd: &Path,
    targets: &[String],
    confirm: bool,
    note: Option<String>,
    by: Option<String>,
) -> Result<()> {
    let by = reviewer(by)?;
    let decision = if confirm { "confirmed" } else { "rejected" };
    let redaction = config.redaction_config();
    let note = note
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty())
        .map(|n| ekos_common::redaction::redact(&n, &redaction));
    if !confirm && note.is_none() {
        bail!("say why it is not the same thing: --note");
    }
    let ledger = open_store(config, cwd)?;
    let mut l = Loaded::read(config, cwd, &*ledger)?;
    let items = l.items(Subject::Identity);
    let at = chrono::Utc::now();
    let mut next = Vec::new();
    for t in targets {
        let item = find(&items, t)?;
        let id = item
            .id
            .parse::<ekos_kir::KirId>()
            .map_err(|_| anyhow!("{t}: not a relationship id"))?;
        let mut rel = l
            .relationships
            .iter()
            .find(|r| r.id == id)
            .cloned()
            .ok_or_else(|| anyhow!("{t}: relationship not found"))?;
        rel.properties.insert("status".into(), json!(decision));
        rel.properties.insert("reviewed_by".into(), json!(by));
        rel.properties
            .insert("reviewed_at".into(), json!(at.to_rfc3339()));
        rel.properties.insert("review_note".into(), json!(note));
        next.push((item.label.clone(), rel));
    }
    ledger.set_write_context(Some(ekos_ledger::provenance::WriteContext {
        run_id: ekos_ledger::provenance::new_run_id(),
        stage: "review".into(),
        source_artifact_id: None,
    }));
    for (label, rel) in &next {
        ledger.append_relationship(rel)?;
        let mut fragment = format!("ekos review identity: {decision} {label} by {by}");
        if let Some(n) = &note {
            fragment.push_str(&format!(" — {n}"));
        }
        let evidence = ekos_kir::KirEvidence::new(
            ekos_kir::SourceLocation::file("cli:ekos review identity"),
            fragment,
        );
        ledger.append_evidence(&evidence)?;
        ledger.append_event(&ekos_kir::KirEvent {
            id: ekos_kir::KirId(uuid::Uuid::new_v5(
                &uuid::Uuid::NAMESPACE_OID,
                format!(
                    "review-identity:{}:{decision}:{by}:{}",
                    rel.id,
                    at.to_rfc3339()
                )
                .as_bytes(),
            )),
            kind: if confirm {
                ekos_kir::EventKind::Merged
            } else {
                ekos_kir::EventKind::Modified
            },
            subject: rel.id,
            payload: json!({
                "decision": decision,
                "relationship_id": rel.id.to_string(),
                "reviewer": by,
                "note": note,
                "via": "cli",
            }),
            evidence: vec![evidence.id],
            occurred_at: at,
        })?;
    }
    let rels: Vec<KirRelationship> = next.iter().map(|(_, r)| r.clone()).collect();
    refresh_with(config, cwd, &*ledger, &mut l, &[], &rels)?;
    println!("{} identity match(es) {decision} by {by}", next.len());
    Ok(())
}

/// `ekos signoff show <subject>`
pub fn signoff_show(config: &EkosConfig, cwd: &Path, name: &str, json_out: bool) -> Result<()> {
    let s = subject(name)?;
    let ledger = open_store_read_only(config, cwd)?;
    let st = subject_statuses(config, cwd, &*ledger)?
        .into_iter()
        .find(|x| x.subject == s.as_str())
        .ok_or_else(|| anyhow!("no status for {}", s.as_str()))?;
    if json_out {
        println!("{}", serde_json::to_string_pretty(&status_json(&st))?);
        return Ok(());
    }
    println!(
        "{}: {} item(s), {} blocking\ndigest {}\nsign-off: {}",
        s.as_str(),
        st.items,
        st.blocking,
        st.digest,
        state_line(&st.signoff)
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
    let n = items.len();
    refresh_after_write(config, cwd, &*ledger, l, std::slice::from_ref(&o))?;
    println!(
        "{} signed off by {by} on {n} item(s), digest {digest}",
        s.as_str(),
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
    refresh_after_write(config, cwd, &*ledger, l, std::slice::from_ref(&o))?;
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
    use super::*;

    fn fresh(config: &EkosConfig, cwd: &Path) -> ReviewState {
        let ledger = open_store_read_only(config, cwd).unwrap();
        let _ = std::fs::remove_file(cache_path(config, cwd));
        review_state(config, cwd, &*ledger).unwrap()
    }

    fn cached(config: &EkosConfig, cwd: &Path) -> ReviewState {
        serde_json::from_slice(&std::fs::read(cache_path(config, cwd)).unwrap()).unwrap()
    }

    /// The state a decision, a sign-off or a revocation caches after its write is exactly what a
    /// fresh read of the ledger computes — including a scope decision that changes other subjects.
    #[test]
    fn identity_review_from_the_cli_unblocks_the_match_and_records_who() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path();
        let config = EkosConfig::default();
        let rel_id;
        {
            let ledger = open_store(&config, cwd).unwrap();
            let a = KirObject::new("customers", ekos_kir::ObjectKind::Table);
            let b = KirObject::new("cust_mstr", ekos_kir::ObjectKind::Table);
            ledger.append_object(&a).unwrap();
            ledger.append_object(&b).unwrap();
            let mut r = KirRelationship::new(
                ekos_kir::RelationshipKind::Custom("SameAs".into()),
                a.id,
                b.id,
            );
            r.properties.insert("status".into(), json!("unconfirmed"));
            ledger.append_relationship(&r).unwrap();
            rel_id = r.id;
        }
        let before = fresh(&config, cwd);
        let identity = |st: &ReviewState| {
            st.statuses
                .iter()
                .find(|x| x.subject == "identity")
                .unwrap()
                .blocking
        };
        assert_eq!(identity(&before), 1);
        assert!(
            decide_identity(
                &config,
                cwd,
                &["customers = cust_mstr".into()],
                false,
                None,
                Some("ann".into())
            )
            .unwrap_err()
            .to_string()
            .contains("--note")
        );
        decide_identity(
            &config,
            cwd,
            &["customers = cust_mstr".into()],
            true,
            None,
            Some("ann".into()),
        )
        .unwrap();
        let after = cached(&config, cwd);
        assert_eq!(identity(&after), 0);
        let again = fresh(&config, cwd);
        assert_eq!(after.items, again.items);
        assert_eq!(after.key, again.key);
        let ledger = open_store_read_only(&config, cwd).unwrap();
        let r = ledger.get_relationship(&rel_id).unwrap().unwrap();
        assert_eq!(r.properties["status"], "confirmed");
        assert_eq!(r.properties["reviewed_by"], "ann");
    }

    #[test]
    fn the_state_cached_after_a_write_equals_a_fresh_read() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path();
        let config = EkosConfig::default();
        {
            let ledger = open_store(&config, cwd).unwrap();
            for name in ["C_ORDER", "T_REPORT"] {
                let mut o = KirObject::new(name, ekos_kir::ObjectKind::Table);
                o.properties.insert("dialect".into(), json!("oracle"));
                ledger.append_object(&o).unwrap();
            }
        }
        let check = |what: &str| {
            let after = cached(&config, cwd);
            let again = fresh(&config, cwd);
            assert_eq!(after.key, again.key, "{what}: key");
            assert_eq!(after.statuses, again.statuses, "{what}: statuses");
            assert_eq!(after.items, again.items, "{what}: items");
        };
        decide(
            &config,
            cwd,
            Subject::Scope,
            &["T_REPORT".into()],
            json!({"scope": "in"}),
            Some("needed".into()),
            Some("ann".into()),
        )
        .unwrap();
        check("decide");
        let digest = fresh(&config, cwd).statuses[0].digest.clone();
        approve(&config, cwd, "scope", &digest, None, Some("ann".into())).unwrap();
        check("approve");
        assert_eq!(cached(&config, cwd).statuses[0].signoff.name(), "valid");
        revoke(&config, cwd, "scope", "redo", Some("ann".into())).unwrap();
        check("revoke");
    }
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
