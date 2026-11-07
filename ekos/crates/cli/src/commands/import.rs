//! `ekos import linkml <file>` — RFC 0170 Phase 4 round trip: an expert's edits to an exported
//! LinkML schema become review decisions in the ledger, so the YAML never becomes a second source
//! of truth.
//!
//! The edited file is compared, element by element, with the schema `ekos export linkml` would
//! produce right now (same status filter, read from the file's `ekos_export_status` annotation).
//! Elements are matched by their `ekos_id` annotation, never by name — a renamed class is still
//! the same concept.
//!
//! | Edit in the YAML | Decision |
//! |---|---|
//! | concept class renamed | `edit --name` |
//! | concept `description` changed | `edit --description` |
//! | permissible value `description` changed | `edit --label` |
//! | `ekos_status: confirmed` | `confirm` |
//! | `ekos_status: rejected` (+ `ekos_review_note`) | `reject` |
//! | `ekos_review_note` changed | recorded with the decision |
//!
//! Everything else — new classes, removed elements, slot changes — is reported and not imported:
//! EKOS records decisions about what it recovered, nothing more. Validation is all-or-nothing: if
//! any element is in error, nothing is written. **Human-only**, like `ekos semantics confirm`.
//!
//! `ekos import ossie <file>` (RFC 0245) does the same for an exported Apache Ossie model: the
//! decisions live in the `EKOS` extensions' `concepts` (`name`, `description`, `status`, `note`)
//! and `enum_meanings` (`label`, `status`, `note`), matched by `ekos_id`; edits anywhere else are
//! observed facts, reported and not imported.

use super::export::{StatusFilter, build_schema};
use super::semantics::{current_items, current_items_for_write};
use anyhow::{Context, Result};
use ekos_compiler_core::EkosConfig;
use ekos_kir::KirObject;
use ekos_semantic::semantics_review::{self, Decision};
use serde::Serialize;
use serde_yaml::Value;
use std::collections::BTreeMap;
use std::path::Path;

/// One element of a schema that carries an `ekos_id`.
#[derive(Debug, Clone, PartialEq, Default)]
struct Element {
    kind: &'static str,
    /// Where it is in the schema, for messages: `classes.InventoryPart`.
    path: String,
    /// Class or permissible-value key.
    key: String,
    description: Option<String>,
    status: Option<String>,
    note: Option<String>,
}

/// One planned decision.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Planned {
    pub id: String,
    pub kind: String,
    pub path: String,
    /// `confirm`, `reject` or `edit`.
    pub action: String,
    /// field → [before, after].
    pub changes: BTreeMap<String, [Option<String>; 2]>,
    pub note: Option<String>,
}

/// What an import would do.
#[derive(Debug, Clone, Serialize, Default)]
pub struct ImportPlan {
    pub decisions: Vec<Planned>,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
    pub applied: usize,
}

/// An annotation's value: LinkML allows both `tag: value` and `tag: {tag:, value:}`.
fn annotation(node: &Value, tag: &str) -> Option<String> {
    let v = node.get("annotations")?.get(tag)?;
    let v = v.get("value").unwrap_or(v);
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Null => None,
        other => serde_yaml::to_string(other)
            .ok()
            .map(|s| s.trim().to_string()),
    }
}

fn text(node: &Value, key: &str) -> Option<String> {
    node.get(key)
        .and_then(Value::as_str)
        .map(|s| s.trim().to_string())
}

fn key_str(k: &Value) -> String {
    match k {
        Value::String(s) => s.clone(),
        other => serde_yaml::to_string(other)
            .unwrap_or_default()
            .trim()
            .to_string(),
    }
}

