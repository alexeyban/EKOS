//! RFC 0211 §3.4 — profiles, in three tiers, and the log-state facts CDC needs.
//!
//! | Tier | Source | Cost |
//! |---|---|---|
//! | P0 | optimizer statistics (`NUM_ROWS`, `NUM_NULLS`, `NUM_DISTINCT`, `AVG_COL_LEN`), read with the catalog | none |
//! | P1 | `SELECT COUNT(*)` per table | one scan per table |
//! | P2 | one aggregate scan per table: exact null counts, numeric and date bounds, longest text | one scan per table |
//!
//! No tier lets a row value of a text column leave the database: a text column contributes a count
//! and a length, never a minimum or a maximum, because those *are* values. Bounds are taken only
//! for numbers and dates, and not even those for a column whose name classifies it as personal data
//! ([`ekos_common::pii`]) — the same rule as `pg-live`. Every scan runs inside `SET TRANSACTION
//! READ ONLY` under the session's call timeout.

use crate::OraError;
use crate::catalog::{CatalogSnapshot, ObjectKind, Rows};
use crate::session::guard_identifier;
use ekos_common::redaction::RedactionConfig;
use ekos_observation_sdk::profile::{self, Flavor, TableOutcome};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub use ekos_observation_sdk::profile::{ProfileStats, ProfileTier};

/// A table's row count and how it was obtained.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableProfile {
    pub tier: ProfileTier,
    pub row_count: i64,
    /// `false` for P0: `NUM_ROWS` is as old as the last statistics run.
    pub row_count_exact: bool,
}

impl TableProfile {
    pub fn from_statistics(num_rows: i64) -> Self {
        Self {
            tier: ProfileTier::P0,
            row_count: num_rows,
            row_count_exact: false,
        }
    }

    pub fn into_json(self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }
}

/// Where scans run: the live session, or recorded rows in tests. No binds: every identifier in a
/// scan has passed [`guard_identifier`] and is quoted.
pub trait ScanSource {
    fn scan(&self, sql: &str) -> Result<Rows, OraError>;
    fn begin_read_only(&self) -> Result<(), OraError>;
    fn end_transaction(&self) -> Result<(), OraError>;
}

fn count_of(rows: &Rows) -> Option<i64> {
    rows.first()?.first()?.as_deref()?.trim().parse().ok()
}

/// The session is gone after an ODPI-C call timeout (`DPI-1067`) or a lost connection; every later
/// call would fail the same way.
fn session_lost(m: &str) -> bool {
    ["DPI-1067", "DPI-1080", "DPI-1010", "ORA-03113", "ORA-03135"]
        .iter()
        .any(|c| m.contains(c))
}

/// Profile every table of `snapshot` at `tier` (P1 or P2; P0 is read with the catalog), writing
/// the results into each table's `detail.profile` and its columns' `profile`. The queries are
/// planned by `ekos_observation_sdk::profile` (the rules every engine shares) and run here inside
/// one read-only transaction.
pub fn scan(
    source: &dyn ScanSource,
    snapshot: &mut CatalogSnapshot,
    tier: ProfileTier,
    redaction: &RedactionConfig,
) -> Result<ProfileStats, OraError> {
    let mut stats = ProfileStats::new(tier);
    if tier == ProfileTier::P0 {
        return Ok(stats);
    }
    let owner = guard_identifier(&snapshot.owner)?.to_string();
    source.begin_read_only()?;
    let mut lost = false;
    for t in snapshot
        .objects
        .iter_mut()
        .filter(|o| o.kind == ObjectKind::Table)
    {
        if lost {
            profile::record_skipped(&mut t.detail);
            stats.record(&TableOutcome::Skipped);
            continue;
        }
        let mut run = |sql: &str| -> Result<Vec<Option<String>>, String> {
            let rows = source.scan(sql).map_err(|e| e.to_string())?;
            rows.into_iter()
                .next()
                .ok_or_else(|| "an aggregate query returned no row".to_string())
        };
        let outcome = profile::profile_table(
            Flavor::Oracle,
            &owner,
            &t.name,
            &mut t.detail,
            tier,
            redaction,
            &mut run,
        );
        match outcome {
            Ok(o) => stats.record(&o),
            Err(e) => {
                lost = session_lost(&e);
                stats.record(&TableOutcome::Failed);
            }
        }
    }
    if !lost {
        source.end_transaction()?;
    }
    Ok(stats)
}

