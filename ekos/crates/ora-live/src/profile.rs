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
use ekos_common::pii;
use ekos_common::redaction::{RedactionConfig, redact};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// How much work a profile may do. Ordered: a higher tier includes everything below it.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ProfileTier {
    /// Optimizer statistics only. No scan of user data.
    #[default]
    P0,
    /// An exact row count per table.
    P1,
    /// One aggregate scan per table.
    P2,
}

impl ProfileTier {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::P0 => "p0",
            Self::P1 => "p1",
            Self::P2 => "p2",
        }
    }
}

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

/// What a profile run did, for the summary line and the `OracleCatalog` object.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileStats {
    pub tier: ProfileTier,
    pub tables_profiled: usize,
    /// Tables whose scan failed (timeout, privilege): recorded on the table, not fatal.
    pub tables_failed: usize,
    /// Tables not scanned because the session was lost (an ODPI-C call timeout closes it).
    pub tables_skipped: usize,
    /// Columns with an exact null count.
    pub columns_measured: usize,
    /// Columns whose bounds were withheld because their name classifies them as personal data.
    pub values_suppressed: usize,
}

/// Oracle refuses more expressions than this in one select list; leave room for `COUNT(*)`.
const EXPRESSIONS_PER_QUERY: usize = 900;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Family {
    Number,
    Date,
    Text,
    /// Counted (nulls) only: LOBs, RAW, XML, objects. `COUNT(lob)` is `ORA-00932`, so these are
    /// counted through `IS NOT NULL`, which every type accepts.
    Other,
    /// `LONG` / `LONG RAW`: not even `COUNT(col)` is legal.
    Long,
}

fn family(data_type: &str) -> Family {
    let t = data_type.to_ascii_uppercase();
    let base = t.split('(').next().unwrap_or_default().trim();
    match base {
        "LONG" | "LONG RAW" => Family::Long,
        "NUMBER" | "FLOAT" | "BINARY_FLOAT" | "BINARY_DOUBLE" | "INTEGER" => Family::Number,
        "DATE" => Family::Date,
        "VARCHAR2" | "NVARCHAR2" | "CHAR" | "NCHAR" => Family::Text,
        _ if base.starts_with("TIMESTAMP") => Family::Date,
        _ => Family::Other,
    }
}

/// One aggregate the P2 scan computes, and where its answer goes.
#[derive(Debug, Clone)]
struct Measure {
    column: usize,
    key: &'static str,
    sql: String,
}

/// The P2 aggregates for one table's columns. Pure, so the "never a text value" rule is tested
/// directly on the SQL it produces.
fn measures(columns: &[Value]) -> (Vec<Measure>, Vec<usize>) {
    let mut out = Vec::new();
    let mut suppressed = Vec::new();
    for (i, c) in columns.iter().enumerate() {
        let Some(name) = c["name"].as_str() else {
            continue;
        };
        let Ok(name) = guard_identifier(name) else {
            continue;
        };
        let col = format!("\"{name}\"");
        let fam = family(c["data_type"].as_str().unwrap_or_default());
        if fam == Family::Long {
            continue;
        }
        out.push(Measure {
            column: i,
            key: "non_null",
            sql: if fam == Family::Other {
                format!("COUNT(CASE WHEN {col} IS NOT NULL THEN 1 END)")
            } else {
                format!("COUNT({col})")
            },
        });
        match fam {
            Family::Number | Family::Date => {
                if pii::classify_name(name).is_some() {
                    suppressed.push(i);
                } else {
                    out.push(Measure {
                        column: i,
                        key: "min",
                        sql: format!("MIN({col})"),
                    });
                    out.push(Measure {
                        column: i,
                        key: "max",
                        sql: format!("MAX({col})"),
                    });
                }
            }
            Family::Text => out.push(Measure {
                column: i,
                key: "max_length",
                sql: format!("MAX(LENGTH({col}))"),
            }),
            Family::Other | Family::Long => {}
        }
    }
    (out, suppressed)
}

fn target(owner: &str, table: &str) -> Result<String, OraError> {
    Ok(format!(
        "\"{}\".\"{}\"",
        guard_identifier(owner)?,
        guard_identifier(table)?
    ))
}

fn count_of(rows: &Rows) -> Option<i64> {
    rows.first()?.first()?.as_deref()?.trim().parse().ok()
}

/// The session is gone after an ODPI-C call timeout (`DPI-1067`) or a lost connection; every later
/// call would fail the same way.
fn session_lost(e: &OraError) -> bool {
    let m = e.to_string();
    ["DPI-1067", "DPI-1080", "DPI-1010", "ORA-03113", "ORA-03135"]
        .iter()
        .any(|c| m.contains(c))
}