/// Every `ekos_id`-carrying element of a schema, by id.
fn elements(schema: &Value, errors: &mut Vec<String>) -> BTreeMap<String, Element> {
    let mut out = BTreeMap::new();
    let mut put = |id: String, e: Element, errors: &mut Vec<String>| {
        if let Some(prev) = out.insert(id.clone(), e.clone()) {
            errors.push(format!(
                "ekos_id {id} appears twice ({} and {})",
                prev.path, e.path
            ));
        }
    };
    if let Some(classes) = schema.get("classes").and_then(Value::as_mapping) {
        for (k, c) in classes {
            let key = key_str(k);
            if let Some(id) = annotation(c, "ekos_id") {
                put(
                    id,
                    Element {
                        kind: "concept",
                        path: format!("classes.{key}"),
                        key: key.clone(),
                        description: text(c, "description"),
                        status: annotation(c, "ekos_status"),
                        note: annotation(c, "ekos_review_note"),
                    },
                    errors,
                );
            }
            if let Some(usage) = c.get("slot_usage").and_then(Value::as_mapping) {
                for (sk, su) in usage {
                    if let Some(id) = annotation(su, "ekos_id") {
                        put(
                            id,
                            Element {
                                kind: "constraint",
                                path: format!("classes.{key}.slot_usage.{}", key_str(sk)),
                                key: key_str(sk),
                                description: None,
                                status: annotation(su, "ekos_status"),
                                note: annotation(su, "ekos_review_note"),
                            },
                            errors,
                        );
                    }
                }
            }
        }
    }
    if let Some(enums) = schema.get("enums").and_then(Value::as_mapping) {
        for (ek, e) in enums {
            let Some(pvs) = e.get("permissible_values").and_then(Value::as_mapping) else {
                continue;
            };
            for (pk, pv) in pvs {
                if let Some(id) = annotation(pv, "ekos_id") {
                    let key = key_str(pk);
                    put(
                        id,
                        Element {
                            kind: "enum",
                            path: format!("enums.{}.permissible_values.{key}", key_str(ek)),
                            key,
                            description: text(pv, "description"),
                            status: annotation(pv, "ekos_status"),
                            note: annotation(pv, "ekos_review_note"),
                        },
                        errors,
                    );
                }
            }
        }
    }
    out
}

type Changes = BTreeMap<String, [Option<String>; 2]>;

/// The edits to an element's own fields, as (before, after) pairs. `None` when the edit is one
/// that cannot be made, which is reported in `errors`.
fn field_changes(e: &Element, b: &Element, errors: &mut Vec<String>) -> Option<Changes> {
    let mut changes = Changes::new();
    let described = e.description != b.description;
    match e.kind {
        "concept" => {
            if e.key != b.key {
                changes.insert("name".into(), [Some(b.key.clone()), Some(e.key.clone())]);
            }
            if described {
                changes.insert(
                    "description".into(),
                    [b.description.clone(), e.description.clone()],
                );
            }
        }
        "enum" if e.key != b.key => {
            errors.push(format!(
                "{}: a code is the stored value and cannot be renamed ({} → {}); change \
                 its description to relabel it",
                e.path, b.key, e.key
            ));
            return None;
        }
        "enum" if described => {
            changes.insert(
                "label".into(),
                [b.description.clone(), e.description.clone()],
            );
        }
        _ => {}
    }
    Some(changes)
}

/// The review decision an edited element amounts to, or `None` when there is none to record
/// (nothing changed, or the change is refused and reported in `p`). A rejection drops the other
/// edits.
fn decide_action(
    e: &Element,
    b: &Element,
    changes: &mut Changes,
    p: &mut ImportPlan,
) -> Option<&'static str> {
    let note_changed = e.note != b.note;
    let status_changed = e.status != b.status;
    let wanted = e.status.as_deref().unwrap_or("hypothesis");
    if status_changed && wanted == "rejected" {
        if !changes.is_empty() {
            p.warnings.push(format!(
                "{}: rejected — the other edits to it are ignored",
                e.path
            ));
        }
        if e.note.as_deref().is_none_or(|n| n.trim().is_empty()) {
            p.errors.push(format!(
                "{}: a rejection needs a reason in `ekos_review_note`",
                e.path
            ));
            return None;
        }
        changes.clear();
        return Some("reject");
    }
    if !changes.is_empty() {
        return Some("edit");
    }
    if status_changed && wanted == "confirmed" {
        return Some("confirm");
    }
    if status_changed {
        p.warnings.push(format!(
            "{}: `ekos_status: {wanted}` cannot be set by hand — only confirmed or rejected",
            e.path
        ));
        return None;
    }
    (note_changed && b.status.as_deref() == Some("confirmed")).then_some("confirm")
}

