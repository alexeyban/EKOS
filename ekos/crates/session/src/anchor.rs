//! Anchor resolution and staleness fingerprints (RFC 0151 Phases 2 and 4).
//!
//! Resolution is exact-match only: an ambiguous hint is recorded as ambiguous, never guessed, and
//! nothing here mutates an existing object or identity.

use ekos_kir::custom_kinds::{SESSION_CLAIM_KIND, SESSION_KIND};
use ekos_kir::{KirId, KirObject, ObjectKind};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Resolution {
    Resolved { object_id: String },
    Ambiguous { candidates: usize },
    Unresolved,
}

pub struct AnchorIndex {
    by_name: HashMap<String, Vec<KirObject>>,
}

impl AnchorIndex {
    /// Indexes every non-session object by exact name (and by `./`-stripped name).
    pub fn from_objects(objects: impl IntoIterator<Item = KirObject>) -> Self {
        let mut by_name: HashMap<String, Vec<KirObject>> = HashMap::new();
        for o in objects {
            if matches!(&o.kind, ObjectKind::Custom(c) if c == SESSION_KIND || c == SESSION_CLAIM_KIND)
            {
                continue;
            }
            by_name.entry(o.name.clone()).or_default().push(o);
        }
        Self { by_name }
    }

    pub fn resolve(&self, hint: &str) -> (Resolution, Option<&KirObject>) {
        let key = hint.strip_prefix("./").unwrap_or(hint);
        match self.by_name.get(key).map(Vec::as_slice) {
            Some([one]) => (
                Resolution::Resolved {
                    object_id: one.id.to_string(),
                },
                Some(one),
            ),
            Some(many) if many.len() > 1 => (
                Resolution::Ambiguous {
                    candidates: many.len(),
                },
                None,
            ),
            _ => (Resolution::Unresolved, None),
        }
    }
}

/// Keys that churn without the object meaning anything different — never part of a fingerprint.
fn is_volatile(key: &str) -> bool {
    key.starts_with("ai_")
        || key.starts_with("llm_")
        || matches!(
            key,
            "description"
                | "doc_type"
                | "rfc_number"
                | "rfc_title"
                | "rfc_status"
                | "line_start"
                | "line_end"
                | "size_bytes"
                | "source_artifact_ids"
                | "created_at"
                | "reviewed_at"
                | "review_status"
                | "status"
        )
}

/// The narrow slice of an object's state that an anchored note is about. Tables pin their
/// columns; symbols/modules their signature-shaped keys; everything else falls back to all
/// non-volatile properties.
pub fn anchor_projection(obj: &KirObject) -> BTreeMap<String, serde_json::Value> {
    let pick = |keys: &[&str]| -> BTreeMap<String, serde_json::Value> {
        keys.iter()
            .filter_map(|k| {
                obj.properties
                    .get(*k)
                    .map(|v| ((*k).to_string(), v.clone()))
            })
            .collect()
    };
    let narrowed = match &obj.kind {
        ObjectKind::Table => pick(&["columns"]),
        ObjectKind::Custom(k) if k.ends_with("Symbol") => pick(&[
            "signature",
            "params",
            "return_type",
            "visibility",
            "symbol_kind",
        ]),
        // A section is its text and heading. Line ranges shift when unrelated text above is
        // edited, and `doc_type`/`rfc_*` are derived metadata — measured on a real ledger they
        // were most of the fingerprint noise (RFC 0151 Phase 4).
        ObjectKind::Custom(k) if k == "Section" => pick(&["excerpt", "heading"]),
        ObjectKind::Custom(k) if matches!(k.as_str(), "Document" | "Page") => {
            pick(&["content_hash", "checksum", "text", "excerpt"])
        }
        // `artifact_id` is content-addressed; `size_bytes` is derived from it.
        ObjectKind::File => pick(&["artifact_id"]),
        ObjectKind::Custom(k) if k == "File" => pick(&["artifact_id"]),
        ObjectKind::Custom(k) if k == "TransformNode" => pick(&["sql", "steps", "expression"]),
        _ => BTreeMap::new(),
    };
    if !narrowed.is_empty() {
        return narrowed;
    }
    obj.properties
        .iter()
        .filter(|(k, _)| !is_volatile(k))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

/// SHA-256 over the canonical (key-sorted) JSON of the projection.
pub fn anchor_fingerprint(obj: &KirObject) -> String {
    let canonical = serde_json::to_string(&anchor_projection(obj)).unwrap_or_default();
    ekos_common::ContentHash::of_str(&canonical).0
}

/// Short human summary of which projected keys differ between two versions.
pub fn change_summary(before: &KirObject, after: &KirObject) -> String {
    let (a, b) = (anchor_projection(before), anchor_projection(after));
    let mut keys: Vec<&String> = a.keys().chain(b.keys()).collect();
    keys.sort();
    keys.dedup();
    let changed: Vec<&str> = keys
        .into_iter()
        .filter(|k| a.get(*k) != b.get(*k))
        .map(String::as_str)
        .collect();
    if changed.is_empty() {
        "changed".into()
    } else {
        format!("changed: {}", changed.join(", "))
    }
}

/// Deterministic id for an object id round-trip helper.
pub fn parse_id(s: &str) -> Option<KirId> {
    s.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn table(name: &str, cols: serde_json::Value) -> KirObject {
        KirObject::new(name, ObjectKind::Table).with_property("columns", cols)
    }

    #[test]
    fn exact_match_resolves_and_ambiguity_is_never_guessed() {
        let idx = AnchorIndex::from_objects([
            table("orders", json!([])),
            table("dup", json!([])),
            table("dup", json!([])),
        ]);
        assert!(matches!(
            idx.resolve("orders").0,
            Resolution::Resolved { .. }
        ));
        assert_eq!(
            idx.resolve("dup").0,
            Resolution::Ambiguous { candidates: 2 }
        );
        assert_eq!(idx.resolve("Orders").0, Resolution::Unresolved);
        assert_eq!(idx.resolve("nope").0, Resolution::Unresolved);
    }

    #[test]
    fn session_objects_are_never_anchor_targets() {
        let idx = AnchorIndex::from_objects([KirObject::new(
            "x",
            ObjectKind::Custom(SESSION_CLAIM_KIND.into()),
        )]);
        assert_eq!(idx.resolve("x").0, Resolution::Unresolved);
    }

    #[test]
    fn unrelated_edits_do_not_change_a_table_fingerprint_but_column_edits_do() {
        let a = table("orders", json!([{"name":"id"}]));
        let mut b = a.clone();
        b.properties.insert("ai_overview".into(), json!("prose"));
        b.properties.insert("description".into(), json!("d"));
        assert_eq!(anchor_fingerprint(&a), anchor_fingerprint(&b));
        let mut c = a.clone();
        c.properties
            .insert("columns".into(), json!([{"name":"id"},{"name":"total"}]));
        assert_ne!(anchor_fingerprint(&a), anchor_fingerprint(&c));
        assert_eq!(change_summary(&a, &c), "changed: columns");
    }

    #[test]
    fn generic_kinds_ignore_volatile_keys() {
        let mut a = KirObject::new("f", ObjectKind::Custom("Widget".into()));
        a.properties.insert("size".into(), json!(1));
        let mut b = a.clone();
        b.properties.insert("llm_note".into(), json!("x"));
        assert_eq!(anchor_fingerprint(&a), anchor_fingerprint(&b));
        b.properties.insert("size".into(), json!(2));
        assert_ne!(anchor_fingerprint(&a), anchor_fingerprint(&b));
    }
}
