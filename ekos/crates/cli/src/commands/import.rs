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
        let mut changes: BTreeMap<String, [Option<String>; 2]> = BTreeMap::new();
        match e.kind {
            "concept" => {
                if e.key != b.key {
                    changes.insert("name".into(), [Some(b.key.clone()), Some(e.key.clone())]);
                }
                if e.description != b.description {
                    changes.insert(
                        "description".into(),
                        [b.description.clone(), e.description.clone()],
                    );
                }
            }
            "enum" => {
                if e.key != b.key {
                    p.errors.push(format!(
                        "{}: a code is the stored value and cannot be renamed ({} → {}); change \
                         its description to relabel it",
                        e.path, b.key, e.key
                    ));
                    continue;
                }
                if e.description != b.description {
                    changes.insert(
                        "label".into(),
                        [b.description.clone(), e.description.clone()],
                    );
                }
            }
            _ => {}
        }
        let note_changed = e.note != b.note;
        let status_changed = e.status != b.status;
        let wanted = e.status.as_deref().unwrap_or("hypothesis");
        let action = if status_changed && wanted == "rejected" {
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
                continue;
            }
            changes.clear();
            "reject"
        } else if !changes.is_empty() {
            "edit"
        } else if status_changed && wanted == "confirmed" {
            "confirm"
        } else if status_changed {
            p.warnings.push(format!(
                "{}: `ekos_status: {wanted}` cannot be set by hand — only confirmed or rejected",
                e.path
            ));
            continue;
        } else if note_changed && b.status.as_deref() == Some("confirmed") {
            "confirm"
        } else {
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

    if !opts.dry_run && plan_out.errors.is_empty() && !plan_out.decisions.is_empty() {
        let by = opts
            .by
            .clone()
            .or_else(|| std::env::var("USER").ok())
            .filter(|b| !b.trim().is_empty())
            .ok_or_else(|| anyhow::anyhow!("who is reviewing? Pass --as <you>"))?;
        let (ledger, items) = current_items_for_write(config, cwd)?;
        let by_id: BTreeMap<String, &KirObject> =
            items.iter().map(|o| (o.id.to_string(), o)).collect();
        let at = chrono::Utc::now().to_rfc3339();
        // Validate every decision first: all or nothing.
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
                stage: "semantics-review:import-linkml".into(),
                source_artifact_id: Some(format!(
                    "linkml:{}",
                    ekos_common::ContentHash::of_str(&text).as_str()
                )),
            }));
            for o in &next {
                ledger.append_object(o)?;
            }
            plan_out.applied = next.len();
        }
    }

    if opts.json {
        println!("{}", serde_json::to_string_pretty(&plan_out)?);
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
}