/// Compare an edited schema with the current export and plan the decisions. Pure.
pub fn plan(baseline: &Value, edited: &Value) -> ImportPlan {
    let mut p = ImportPlan::default();
    let mut ignored = Vec::new();
    let base = elements(baseline, &mut ignored);
    let new = elements(edited, &mut p.errors);

    // Elements a human added carry no ekos_id: report them, import nothing.
    let added = |sel: &str| -> usize {
        edited
            .get(sel)
            .and_then(Value::as_mapping)
            .map(|m| {
                m.iter()
                    .filter(|(k, _)| {
                        baseline
                            .get(sel)
                            .and_then(Value::as_mapping)
                            .is_none_or(|b| !b.contains_key(*k))
                    })
                    .filter(|(_, v)| annotation(v, "ekos_id").is_none())
                    .count()
            })
            .unwrap_or(0)
    };
    for sel in ["classes", "enums"] {
        let n = added(sel);
        if n > 0 {
            p.warnings.push(format!(
                "{n} new {sel} without an ekos_id are not imported — EKOS records decisions about \
                 what it recovered; add new definitions in your LinkML sources"
            ));
        }
    }
    let missing = base.keys().filter(|id| !new.contains_key(*id)).count();
    if missing > 0 {
        p.warnings.push(format!(
            "{missing} recovered element(s) are not in the file and are left unchanged — deleting \
             is not a decision; set `ekos_status: rejected` to reject one"
        ));
    }

    for (id, e) in &new {
        let Some(b) = base.get(id) else {
            p.errors.push(format!(
                "{}: ekos_id {id} is not a current item (stale export? re-export and redo the edit)",
                e.path
            ));
            continue;
        };
        let Some(mut changes) = field_changes(e, b, &mut p.errors) else {
            continue;
        };
        let status_changed = e.status != b.status;
        let Some(action) = decide_action(e, b, &mut changes, &mut p) else {
            continue;
        };
        if status_changed {
            changes.insert("status".into(), [b.status.clone(), e.status.clone()]);
        }
        p.decisions.push(Planned {
            id: id.clone(),
            kind: e.kind.into(),
            path: e.path.clone(),
            action: action.into(),
            changes,
            note: e
                .note
                .clone()
                .filter(|n| Some(n) != b.note.as_ref() || action == "reject"),
        });
    }
    p
}

/// The `Decision` a planned row stands for.
fn decision_for(row: &Planned) -> Decision {
    match row.action.as_str() {
        "reject" => Decision::Reject,
        "confirm" => Decision::Confirm,
        _ => {
            let new = |f: &str| row.changes.get(f).and_then(|c| c[1].clone());
            Decision::Edit {
                name: new("name"),
                description: new("description"),
                label: new("label"),
            }
        }
    }
}

/// Options for one import.
pub struct ImportOptions<'a> {
    pub file: &'a Path,
    pub dry_run: bool,
    pub json: bool,
    pub by: Option<String>,
}

/// `ekos import linkml`.
pub fn linkml(config: &EkosConfig, cwd: &Path, opts: &ImportOptions) -> Result<()> {
    let text = std::fs::read_to_string(opts.file)
        .with_context(|| format!("reading {}", opts.file.display()))?;
    let mut plan_out = match serde_yaml::from_str::<Value>(&text) {
        Ok(edited) if edited.is_mapping() => {
            let status = match annotation(&edited, "ekos_export_status").as_deref() {
                Some("confirmed") => StatusFilter::Confirmed,
                Some("hypothesis") => StatusFilter::Hypothesis,
                _ => StatusFilter::All,
            };
            let name = edited
                .get("name")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| super::export::default_schema_name(cwd));
            let (ledger, items) = current_items(config, cwd)?;
            let vocab = super::semantics::load_vocabulary(config, cwd)?;
            let baseline = build_schema(&*ledger, &items, status, &name, &vocab)?
                .map(|v| serde_yaml::to_value(v).unwrap_or(Value::Null))
                .unwrap_or(Value::Null);
            drop(ledger);
            plan(&baseline, &edited)
        }
        Ok(_) => ImportPlan {
            errors: vec!["the file is not a LinkML schema (a YAML mapping)".into()],
            ..Default::default()
        },
        Err(e) => ImportPlan {
            errors: vec![format!("YAML does not parse: {e}")],
            ..Default::default()
        },
    };

    let source = format!(
        "linkml:{}",
        ekos_common::ContentHash::of_str(&text).as_str()
    );
    apply_and_report(
        config,
        cwd,
        opts,
        &mut plan_out,
        "semantics-review:import-linkml",
        source,
    )
}

/// Apply a plan's decisions (unless a dry run, or any error) and print what happened. Every
/// decision is validated before anything is written: all or nothing. Shared by every import.
fn apply_and_report(
    config: &EkosConfig,
    cwd: &Path,
    opts: &ImportOptions,
    plan_out: &mut ImportPlan,
    stage: &str,
    source_artifact_id: String,
) -> Result<()> {
    if !opts.dry_run && plan_out.errors.is_empty() && !plan_out.decisions.is_empty() {
        apply(config, cwd, opts, plan_out, stage, source_artifact_id)?;
    }
    report(opts, plan_out)
}

