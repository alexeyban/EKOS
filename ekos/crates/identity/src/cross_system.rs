//! Cross-system identity resolution (RFC 0029) — a heuristic scorer for
//! candidate matches between objects observed under different names in
//! different systems (e.g. Informix `cust_mstr`, Postgres `customers`,
//! Databricks `gold.dim_customer` — all "the customer table").
//!
//! Deliberately **separate** from [`crate::DefaultResolver`] (RFC 0007):
//! that resolver merges same-file/same-compile duplicates it is confident
//! enough about to fold silently before the CKM exists, and already
//! excludes `Custom("TransformNode")` objects from its own blocking
//! entirely (RFC 0027/0028's fix for a real over-merge bug). This module
//! produces **candidates only** — callers must persist them as
//! `unconfirmed` relationships and never auto-apply them; see
//! `crates/cli/src/commands/identity.rs` and the `ekos_identity_review` MCP
//! tool.

use std::collections::HashMap;

use ekos_kir::{KirId, KirObject, ObjectKind};
use serde::{Deserialize, Serialize};

use crate::similarity::{self, jaccard};

/// Below this, a pair is not worth surfacing at all — not a "confident
/// match" cutoff (that's a human/agent judgment call via
/// `ekos_identity_review`), just a floor against obvious noise.
pub const MIN_CANDIDATE_CONFIDENCE: f32 = 0.3;

/// Per-signal breakdown behind one candidate's `confidence` — carried
/// through to the persisted `Relationship`'s `properties` so a reviewer
/// (human or agent) can see *why* a match was proposed, not just a number.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossSystemSignals {
    pub column_overlap: Option<f32>,
    pub name_pattern: f32,
    pub type_compat: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossSystemCandidate {
    pub a: KirId,
    pub b: KirId,
    pub confidence: f32,
    pub signals: CrossSystemSignals,
}

/// Object kinds this comparison applies to: SQL-DDL `Table`s and
/// Transformation IR `Source`/`Sink` nodes (RFC 0027) — the only KIR shapes
/// carrying an `object_name`/`columns` pair meaningful here. Returns the
/// matchable name (not `KirObject::name`, which for a `TransformNode` is
/// `"{source_path}:{index}"`, not the underlying table identifier).
fn matchable_name(obj: &KirObject) -> Option<String> {
    match &obj.kind {
        ObjectKind::Table => Some(obj.name.clone()),
        ObjectKind::Custom(k) if k == "TransformNode" => {
            let node_type = obj.properties.get("node_type")?.as_str()?;
            if node_type != "Source" && node_type != "Sink" {
                return None;
            }
            obj.properties
                .get("object_name")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        }
        _ => None,
    }
}

/// v1 heuristic affix list — organization-specific ETL naming conventions
/// vary; extending this from real usage is expected tuning, not a design
/// flaw. Applied as whole tokens after `similarity::normalize`, not
/// substring matching, so e.g. "raw" doesn't strip inside "drawer".
const CROSS_SYSTEM_AFFIXES: &[&str] = &["mstr", "stg", "raw", "dim", "fact", "tbl"];