/// Archive-log and supplemental-logging state (§3.4), for the CDC part of the plan. A refusal is
/// recorded as `unknown` with the error, never as "off".
pub fn log_state(source: &dyn ScanSource, owner: &str) -> Value {
    let database = source.scan(
        "SELECT log_mode, supplemental_log_data_min, supplemental_log_data_pk, \
         supplemental_log_data_all, force_logging FROM v$database",
    );
    let mut out = match database {
        Ok(rows) => {
            let r = rows.into_iter().next().unwrap_or_default();
            let at = |i: usize| r.get(i).cloned().flatten().map_or(Value::Null, Value::from);
            json!({
                "log_mode": at(0),
                "supplemental_log_data_min": at(1),
                "supplemental_log_data_pk": at(2),
                "supplemental_log_data_all": at(3),
                "force_logging": at(4),
            })
        }
        Err(e) => json!({ "unknown": e.to_string() }),
    };
    match guard_identifier(owner) {
        Ok(o) => {
            // The owner is a checked identifier, so it can be a literal here: scans take no binds.
            match source.scan(&format!(
                "SELECT COUNT(*) FROM dba_log_groups WHERE owner = '{o}'"
            )) {
                Ok(rows) => {
                    out["table_log_groups"] = count_of(&rows).map_or(Value::Null, Value::from)
                }
                Err(e) => out["table_log_groups_unknown"] = e.to_string().into(),
            }
        }
        Err(e) => out["table_log_groups_unknown"] = e.to_string().into(),
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{CatalogObject, ViewFamily};
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    /// Answers each scan by the first table name it mentions; records every SQL text.
    #[derive(Default)]
    struct Recorded {
        answers: BTreeMap<String, Result<Rows, String>>,
        seen: RefCell<Vec<String>>,
    }

    impl ScanSource for Recorded {
        fn scan(&self, sql: &str) -> Result<Rows, OraError> {
            self.seen.borrow_mut().push(sql.to_string());
            let hit = self.answers.iter().find(|(k, _)| {
                // Quoted names match exactly; a lowercase key (a system view) matches as text.
                sql.contains(&format!("\"{k}\""))
                    || (k.chars().any(char::is_lowercase) && sql.contains(k.as_str()))
            });
            match hit {
                Some((_, Ok(r))) => Ok(r.clone()),
                Some((_, Err(e))) => Err(OraError::Query {
                    sql: sql.into(),
                    message: e.clone(),
                }),
                None => Ok(vec![]),
            }
        }
        fn begin_read_only(&self) -> Result<(), OraError> {
            self.seen.borrow_mut().push("BEGIN READ ONLY".into());
            Ok(())
        }
        fn end_transaction(&self) -> Result<(), OraError> {
            self.seen.borrow_mut().push("END".into());
            Ok(())
        }
    }

    fn row(v: &[&str]) -> Vec<Option<String>> {
        v.iter()
            .map(|s| (!s.is_empty()).then(|| s.to_string()))
            .collect()
    }

    fn table(name: &str, columns: Value) -> CatalogObject {
        CatalogObject {
            kind: ObjectKind::Table,
            owner: "ADEMPIERE".into(),
            name: name.into(),
            status: Some("VALID".into()),
            detail: json!({ "columns": columns }),
        }
    }

    fn snapshot(objects: Vec<CatalogObject>) -> CatalogSnapshot {
        CatalogSnapshot {
            owner: "ADEMPIERE".into(),
            family: ViewFamily::Dba,
            objects,
            census: BTreeMap::new(),
            columns: 0,
            folded_not_null: 0,
            other_constraints: BTreeMap::new(),
        }
    }

    fn invoice() -> CatalogObject {
        table(
            "C_INVOICE",
            json!([
                { "name": "C_INVOICE_ID", "data_type": "NUMBER(10,0)" },
                { "name": "DOCUMENTNO", "data_type": "VARCHAR2(30 CHAR)" },
                { "name": "DATEINVOICED", "data_type": "DATE" },
                { "name": "EMAIL", "data_type": "VARCHAR2(60)" },
                { "name": "PHONE_NUMBER", "data_type": "NUMBER" },
                { "name": "NOTES", "data_type": "LONG" },
                { "name": "ATTACHMENT", "data_type": "BLOB" },
            ]),
        )
    }

    // The "never a text value, never a personal bound" rule is tested on the SQL itself, for
    // every engine, in `ekos_observation_sdk::profile`.

    #[test]
    fn p2_records_exact_nulls_bounds_and_lengths_inside_a_read_only_transaction() {
        let mut src = Recorded::default();
        // COUNT(*), then per measure in order: ID count/min/max, DOCUMENTNO count/len,
        // DATEINVOICED count/min/max, EMAIL count/len, PHONE count, ATTACHMENT count.
        src.answers.insert(
            "C_INVOICE".into(),
            Ok(vec![row(&[
                "10",
                "10",
                "1000000",
                "1000009",
                "10",
                "12",
                "8",
                "2003-01-01T00:00:00",
                "2024-12-01T00:00:00",
                "3",
                "40",
                "2",
                "0",
            ])]),
        );
        let mut snap = snapshot(vec![invoice()]);
        let stats = scan(
            &src,
            &mut snap,
            ProfileTier::P2,
            &RedactionConfig::default(),
        )
        .unwrap();
        assert_eq!(stats.tables_profiled, 1);
        assert_eq!(stats.values_suppressed, 1);
        let t = &snap.objects[0].detail;
        assert_eq!(t["profile"]["row_count"], 10);
        assert_eq!(t["profile"]["row_count_exact"], true);
        let col = |i: usize| &t["columns"][i]["profile"];
        assert_eq!(col(0)["nulls"], 0);
        assert_eq!(col(0)["min"], "1000000");
        assert_eq!(col(1)["max_length"], 12);
        assert!(col(1).get("min").is_none());
        assert_eq!(col(2)["nulls"], 2);
        assert_eq!(col(2)["nulls_exact"], true);
        assert_eq!(col(3)["nulls"], 7);
        assert!(col(3).get("max").is_none());
        assert_eq!(col(4)["values_suppressed"], true);
        assert!(col(4).get("min").is_none());
        assert!(
            t["columns"][5].get("profile").is_none(),
            "LONG not measured"
        );
        let seen = src.seen.borrow();
        assert_eq!(seen.first().map(String::as_str), Some("BEGIN READ ONLY"));
        assert_eq!(seen.last().map(String::as_str), Some("END"));
    }

    #[test]
    fn a_failed_table_is_recorded_and_a_lost_session_skips_the_rest() {
        let mut src = Recorded::default();
        src.answers.insert(
            "A".into(),
            Err("ORA-00942: table or view does not exist".into()),
        );
        src.answers.insert(
            "B".into(),
            Err("DPI-1067: call timeout of 30000 ms exceeded".into()),
        );
        let mut snap = snapshot(vec![
            table("A", json!([])),
            table("B", json!([])),
            table("C", json!([])),
        ]);
        let stats = scan(
            &src,
            &mut snap,
            ProfileTier::P1,
            &RedactionConfig::default(),
        )
        .unwrap();
        assert_eq!(
            (
                stats.tables_failed,
                stats.tables_skipped,
                stats.tables_profiled
            ),
            (2, 1, 0)
        );
        assert!(
            snap.objects[0].detail["profile"]["error"]
                .as_str()
                .unwrap()
                .contains("ORA-00942")
        );
        assert!(snap.objects[2].detail["profile"]["skipped"].is_string());
    }

    #[test]
    fn p1_counts_rows_and_p0_scans_nothing() {
        let mut src = Recorded::default();
        src.answers
            .insert("C_INVOICE".into(), Ok(vec![row(&["42"])]));
        let mut snap = snapshot(vec![invoice()]);
        scan(
            &src,
            &mut snap,
            ProfileTier::P0,
            &RedactionConfig::default(),
        )
        .unwrap();
        assert!(src.seen.borrow().is_empty());
        scan(
            &src,
            &mut snap,
            ProfileTier::P1,
            &RedactionConfig::default(),
        )
        .unwrap();
        assert_eq!(snap.objects[0].detail["profile"]["row_count"], 42);
        assert!(
            src.seen
                .borrow()
                .iter()
                .any(|s| s == "SELECT COUNT(*) FROM \"ADEMPIERE\".\"C_INVOICE\"")
        );
    }

    #[test]
    fn many_columns_are_split_across_queries_under_the_select_list_limit() {
        let cols: Vec<Value> = (0..600)
            .map(|i| json!({ "name": format!("N{i}"), "data_type": "NUMBER" }))
            .collect();
        let mut src = Recorded::default();
        // Every chunk answers COUNT(*) = 5 and "5" for each aggregate.
        src.answers
            .insert("WIDE".into(), Ok(vec![vec![Some("5".to_string()); 1000]]));
        let mut snap = snapshot(vec![table("WIDE", Value::Array(cols))]);
        let stats = scan(
            &src,
            &mut snap,
            ProfileTier::P2,
            &RedactionConfig::default(),
        )
        .unwrap();
        let queries = src
            .seen
            .borrow()
            .iter()
            .filter(|s| s.starts_with("SELECT"))
            .count();
        assert_eq!(queries, 2, "1,800 aggregates in chunks of 900");
        assert_eq!(stats.columns_measured, 600);
    }

    #[test]
    fn log_state_refused_is_unknown_never_off() {
        let mut src = Recorded::default();
        src.answers.insert(
            "v$database".into(),
            Err("ORA-00942: table or view does not exist".into()),
        );
        let s = log_state(&src, "ADEMPIERE");
        assert!(s["unknown"].as_str().unwrap().contains("ORA-00942"));
        assert!(s.get("log_mode").is_none());
    }
}