/// Profile every table of `snapshot` at `tier` (P1 or P2; P0 is read with the catalog), writing
/// the results into each table's `detail.profile` and its columns' `profile`.
pub fn scan(
    source: &dyn ScanSource,
    snapshot: &mut CatalogSnapshot,
    tier: ProfileTier,
    redaction: &RedactionConfig,
) -> Result<ProfileStats, OraError> {
    let mut stats = ProfileStats {
        tier,
        ..Default::default()
    };
    if tier == ProfileTier::P0 {
        return Ok(stats);
    }
    let owner = snapshot.owner.clone();
    source.begin_read_only()?;
    let mut lost = false;
    for t in snapshot
        .objects
        .iter_mut()
        .filter(|o| o.kind == ObjectKind::Table)
    {
        if lost {
            stats.tables_skipped += 1;
            t.detail["profile"]["skipped"] = "session lost after an earlier failure".into();
            continue;
        }
        let result = match tier {
            ProfileTier::P1 => count_rows(source, &owner, &t.name).map(|n| (n, 0, 0)),
            _ => {
                let columns = t.detail["columns"].as_array_mut();
                match columns {
                    Some(cols) => scan_table(source, &owner, &t.name, cols, redaction),
                    None => count_rows(source, &owner, &t.name).map(|n| (n, 0, 0)),
                }
            }
        };
        match result {
            Ok((rows, measured, suppressed)) => {
                t.detail["profile"] = TableProfile {
                    tier,
                    row_count: rows,
                    row_count_exact: true,
                }
                .into_json();
                stats.tables_profiled += 1;
                stats.columns_measured += measured;
                stats.values_suppressed += suppressed;
            }
            Err(e) => {
                lost = session_lost(&e);
                stats.tables_failed += 1;
                t.detail["profile"]["error"] = redact(&e.to_string(), redaction).into();
            }
        }
    }
    if !lost {
        source.end_transaction()?;
    }
    Ok(stats)
}

fn count_rows(source: &dyn ScanSource, owner: &str, table: &str) -> Result<i64, OraError> {
    let sql = format!("SELECT COUNT(*) FROM {}", target(owner, table)?);
    let rows = source.scan(&sql)?;
    count_of(&rows).ok_or_else(|| OraError::Query {
        sql,
        message: "COUNT(*) returned no number".into(),
    })
}

/// One table's P2 scan. Returns (rows, columns measured, columns suppressed).
fn scan_table(
    source: &dyn ScanSource,
    owner: &str,
    table: &str,
    columns: &mut [Value],
    redaction: &RedactionConfig,
) -> Result<(i64, usize, usize), OraError> {
    let from = target(owner, table)?;
    let (measures, suppressed) = measures(columns);
    let mut rows_total = None;
    let mut answers: Vec<(usize, &'static str, Option<String>)> = Vec::new();
    // A table without columns to measure still gets its count.
    let chunks: Vec<&[Measure]> = if measures.is_empty() {
        vec![&[]]
    } else {
        measures.chunks(EXPRESSIONS_PER_QUERY).collect()
    };
    for chunk in chunks {
        let list: Vec<&str> = std::iter::once("COUNT(*)")
            .chain(chunk.iter().map(|m| m.sql.as_str()))
            .collect();
        let sql = format!("SELECT {} FROM {from}", list.join(", "));
        let rows = source.scan(&sql)?;
        let row = rows.into_iter().next().ok_or_else(|| OraError::Query {
            sql: sql.clone(),
            message: "an aggregate query returned no row".into(),
        })?;
        rows_total = row
            .first()
            .and_then(|v| v.as_deref())
            .and_then(|v| v.trim().parse::<i64>().ok());
        for (m, v) in chunk.iter().zip(row.into_iter().skip(1)) {
            answers.push((m.column, m.key, v));
        }
    }
    let rows = rows_total.ok_or_else(|| OraError::Query {
        sql: format!("SELECT COUNT(*) FROM {from}"),
        message: "COUNT(*) returned no number".into(),
    })?;
    let mut measured = 0;
    for (i, key, v) in answers {
        let p = profile_of(&mut columns[i]);
        p["tier"] = "p2".into();
        match key {
            "non_null" => {
                if let Some(n) = v.and_then(|v| v.trim().parse::<i64>().ok()) {
                    p["nulls"] = (rows - n).into();
                    p["nulls_exact"] = true.into();
                    measured += 1;
                }
            }
            "max_length" => {
                if let Some(n) = v.and_then(|v| v.trim().parse::<i64>().ok()) {
                    p["max_length"] = n.into();
                }
            }
            _ => {
                if let Some(v) = v {
                    p[key] = redact(&v, redaction).into();
                }
            }
        }
    }
    for &i in &suppressed {
        let p = profile_of(&mut columns[i]);
        p["values_suppressed"] = true.into();
        if let Some(c) = columns[i]["name"].as_str().and_then(pii::classify_name) {
            columns[i]["profile"]["pii"] = serde_json::to_value(c).unwrap_or(Value::Null);
        }
    }
    Ok((rows, measured, suppressed.len()))
}

fn profile_of(column: &mut Value) -> &mut Value {
    if !column["profile"].is_object() {
        column["profile"] = json!({});
    }
    &mut column["profile"]
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

    /// The rule the whole tier rests on: a text column is never the argument of `MIN` or `MAX`.
    #[test]
    fn no_text_column_is_ever_an_argument_of_min_or_max() {
        let (m, suppressed) = measures(invoice().detail["columns"].as_array().unwrap());
        let sql: Vec<&str> = m.iter().map(|m| m.sql.as_str()).collect();
        for text in ["DOCUMENTNO", "EMAIL"] {
            assert!(
                sql.iter()
                    .filter(|s| s.contains(text))
                    .all(|s| s.starts_with("COUNT(") || s.starts_with("MAX(LENGTH(")),
                "{text}: {sql:?}"
            );
        }
        assert!(sql.contains(&"MIN(\"DATEINVOICED\")"));
        assert!(
            !sql.iter().any(|s| s.contains("NOTES")),
            "LONG is not countable"
        );
        assert!(sql.contains(&"COUNT(CASE WHEN \"ATTACHMENT\" IS NOT NULL THEN 1 END)"));
        // A number whose name says personal data gets a count and nothing else.
        assert!(
            sql.iter()
                .filter(|s| s.contains("PHONE_NUMBER"))
                .all(|s| s.starts_with("COUNT("))
        );
        assert_eq!(suppressed, vec![4]);
    }

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