/// Normalizes a table-like identifier for cross-system comparison: strips a
/// schema/catalog prefix (text before the last `.`, e.g. `gold.` or `dbo.`),
/// then reuses [`similarity::normalize`] (lowercase, separator-to-space,
/// RFC 0007's existing suffix stripping), then strips
/// [`CROSS_SYSTEM_AFFIXES`] as whole tokens. `gold.dim_customer` →
/// `"customer"`, `cust_mstr` → `"cust"`, `customers` → `"customers"`.
fn normalize_cross_system(name: &str) -> String {
    let base = name.rsplit('.').next().unwrap_or(name);
    let normalized = similarity::normalize(base);
    normalized
        .split_whitespace()
        .filter(|tok| !CROSS_SYSTEM_AFFIXES.contains(tok))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Bucket a raw SQL type string into a coarse type family so `int4` and
/// `bigint` (both integers) compare compatible without an exhaustive
/// per-dialect type map.
fn type_family(data_type: &str) -> String {
    let t = data_type.to_lowercase();
    if t.contains("int") {
        "integer".to_string()
    } else if t.contains("char") || t.contains("text") || t.contains("string") {
        "string".to_string()
    } else if t.contains("date") || t.contains("time") {
        "datetime".to_string()
    } else if t.contains("numeric")
        || t.contains("decimal")
        || t.contains("float")
        || t.contains("double")
    {
        "numeric".to_string()
    } else if t.contains("bool") {
        "boolean".to_string()
    } else {
        t
    }
}

/// Lowercased column-name → type-family map, only `Some` when the object's
/// columns carry `data_type` (only SQL-DDL `Table` objects do — `TransformNode`
/// columns are name-only, per RFC 0027's MVP scope).
fn column_types(obj: &KirObject) -> Option<HashMap<String, String>> {
    let cols = obj.properties.get("columns")?.as_array()?;
    let map: HashMap<String, String> = cols
        .iter()
        .filter_map(|c| {
            let name = c.get("name")?.as_str()?.to_lowercase();
            let dtype = c.get("data_type")?.as_str()?;
            Some((name, type_family(dtype)))
        })
        .collect();
    if map.is_empty() { None } else { Some(map) }
}

fn type_compat_score(a: &KirObject, b: &KirObject) -> Option<f32> {
    let types_a = column_types(a)?;
    let types_b = column_types(b)?;
    let shared: Vec<&String> = types_a
        .keys()
        .filter(|k| types_b.contains_key(*k))
        .collect();
    if shared.is_empty() {
        return None;
    }
    let matches = shared
        .iter()
        .filter(|k| types_a.get(**k) == types_b.get(**k))
        .count();
    Some(matches as f32 / shared.len() as f32)
}

fn column_overlap_score(a: &KirObject, b: &KirObject) -> Option<f32> {
    let cols_a = similarity::column_names(a)?;
    let cols_b = similarity::column_names(b)?;
    Some(jaccard(&cols_a, &cols_b))
}

/// Weighted combination of whichever signals are available for this pair —
/// signals that don't apply (e.g. no type data on either side) are excluded
/// from the average entirely, not scored as 0, so their absence never
/// silently drags a real match below the floor.
fn combine_signals(
    column_overlap: Option<f32>,
    name_pattern: f32,
    type_compat: Option<f32>,
) -> f32 {
    let mut weighted_sum = 0.4 * name_pattern;
    let mut weight_total = 0.4;
    if let Some(c) = column_overlap {
        weighted_sum += 0.4 * c;
        weight_total += 0.4;
    }
    if let Some(t) = type_compat {
        weighted_sum += 0.2 * t;
        weight_total += 0.2;
    }
    weighted_sum / weight_total
}

/// Scores every pair of `Table`/`TransformNode` `Source`/`Sink` objects,
/// returning candidates at or above [`MIN_CANDIDATE_CONFIDENCE`]. O(n²) over
/// the *filtered* candidate set (table-like objects only, not every KIR
/// object) — acceptable for v1 at realistic table/pipeline-endpoint counts;
/// flagged as a follow-up if a real workspace's count makes it not.
pub fn find_cross_system_candidates(objects: &[KirObject]) -> Vec<CrossSystemCandidate> {
    let candidates: Vec<(&KirObject, String, String)> = objects
        .iter()
        .filter_map(|obj| {
            matchable_name(obj).map(|name| (obj, name.clone(), normalize_cross_system(&name)))
        })
        .collect();

    let mut results = Vec::new();
    for i in 0..candidates.len() {
        for j in (i + 1)..candidates.len() {
            let (obj_a, name_a, norm_a) = &candidates[i];
            let (obj_b, name_b, norm_b) = &candidates[j];
            if obj_a.id == obj_b.id {
                continue;
            }
            // Same-name, same-*kind* pairs are `DefaultResolver`'s job (RFC
            // 0007) — skip them here to avoid double-proposing. But
            // `DefaultResolver` only ever compares objects of identical
            // `ObjectKind` (see its `structural_score`), so an exact-name
            // match *across* kinds — a SQL `Table` and a Pentaho
            // `Source`/`Sink` node sharing the same `object_name` — is never
            // handled by either resolver unless it's scored here too. This
            // is in fact the single most confident case this module can see
            // (name_pattern resolves to 1.0), so it must not be skipped.
            if obj_a.kind == obj_b.kind && name_a.eq_ignore_ascii_case(name_b) {
                continue;
            }

            let name_pattern = similarity::jaro_winkler(norm_a, norm_b);
            let column_overlap = column_overlap_score(obj_a, obj_b);
            let type_compat = type_compat_score(obj_a, obj_b);
            let confidence = combine_signals(column_overlap, name_pattern, type_compat);

            if confidence >= MIN_CANDIDATE_CONFIDENCE {
                results.push(CrossSystemCandidate {
                    a: obj_a.id,
                    b: obj_b.id,
                    confidence,
                    signals: CrossSystemSignals {
                        column_overlap,
                        name_pattern,
                        type_compat,
                    },
                });
            }
        }
    }
    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_kir::ObjectKind;
    use serde_json::json;

    fn table(name: &str, columns: &[(&str, &str)]) -> KirObject {
        let cols: Vec<serde_json::Value> = columns
            .iter()
            .map(|(n, t)| json!({"name": n, "data_type": t}))
            .collect();
        KirObject::new(name, ObjectKind::Table).with_property("columns", json!(cols))
    }

    fn transform_source(
        source_path: &str,
        index: usize,
        object_name: &str,
        columns: &[&str],
    ) -> KirObject {
        let mut obj = KirObject::new(
            format!("{source_path}:{index}"),
            ObjectKind::Custom("TransformNode".to_string()),
        );
        obj.properties.insert("node_type".into(), json!("Source"));
        obj.properties
            .insert("object_name".into(), json!(object_name));
        obj.properties.insert("columns".into(), json!(columns));
        obj
    }

    fn transform_sink(
        source_path: &str,
        index: usize,
        object_name: &str,
        columns: &[&str],
    ) -> KirObject {
        let mut obj = transform_source(source_path, index, object_name, columns);
        obj.properties.insert("node_type".into(), json!("Sink"));
        obj
    }

    #[test]
    fn normalize_strips_schema_prefix_and_etl_affixes() {
        assert_eq!(normalize_cross_system("gold.dim_customer"), "customer");
        assert_eq!(normalize_cross_system("cust_mstr"), "cust");
        assert_eq!(normalize_cross_system("customers"), "customers");
        assert_eq!(normalize_cross_system("dbo.cust_mstr"), "cust");
    }

    #[test]
    fn column_overlap_scores_shared_column_names() {
        let a = table(
            "customers",
            &[("id", "int"), ("name", "varchar"), ("status", "varchar")],
        );
        let b = table("gold.dim_customer", &[("id", "bigint"), ("name", "string")]);
        let score = column_overlap_score(&a, &b).unwrap();
        assert!(score > 0.5, "expected high overlap, got {score}");
    }

    #[test]
    fn type_compat_none_when_either_side_untyped() {
        let a = table("customers", &[("id", "int")]);
        let b = transform_source("load.ktr", 0, "dbo.cust_mstr", &["id"]);
        assert!(type_compat_score(&a, &b).is_none());
    }

    #[test]
    fn type_compat_scores_matching_families() {
        let a = table("customers", &[("id", "int"), ("name", "varchar")]);
        let b = table("cust_mstr", &[("id", "bigint"), ("name", "text")]);
        let score = type_compat_score(&a, &b).unwrap();
        assert_eq!(
            score, 1.0,
            "int~bigint and varchar~text are the same family"
        );
    }

    /// The scenario this RFC exists for: the same entity under three names
    /// across three systems must produce candidate pairs above the floor.
    #[test]
    fn three_system_customer_table_scenario_produces_candidates() {
        let informix = table(
            "cust_mstr",
            &[("id", "int"), ("name", "char"), ("status", "char")],
        );
        let postgres = table(
            "customers",
            &[("id", "int"), ("name", "varchar"), ("status", "varchar")],
        );
        let databricks = table("gold.dim_customer", &[("id", "bigint"), ("name", "string")]);

        let objects = vec![informix, postgres, databricks];
        let candidates = find_cross_system_candidates(&objects);

        assert!(
            candidates.len() >= 2,
            "expected at least 2 of the 3 pairs to surface, got {candidates:?}"
        );
        assert!(
            candidates
                .iter()
                .all(|c| c.confidence >= MIN_CANDIDATE_CONFIDENCE)
        );
    }

    #[test]
    fn transform_node_source_and_table_can_match() {
        let table_obj = table("customers", &[("id", "int"), ("name", "varchar")]);
        let source_obj = transform_source("load.ktr", 0, "dbo.cust_mstr", &["id", "name"]);
        let objects = vec![table_obj, source_obj];
        let candidates = find_cross_system_candidates(&objects);
        assert_eq!(candidates.len(), 1);
    }

    #[test]
    fn unrelated_tables_do_not_produce_a_candidate() {
        let a = table("orders", &[("order_id", "int"), ("total", "numeric")]);
        let b = table(
            "employee_territories",
            &[("employee_id", "int"), ("territory_id", "int")],
        );
        let objects = vec![a, b];
        let candidates = find_cross_system_candidates(&objects);
        assert!(
            candidates.is_empty(),
            "genuinely unrelated tables must not surface a candidate, got {candidates:?}"
        );
    }

    #[test]
    fn non_table_like_objects_are_ignored() {
        let mut section =
            KirObject::new("doc.pdf: page 1", ObjectKind::Custom("Section".to_string()));
        section.properties.insert("excerpt".into(), json!("hello"));
        let objects = vec![section];
        assert!(find_cross_system_candidates(&objects).is_empty());
    }

    #[test]
    fn identical_names_are_not_proposed_as_candidates() {
        // Same-name same-system dedup is DefaultResolver's job (RFC 0007),
        // not cross-system's.
        let a = table("customers", &[("id", "int")]);
        let b = table("customers", &[("id", "int")]);
        let objects = vec![a, b];
        assert!(find_cross_system_candidates(&objects).is_empty());
    }

    /// The bug this module previously had: `DefaultResolver` (RFC 0007) only
    /// ever compares objects of identical `ObjectKind`, so an exact-name
    /// match *across* kinds — a SQL `Table` and the Pentaho `Sink` node that
    /// writes to it, sharing the literal same `object_name` — was skipped
    /// here too (on the assumption same-name dedup was "someone else's
    /// job") and so never linked by *either* resolver. This is the highest-
    /// confidence case cross-system identity resolution can see and must
    /// not be dropped.
    #[test]
    fn exact_name_match_across_kinds_is_proposed_at_max_confidence() {
        let table_obj = table(
            "fact_patient_coded_value",
            &[("patient_id", "int"), ("coded_value_id", "int")],
        );
        let sink_obj = transform_sink(
            "load-fact-coded-values.ktr",
            1,
            "fact_patient_coded_value",
            &["patient_id", "coded_value_id"],
        );
        let objects = vec![table_obj, sink_obj];
        let candidates = find_cross_system_candidates(&objects);
        assert_eq!(
            candidates.len(),
            1,
            "expected exactly one cross-kind candidate, got {candidates:?}"
        );
        assert!(
            candidates[0].confidence >= 0.99,
            "expected near-max confidence for an identical name, got {}",
            candidates[0].confidence
        );
    }

    /// Same-kind exact matches must still be left to `DefaultResolver` —
    /// this module should not start double-proposing those.
    #[test]
    fn exact_name_match_same_kind_is_still_skipped() {
        let a = table("fact_patient_coded_value", &[("patient_id", "int")]);
        let b = table("fact_patient_coded_value", &[("patient_id", "int")]);
        let objects = vec![a, b];
        assert!(find_cross_system_candidates(&objects).is_empty());
    }
}