fn apply(
    config: &EkosConfig,
    cwd: &Path,
    opts: &ImportOptions,
    plan_out: &mut ImportPlan,
    stage: &str,
    source_artifact_id: String,
) -> Result<()> {
    let by = opts
        .by
        .clone()
        .or_else(|| std::env::var("USER").ok())
        .filter(|b| !b.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("who is reviewing? Pass --as <you>"))?;
    let (ledger, items) = current_items_for_write(config, cwd)?;
    let by_id: BTreeMap<String, &KirObject> = items.iter().map(|o| (o.id.to_string(), o)).collect();
    let at = chrono::Utc::now().to_rfc3339();
    let mut next = Vec::new();
    for row in &plan_out.decisions {
        let current = by_id
            .get(&row.id)
            .ok_or_else(|| anyhow::anyhow!("{}: item vanished", row.path))?;
        match semantics_review::apply_review(
            current,
            &decision_for(row),
            &by,
            &at,
            row.note.as_deref(),
        ) {
            Ok(o) => next.push(o),
            Err(e) => plan_out.errors.push(format!("{}: {e}", row.path)),
        }
    }
    if plan_out.errors.is_empty() {
        ledger.set_write_context(Some(ekos_ledger::provenance::WriteContext {
            run_id: ekos_ledger::provenance::new_run_id(),
            stage: stage.into(),
            source_artifact_id: Some(source_artifact_id),
        }));
        for o in &next {
            ledger.append_object(o)?;
        }
        plan_out.applied = next.len();
    }
    Ok(())
}

fn report(opts: &ImportOptions, plan_out: &ImportPlan) -> Result<()> {
    if opts.json {
        println!("{}", serde_json::to_string_pretty(plan_out)?);
    } else {
        for d in &plan_out.decisions {
            let changes: Vec<String> = d
                .changes
                .iter()
                .map(|(k, [a, b])| {
                    format!(
                        "{k}: {} → {}",
                        a.as_deref().unwrap_or("∅"),
                        b.as_deref().unwrap_or("∅")
                    )
                })
                .collect();
            println!("{:8} {}  {}", d.action, d.path, changes.join("; "));
        }
        for w in &plan_out.warnings {
            println!("warning: {w}");
        }
        for e in &plan_out.errors {
            println!("error:   {e}");
        }
        println!(
            "{} decision(s) planned, {} applied{}.",
            plan_out.decisions.len(),
            plan_out.applied,
            if opts.dry_run { " (dry run)" } else { "" }
        );
    }
    if !plan_out.errors.is_empty() && !opts.json {
        anyhow::bail!("{} error(s); nothing was written", plan_out.errors.len());
    }
    Ok(())
}

// ── RFC 0245: `ekos import ossie` ──────────────────────────────────────────────────────────────

/// One reviewable entry of an exported Ossie model: a business concept on a dataset or a code
/// meaning on a field, from the `EKOS` extensions — the only parts of the file that are decisions.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct OssieEntry {
    kind: &'static str,
    path: String,
    name: Option<String>,
    description: Option<String>,
    label: Option<String>,
    status: Option<String>,
    note: Option<String>,
}

fn ext_str(v: &serde_json::Value, k: &str) -> Option<String> {
    v.get(k)
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .filter(|s| !s.is_empty())
}

/// `ekos_id` → entry, for every concept and code meaning in `m`. A malformed extension is an error.
fn ossie_entries(
    m: &ekos_ossie::SemanticModel,
    errors: &mut Vec<String>,
) -> BTreeMap<String, OssieEntry> {
    let mut out = BTreeMap::new();
    let mut add = |id: Option<String>, e: OssieEntry, errors: &mut Vec<String>| match id {
        Some(id) => {
            out.insert(id, e);
        }
        None => errors.push(format!("{}: an entry without `ekos_id`", e.path)),
    };
    let data = |exts: &[ekos_ossie::CustomExtension], path: &str, errors: &mut Vec<String>| {
        let has = exts
            .iter()
            .any(|x| x.vendor_name == ekos_ossie::EKOS_VENDOR);
        let parsed = ekos_ossie::ekos_data(exts);
        if has && parsed.is_none() {
            errors.push(format!(
                "{path}: the EKOS extension's `data` is not valid JSON"
            ));
        }
        parsed
    };
    for d in &m.datasets {
        let at = format!("datasets.{}", d.name);
        if let Some(x) = data(&d.custom_extensions, &at, errors) {
            for c in x["concepts"].as_array().into_iter().flatten() {
                let name = ext_str(c, "name");
                let e = OssieEntry {
                    kind: "BusinessConcept",
                    path: format!("{at}.concepts.{}", name.as_deref().unwrap_or("?")),
                    name,
                    description: ext_str(c, "description"),
                    status: ext_str(c, "status"),
                    note: ext_str(c, "note"),
                    ..Default::default()
                };
                add(ext_str(c, "ekos_id"), e, errors);
            }
        }
        for f in &d.fields {
            let at = format!("{at}.fields.{}", f.name);
            let Some(x) = data(&f.custom_extensions, &at, errors) else {
                continue;
            };
            for v in x["enum_meanings"].as_array().into_iter().flatten() {
                let e = OssieEntry {
                    kind: "EnumMeaning",
                    path: format!("{at}.{}", ext_str(v, "value").unwrap_or_default()),
                    label: ext_str(v, "label"),
                    status: ext_str(v, "status"),
                    note: ext_str(v, "note"),
                    ..Default::default()
                };
                add(ext_str(v, "ekos_id"), e, errors);
            }
        }
    }
    out
}

