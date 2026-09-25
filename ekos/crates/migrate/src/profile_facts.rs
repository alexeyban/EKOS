//! RFC 0157 — profiles and drift, as ledger facts.
//!
//! The types here are deliberately *neutral*: plain data with no PostgreSQL in sight. The connector
//! produces them, this module persists them, and a future source (SQL Server, Oracle) writes the
//! same shapes. Keeping the fact model free of the source engine is what stops "the fact model"
//! quietly becoming "whatever PostgreSQL happened to return".
//!
//! **No row values.** A profile carries counts, fractions, bounds for non-PII non-text columns, and
//! classifications. Anything that could be a value from a row is the connector's job to withhold
//! (RFC 0157's suppression rules), and RFC 0154's ledger-scan test is the check.

use crate::Error;
use crate::kinds;
use crate::project::{project_id, unit_id, write_context};
use ekos_kir::{KirId, KirObject, KirRelationship, ObjectKind, RelationshipKind};
use ekos_ledger::KnowledgeStore;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

fn det_id(seed: &str) -> KirId {
    KirId(Uuid::new_v5(&Uuid::NAMESPACE_URL, seed.as_bytes()))
}

pub fn table_profile_id(project: &str, table: &str, tier: &str) -> KirId {
    det_id(&format!("migration-table-profile:{project}:{table}:{tier}"))
}

pub fn column_profile_id(project: &str, column: &str, tier: &str) -> KirId {
    det_id(&format!(
        "migration-column-profile:{project}:{column}:{tier}"
    ))
}

pub fn drift_id(project: &str, object: &str, kind: &str) -> KirId {
    det_id(&format!("migration-drift:{project}:{object}:{kind}"))
}

/// A table profile, source-independent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TableProfileFact {
    pub qualified_name: String,
    pub tier: String,
    pub row_count: i64,
    pub row_count_is_exact: bool,
    pub total_bytes: i64,
    pub last_analyze: Option<String>,
    pub inserts: i64,
    pub updates: i64,
    pub deletes: i64,
}

/// A column profile, source-independent.
///
/// `min`/`max` are `Option` and are expected to be `None` far more often than not: the connector
/// withholds them for every PII column and for every text column, PII or not.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ColumnProfileFact {
    pub qualified_name: String,
    pub tier: String,
    pub data_type: String,
    pub null_fraction: f64,
    pub distinct_estimate: Option<f64>,
    pub min: Option<String>,
    pub max: Option<String>,
    pub numeric_precision_used: Option<i32>,
    pub numeric_scale_used: Option<i32>,
    pub monotonic: Option<bool>,
    pub pii_class: Option<String>,
    pub pii_method: Option<String>,
    pub values_suppressed: bool,
}

/// Write a table profile and its columns, linked to the unit they describe.
pub fn write_table_profile(
    store: &dyn KnowledgeStore,
    project: &str,
    table: &TableProfileFact,
    columns: &[ColumnProfileFact],
    run_id: &str,
) -> Result<KirId, Error> {
    store.set_write_context(Some(write_context("profile", run_id)));
    let result = write_table_profile_inner(store, project, table, columns);
    store.set_write_context(None);
    result
}

fn write_table_profile_inner(
    store: &dyn KnowledgeStore,
    project: &str,
    table: &TableProfileFact,
    columns: &[ColumnProfileFact],
) -> Result<KirId, Error> {
    let mut obj = KirObject::new(
        table.qualified_name.clone(),
        ObjectKind::Custom(kinds::TABLE_PROFILE_KIND.into()),
    );
    obj.id = table_profile_id(project, &table.qualified_name, &table.tier);
    obj.properties.insert("project".into(), json!(project));
    let serde_json::Value::Object(map) = serde_json::to_value(table)? else {
        unreachable!("a struct serializes to an object");
    };
    obj.properties.extend(map);
    store.append_object(&obj)?;

    // The profile describes a unit, so it hangs off the unit rather than floating beside it.
    store.append_relationship(&KirRelationship::deterministic(
        RelationshipKind::Custom(kinds::HAS_PROFILE.into()),
        unit_id(project, &table.qualified_name),
        obj.id,
        &table.tier,
    ))?;

    for c in columns {
        let mut col = KirObject::new(
            c.qualified_name.clone(),
            ObjectKind::Custom(kinds::COLUMN_PROFILE_KIND.into()),
        );
        col.id = column_profile_id(project, &c.qualified_name, &c.tier);
        col.properties.insert("project".into(), json!(project));
        let serde_json::Value::Object(map) = serde_json::to_value(c)? else {
            unreachable!("a struct serializes to an object");
        };
        col.properties.extend(map);
        store.append_object(&col)?;
        store.append_relationship(&KirRelationship::deterministic(
            RelationshipKind::Custom(kinds::HAS_PROFILE.into()),
            obj.id,
            col.id,
            "",
        ))?;
    }
    Ok(obj.id)
}

