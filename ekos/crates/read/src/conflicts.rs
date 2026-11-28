//! Reading conflicts (RFC 0172 / 0173): which are current, which are open about an object, and
//! which claims of an answer they dispute. Moved from the CLI (RFC 0269 phase 1) so every reader
//! of the ledger marks disputed claims the same way. Deciding a conflict is not here: that is a
//! person's act, in the CLI.

use crate::Result;
use ekos_compiler_core::EkosConfig;
use ekos_kir::{KirId, KirObject};
use ekos_ledger::KnowledgeStore;
use ekos_semantic::conflicts::{self, DISMISSED, OPEN, RESOLVED};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub fn manifest_path(config: &EkosConfig, cwd: &Path) -> PathBuf {
    config.ekos_dir(cwd).join("conflicts").join("current.json")
}

/// Phase 3: doc-vs-data conflicts belong to `ekos migrate assess`, not to `commit`, so they live in
/// their own manifest (`table → ids`) that a commit never overwrites.
pub fn migrate_manifest_path(config: &EkosConfig, cwd: &Path) -> PathBuf {
    config.ekos_dir(cwd).join("conflicts").join("migrate.json")
}

pub fn migrate_ids(config: &EkosConfig, cwd: &Path) -> BTreeSet<String> {
    std::fs::read(migrate_manifest_path(config, cwd))
        .ok()
        .and_then(|b| serde_json::from_slice::<BTreeMap<String, Vec<String>>>(&b).ok())
        .map(|m| m.into_values().flatten().collect())
        .unwrap_or_default()
}

pub fn s<'a>(o: &'a KirObject, key: &str) -> &'a str {
    o.properties.get(key).and_then(Value::as_str).unwrap_or("")
}

/// What is current: the commit's conflicts plus Migrate's. `None` when neither manifest exists yet
/// (then every conflict in the ledger counts).
pub fn current_ids(config: &EkosConfig, cwd: &Path) -> Option<BTreeSet<String>> {
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

pub fn commit_ids(config: &EkosConfig, cwd: &Path) -> Option<BTreeSet<String>> {
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
    let mut out: Vec<Value> = open_items_for(&current_ids(config, cwd), ledger, subject)?
        .iter()
        .map(|l| brief(&l.item))
        .collect();
    out.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    Ok(out)
}

/// An open conflict reached from `subject`, with the attribute and subject name as that link
/// states them: a label conflict reaches its table through an RFC 0173 column link, where the
/// attribute is `columns.<column>.meaning`, not the conflict's own `label`.
pub struct Linked {
    pub item: KirObject,
    pub attribute: String,
    pub subject: String,
}

pub fn open_items_for(
    current: &Option<BTreeSet<String>>,
    ledger: &dyn KnowledgeStore,
    subject: &KirId,
) -> Result<Vec<Linked>> {
    let mut out: Vec<Linked> = Vec::new();
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
            let stated = |k: &str| {
                rel.properties
                    .get(k)
                    .and_then(Value::as_str)
                    .unwrap_or_else(|| s(&o, k))
                    .to_string()
            };
            out.push(Linked {
                attribute: stated("attribute"),
                subject: stated("subject_name"),
                item: o,
            });
        }
    }
    Ok(out)
}

/// RFC 0173 — mark the claims in a query answer whose attribute has an open conflict. One
/// `Disputes` lookup per distinct entity in the set; the manifests are read once.
pub fn flag_disputed(
    config: &EkosConfig,
    cwd: &Path,
    ledger: &dyn KnowledgeStore,
    evidence: &mut ekos_runtime::reason::EvidenceSet,
) -> Result<usize> {
    let current = current_ids(config, cwd);
    evidence.mark_disputed(|entity| disputes_of(&current, ledger, entity))
}

pub fn disputes_of(
    current: &Option<BTreeSet<String>>,
    ledger: &dyn KnowledgeStore,
    entity: &KirId,
) -> Result<Vec<ekos_runtime::reason::OpenDispute>> {
    Ok(open_items_for(current, ledger, entity)?
        .into_iter()
        .map(|l| ekos_runtime::reason::OpenDispute {
            conflict: l.item.id,
            subject: l.subject,
            attribute: l.attribute,
        })
        .collect())
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
        // RFC 0211: what in the source tree accounts for it, when something does.
        "explanation": o.properties.get("explanation"),
    })
}