/// A dataset with everything EKOS generates or carries for review removed: what is left is the
/// observed schema, which a decision cannot change.
fn observed_part(d: &ekos_ossie::Dataset) -> ekos_ossie::Dataset {
    let mut d = d.clone();
    d.custom_extensions.clear();
    d.ai_context = None;
    for f in &mut d.fields {
        f.custom_extensions.clear();
        f.ai_context = None;
    }
    for m in &mut d.metrics {
        m.custom_extensions.clear();
    }
    d
}

/// Warnings for edits to observed facts and for added or removed datasets: reported, never
/// imported.
fn observed_edits(
    baseline: &ekos_ossie::SemanticModel,
    edited: &ekos_ossie::SemanticModel,
    p: &mut ImportPlan,
) {
    let id_of = |d: &ekos_ossie::Dataset| {
        ekos_ossie::ekos_data(&d.custom_extensions).and_then(|x| ext_str(&x, "ekos_id"))
    };
    let base: BTreeMap<String, &ekos_ossie::Dataset> = baseline
        .datasets
        .iter()
        .filter_map(|d| Some((id_of(d)?, d)))
        .collect();
    let mut added = 0;
    for d in &edited.datasets {
        match id_of(d).and_then(|id| base.get(&id).copied()) {
            None => added += 1,
            Some(b) if observed_part(b) != observed_part(d) => p.warnings.push(format!(
                "datasets.{}: edits to the observed schema (fields, types, keys, descriptions, \
                 metrics) are not imported — change the source, or keep an Apache Ossie model \
                 in the project's sources",
                d.name
            )),
            Some(_) => {}
        }
    }
    if added > 0 {
        p.warnings.push(format!(
            "{added} dataset(s) without a current EKOS id are not imported — EKOS records \
             decisions about what it recovered"
        ));
    }
    if edited.relationships != baseline.relationships || edited.metrics != baseline.metrics {
        p.warnings.push(
            "relationship or model-metric edits are not imported — they are observed facts".into(),
        );
    }
}

/// Every current RFC 0170 concept and code meaning as the entry an export would write for it —
/// rejected ones included, which an export leaves out: re-importing a file whose rejection is
/// already recorded must not call that entry stale.
fn known_entries(items: &[KirObject]) -> BTreeMap<String, OssieEntry> {
    let text = |o: &KirObject, keys: &[&str]| {
        keys.iter()
            .filter_map(|k| o.properties.get(*k).and_then(serde_json::Value::as_str))
            .find(|v| !v.is_empty())
            .map(str::to_string)
    };
    items
        .iter()
        .filter_map(|o| {
            let common = OssieEntry {
                status: text(o, &["status"]),
                note: text(o, &["review_note"]),
                ..Default::default()
            };
            let entry = if ekos_ossie::kinds::is_concept(o) {
                OssieEntry {
                    kind: "BusinessConcept",
                    name: text(o, &["expert_name"]).or_else(|| Some(o.name.clone())),
                    description: text(o, &["expert_description", "description"]),
                    ..common
                }
            } else if ekos_ossie::kinds::is_enum_meaning(o) {
                OssieEntry {
                    kind: "EnumMeaning",
                    label: text(o, &["expert_label", "label"]),
                    ..common
                }
            } else {
                return None;
            };
            Some((o.id.to_string(), entry))
        })
        .collect()
}