pub fn finding_id(project: &str, rule: &str, object: &str) -> KirId {
    det_id(&format!("migration-finding:{project}:{rule}:{object}"))
}

/// One rule that fired, as a fact.
///
/// `affected_rows` is `Option` on purpose. A finding that was never measured is not a finding that
/// affects zero rows, and the report must be able to tell them apart — "not measured" is a gap, and
/// "zero" is the cheapest possible disposition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FindingFact {
    pub rule_id: String,
    pub family: String,
    pub severity: String,
    pub target: Option<String>,
    pub lossiness: Option<String>,
    pub object: String,
    pub message: String,
    pub affected_rows: Option<i64>,
    pub evidence_sql: Option<String>,
    pub blocks: bool,
}

/// Write findings. Each is a fact with the query behind it, so a reviewer can re-run the
/// measurement rather than take it on trust.
pub fn write_findings(
    store: &dyn KnowledgeStore,
    project: &str,
    findings: &[FindingFact],
    run_id: &str,
) -> Result<usize, Error> {
    store.set_write_context(Some(write_context("assess", run_id)));
    let result = (|| {
        for f in findings {
            let mut obj = KirObject::new(
                f.object.clone(),
                ObjectKind::Custom(kinds::FINDING_KIND.into()),
            );
            obj.id = finding_id(project, &f.rule_id, &f.object);
            obj.properties.insert("project".into(), json!(project));
            let serde_json::Value::Object(map) = serde_json::to_value(f)? else {
                unreachable!("a struct serializes to an object");
            };
            obj.properties.extend(map);
            store.append_object(&obj)?;
            store.append_relationship(&KirRelationship::deterministic(
                RelationshipKind::References,
                project_id(project),
                obj.id,
                &f.rule_id,
            ))?;
        }
        Ok::<usize, Error>(findings.len())
    })();
    store.set_write_context(None);
    result
}

/// Write drift findings. Each is a fact so a reviewer can disposition it, not a log line.
pub fn write_drift(
    store: &dyn KnowledgeStore,
    project: &str,
    drifts: &[crate::drift::Drift],
    run_id: &str,
) -> Result<usize, Error> {
    store.set_write_context(Some(write_context("discover", run_id)));
    let result = (|| {
        for d in drifts {
            let mut obj = KirObject::new(
                d.object.clone(),
                ObjectKind::Custom(kinds::DRIFT_KIND.into()),
            );
            obj.id = drift_id(project, &d.object, d.detail.as_str());
            obj.properties.insert("project".into(), json!(project));
            obj.properties
                .insert("drift_kind".into(), json!(d.detail.as_str()));
            obj.properties.insert("detail".into(), json!(d.message));
            // Structural drift changes what a migration would produce, not only what it is called.
            // RFC 0158 blocks on these; the rest inform.
            obj.properties
                .insert("structural".into(), json!(d.detail.is_structural()));
            store.append_object(&obj)?;
            store.append_relationship(&KirRelationship::deterministic(
                RelationshipKind::References,
                project_id(project),
                obj.id,
                d.detail.as_str(),
            ))?;
        }
        Ok::<usize, Error>(drifts.len())
    })();
    store.set_write_context(None);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_ids_are_deterministic_and_tier_scoped() {
        assert_eq!(
            table_profile_id("p", "s.t", "p0"),
            table_profile_id("p", "s.t", "p0")
        );
        assert_ne!(
            table_profile_id("p", "s.t", "p0"),
            table_profile_id("p", "s.t", "p1"),
            "a P1 profile must not overwrite the P0 one — they answer different questions"
        );
        assert_ne!(
            column_profile_id("p", "s.t.c", "p0"),
            column_profile_id("q", "s.t.c", "p0")
        );
    }
}