/// Compare an edited Ossie export with the current one and plan the decisions. Pure. Entries
/// are compared with `known` (every current item, see [`known_entries`]); the baseline export
/// decides only what counts as an observed-schema edit and what is missing from the file.
pub fn plan_ossie(
    baseline: &ekos_ossie::SemanticModel,
    edited: &ekos_ossie::SemanticModel,
    known: &BTreeMap<String, OssieEntry>,
) -> ImportPlan {
    let mut p = ImportPlan::default();
    let mut ignored = Vec::new();
    let exported = ossie_entries(baseline, &mut ignored);
    let base = known;
    let new = ossie_entries(edited, &mut p.errors);
    observed_edits(baseline, edited, &mut p);
    let missing = exported.keys().filter(|id| !new.contains_key(*id)).count();
    if missing > 0 {
        p.warnings.push(format!(
            "{missing} recovered meaning(s) are not in the file and are left unchanged — deleting \
             is not a decision; set `\"status\": \"rejected\"` to reject one"
        ));
    }
    for (id, e) in &new {
        let Some(b) = base.get(id) else {
            p.errors.push(format!(
                "{}: ekos_id {id} is not a current item (stale export? re-export and redo the edit)",
                e.path
            ));
            continue;
        };
        let mut changes: BTreeMap<String, [Option<String>; 2]> = BTreeMap::new();
        for (field, before, after) in [
            ("name", &b.name, &e.name),
            ("description", &b.description, &e.description),
            ("label", &b.label, &e.label),
        ] {
            if before != after {
                if after.is_none() {
                    p.errors
                        .push(format!("{}: `{field}` cannot be emptied", e.path));
                    continue;
                }
                changes.insert(field.into(), [before.clone(), after.clone()]);
            }
        }
        let status_changed = e.status != b.status;
        let wanted = e.status.as_deref().unwrap_or("");
        let action = if status_changed && wanted == "rejected" {
            if !changes.is_empty() {
                p.warnings.push(format!(
                    "{}: rejected, so its other edits are not recorded",
                    e.path
                ));
                changes.clear();
            }
            Some("reject")
        } else if !changes.is_empty() {
            Some("edit")
        } else if status_changed && wanted == "confirmed" {
            Some("confirm")
        } else if status_changed && matches!(wanted, "hypothesis" | "needs_review" | "") {
            // What an untouched entry of an older export looks like: no decision.
            p.warnings.push(format!(
                "{}: the file says `{wanted}`, the ledger `{}` — an older export; left unchanged",
                e.path,
                b.status.as_deref().unwrap_or("?")
            ));
            None
        } else if status_changed {
            p.warnings.push(format!(
                "{}: `status: {wanted}` cannot be set by hand — only confirmed or rejected",
                e.path
            ));
            None
        } else {
            let note_changed = e.note.is_some() && e.note != b.note;
            (note_changed && b.status.as_deref() == Some("confirmed")).then_some("confirm")
        };
        let Some(action) = action else { continue };
        if status_changed {
            changes.insert("status".into(), [b.status.clone(), e.status.clone()]);
        }
        p.decisions.push(Planned {
            id: id.clone(),
            kind: e.kind.into(),
            path: e.path.clone(),
            action: action.into(),
            changes,
            note: e
                .note
                .clone()
                .filter(|n| Some(n) != b.note.as_ref() || action == "reject"),
        });
    }
    p
}

/// The single model of an exported file, or why it is not one.
fn edited_model(text: &str) -> std::result::Result<ekos_ossie::SemanticModel, String> {
    let mut models = ekos_ossie::parse(text).map_err(|e| e.to_string())?;
    if models.len() != 1 {
        return Err(format!(
            "the file holds {} models; an `ekos export ossie` file holds one",
            models.len()
        ));
    }
    let m = models.remove(0);
    if ekos_ossie::ekos_data(&m.custom_extensions)
        .and_then(|x| ext_str(&x, "export_status"))
        .is_none()
    {
        return Err(
            "not an `ekos export ossie` file (no model-level EKOS extension) — an Ossie model \
             you wrote belongs in the project's sources, where `ekos recover` reads it"
                .into(),
        );
    }
    Ok(m)
}

/// `ekos import ossie` — human-only, like `ekos semantics confirm`.
pub fn ossie(config: &EkosConfig, cwd: &Path, opts: &ImportOptions) -> Result<()> {
    let text = std::fs::read_to_string(opts.file)
        .with_context(|| format!("reading {}", opts.file.display()))?;
    let mut plan_out = match edited_model(&text) {
        Ok(edited) => {
            let x = ekos_ossie::ekos_data(&edited.custom_extensions).unwrap_or_default();
            let status = ext_str(&x, "export_status")
                .and_then(|s| StatusFilter::from_name(&s))
                .unwrap_or(StatusFilter::All);
            let (database, source) = (ext_str(&x, "database"), ext_str(&x, "source"));
            let ledger = crate::commands::store::open_store_read_only(config, cwd)?;
            let known = known_entries(&super::semantics::items_in(config, cwd, &*ledger)?);
            let baseline = super::export::ossie_model(
                config,
                cwd,
                &*ledger,
                &super::export::OssieRequest {
                    status,
                    name: Some(&edited.name),
                    database: database.as_deref(),
                    source: source.as_deref(),
                },
            )?;
            drop(ledger);
            plan_ossie(&baseline, &edited, &known)
        }
        Err(e) => ImportPlan {
            errors: vec![e],
            ..Default::default()
        },
    };
    let source = format!("ossie:{}", ekos_common::ContentHash::of_str(&text).as_str());
    apply_and_report(
        config,
        cwd,
        opts,
        &mut plan_out,
        "semantics-review:import-ossie",
        source,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = r#"
name: t
annotations: {ekos_export_status: all}
classes:
  Parts:
    description: Table parts.
  PartsWithInventoryAccnoId:
    is_a: Parts
    description: Rows of `parts` where parts.inventory_accno_id IS NOT NULL.
    annotations: {ekos_id: c1, ekos_status: hypothesis}
  UserPreferenceWithoutUserId:
    is_a: Parts
    description: x
    annotations: {ekos_id: c2, ekos_status: hypothesis}
  PartsPrice:
    slot_usage:
      price:
        minimum_value: 0
        annotations: {ekos_id: k1, ekos_status: hypothesis}
enums:
  OeOeClassId:
    permissible_values:
      '4':
        description: RFQ
        annotations: {ekos_id: e4, ekos_status: hypothesis}
"#;

    fn y(s: &str) -> Value {
        serde_yaml::from_str(s).unwrap()
    }

    #[test]
    fn edits_become_decisions_matched_by_id_not_name() {
        let edited = BASE
            .replace("  PartsWithInventoryAccnoId:", "  InventoryPart:")
            .replace("description: RFQ", "description: Request for quotation")
            .replace(
                "{ekos_id: c2, ekos_status: hypothesis}",
                "{ekos_id: c2, ekos_status: rejected, ekos_review_note: anti-join}",
            )
            .replace(
                "{ekos_id: k1, ekos_status: hypothesis}",
                "{ekos_id: k1, ekos_status: confirmed}",
            );
        let p = plan(&y(BASE), &y(&edited));
        assert!(p.errors.is_empty(), "{:?}", p.errors);
        let got: Vec<(String, String)> = p
            .decisions
            .iter()
            .map(|d| (d.id.clone(), d.action.clone()))
            .collect();
        assert_eq!(
            got,
            vec![
                ("c1".into(), "edit".into()),
                ("c2".into(), "reject".into()),
                ("e4".into(), "edit".into()),
                ("k1".into(), "confirm".into()),
            ]
        );
        assert_eq!(
            decision_for(&p.decisions[0]),
            Decision::Edit {
                name: Some("InventoryPart".into()),
                description: None,
                label: None
            }
        );
        assert_eq!(
            decision_for(&p.decisions[2]),
            Decision::Edit {
                name: None,
                description: None,
                label: Some("Request for quotation".into())
            }
        );
        assert_eq!(p.decisions[1].note.as_deref(), Some("anti-join"));
    }

    #[test]
    fn an_unchanged_file_plans_nothing() {
        let p = plan(&y(BASE), &y(BASE));
        assert!(p.decisions.is_empty() && p.errors.is_empty() && p.warnings.is_empty());
    }

    #[test]
    fn invalid_edits_are_errors_or_warnings_never_guesses() {
        let edited = BASE
            .replace("'4':", "'5':")
            .replace(
                "{ekos_id: c2, ekos_status: hypothesis}",
                "{ekos_id: c2, ekos_status: rejected}",
            )
            .replace(
                "{ekos_id: k1, ekos_status: hypothesis}",
                "{ekos_id: zz, ekos_status: confirmed}",
            )
            .replace(
                "{ekos_id: c1, ekos_status: hypothesis}",
                "{ekos_id: c1, ekos_status: needs_review}",
            )
            + "  Brand:\n    description: new\n";
        let p = plan(&y(BASE), &y(&edited));
        assert_eq!(p.errors.len(), 3, "{:?}", p.errors);
        assert!(p.errors.iter().any(|e| e.contains("cannot be renamed")));
        assert!(p.errors.iter().any(|e| e.contains("needs a reason")));
        assert!(p.errors.iter().any(|e| e.contains("not a current item")));
        assert!(
            p.warnings
                .iter()
                .any(|w| w.contains("cannot be set by hand"))
        );
        assert!(p.warnings.iter().any(|w| w.contains("not imported")));
        assert!(p.decisions.is_empty());
    }

    // ── RFC 0245: Apache Ossie ─────────────────────────────────────────────────────────────

    fn ossie_base() -> ekos_ossie::SemanticModel {
        use ekos_ossie::*;
        let ext = |v: serde_json::Value| vec![CustomExtension::ekos(&v)];
        SemanticModel {
            version: SPEC_VERSION.into(),
            name: "shop".into(),
            datasets: vec![Dataset {
                name: "orders".into(),
                source: "public.orders".into(),
                fields: vec![Field {
                    name: "status".into(),
                    expression: Expression::sql("status"),
                    custom_extensions: ext(serde_json::json!({"enum_meanings": [
                        {"ekos_id": "e1", "value": "'O'", "label": "open", "status": "hypothesis"}]})),
                    ..Default::default()
                }],
                custom_extensions: ext(serde_json::json!({"ekos_id": "t1", "status": "observed",
                    "concepts": [{"ekos_id": "c1", "name": "OpenOrder", "description": "not shipped",
                                  "status": "hypothesis"}]})),
                ..Default::default()
            }],
            custom_extensions: ext(serde_json::json!({"export_status": "all"})),
            ..Default::default()
        }
    }

    /// What the ledger holds, as the baseline export states it (the tests have no ledger).
    fn known_of(b: &ekos_ossie::SemanticModel) -> BTreeMap<String, OssieEntry> {
        ossie_entries(b, &mut Vec::new())
    }

    /// Rewrite one EKOS extension's JSON in place.
    fn edit_ext(exts: &mut [ekos_ossie::CustomExtension], f: impl FnOnce(&mut serde_json::Value)) {
        let mut v: serde_json::Value = serde_json::from_str(&exts[0].data).unwrap();
        f(&mut v);
        exts[0].data = v.to_string();
    }

    #[test]
    fn an_unchanged_ossie_export_plans_nothing() {
        let b = ossie_base();
        // Through YAML and back, as a person's editor would.
        let edited = ekos_ossie::parse(&b.to_yaml()).unwrap().remove(0);
        let p = plan_ossie(&b, &edited, &known_of(&b));
        assert!(p.decisions.is_empty() && p.warnings.is_empty() && p.errors.is_empty());
    }

    #[test]
    fn ossie_edits_become_decisions_matched_by_id() {
        let b = ossie_base();
        let mut e = b.clone();
        edit_ext(&mut e.datasets[0].fields[0].custom_extensions, |v| {
            v["enum_meanings"][0]["label"] = "open order".into();
        });
        edit_ext(&mut e.datasets[0].custom_extensions, |v| {
            v["concepts"][0]["status"] = "rejected".into();
            v["concepts"][0]["note"] = "not a real concept".into();
        });
        // Renaming the dataset changes nothing: entries match by id, not by name.
        e.datasets[0].name = "orders_renamed".into();
        let p = plan_ossie(&b, &e, &known_of(&b));
        assert!(p.errors.is_empty(), "{:?}", p.errors);
        let mut got: Vec<(String, String)> = p
            .decisions
            .iter()
            .map(|d| (d.id.clone(), d.action.clone()))
            .collect();
        got.sort();
        assert_eq!(
            got,
            [("c1".into(), "reject".into()), ("e1".into(), "edit".into())]
        );
        let edit = p.decisions.iter().find(|d| d.id == "e1").unwrap();
        assert_eq!(
            edit.changes["label"],
            [Some("open".into()), Some("open order".into())]
        );
        let reject = p.decisions.iter().find(|d| d.id == "c1").unwrap();
        assert_eq!(reject.note.as_deref(), Some("not a real concept"));
        assert!(matches!(
            decision_for(edit),
            Decision::Edit { label: Some(_), .. }
        ));
    }

    #[test]
    fn ossie_observed_edits_and_bad_entries_are_never_guessed_into_decisions() {
        let b = ossie_base();
        let mut e = b.clone();
        e.datasets[0].fields[0].description = Some("order state".into());
        edit_ext(&mut e.datasets[0].custom_extensions, |v| {
            v["concepts"][0]["status"] = "hypothesis-ish".into();
        });
        edit_ext(&mut e.datasets[0].fields[0].custom_extensions, |v| {
            v["enum_meanings"][0]["ekos_id"] = "nope".into();
        });
        let p = plan_ossie(&b, &e, &known_of(&b));
        assert!(p.decisions.is_empty());
        assert!(p.warnings.iter().any(|w| w.contains("observed schema")));
        assert!(
            p.warnings
                .iter()
                .any(|w| w.contains("cannot be set by hand"))
        );
        assert!(p.errors.iter().any(|x| x.contains("nope")));
    }

    #[test]
    fn only_an_ekos_export_can_be_imported() {
        let mut m = ossie_base();
        m.custom_extensions.clear();
        assert!(
            edited_model(&m.to_yaml())
                .unwrap_err()
                .contains("not an `ekos export ossie`")
        );
        assert!(edited_model(&ossie_base().to_yaml()).is_ok());
    }
}
