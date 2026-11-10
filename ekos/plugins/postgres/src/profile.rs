//! RFC 0211 §3.4 for PostgreSQL — the `[[databases]] profile` tiers.
//!
//! - **P0** (always): the planner's statistics, two catalog queries per schema. `pg_class.reltuples`
//!   gives each table's estimated rows; `pg_stats` gives each column's null fraction, distinct
//!   estimate and average width. Only those counts are read — never `most_common_vals` or
//!   `histogram_bounds`, which are values sampled from the table.
//! - **P1 / P2**: the aggregate queries `ekos_observation_sdk::profile` plans, run one table at a
//!   time on the read-only session (`default_transaction_read_only = on`, statement timeout).
//!
//! Every function takes the query runner as a closure, so the tiers are tested without a server.

use ekos_common::redaction::RedactionConfig;
use ekos_observation_sdk::live_catalog::OwnerCatalog;
use ekos_observation_sdk::profile::{self, Flavor, ProfileStats, ProfileTier, TableOutcome};
use serde_json::json;
use std::collections::BTreeMap;

/// Runs one query; rows of cells, `None` for SQL NULL.
pub type Run<'a> = dyn FnMut(&str) -> Result<Vec<Vec<Option<String>>>, String> + 'a;

fn literal(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

fn cell(row: &[Option<String>], i: usize) -> Option<&str> {
    row.get(i)
        .and_then(Option::as_deref)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn float(row: &[Option<String>], i: usize) -> Option<f64> {
    cell(row, i)?.parse().ok()
}

/// P0: the planner's statistics onto every table and column of `catalog`. A table never analyzed
/// (`reltuples` = -1) gets no row estimate rather than a negative one.
pub fn statistics(catalog: &mut OwnerCatalog, run: &mut Run<'_>) -> Result<(), String> {
    let schema = literal(&catalog.owner);
    let tables = run(&format!(
        "SELECT c.relname, c.reltuples::bigint::text, \
                GREATEST(s.last_analyze, s.last_autoanalyze)::text \
         FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
         LEFT JOIN pg_stat_user_tables s ON s.relid = c.oid \
         WHERE n.nspname = {schema} AND c.relkind IN ('r', 'p')"
    ))?;
    let columns = run(&format!(
        "SELECT tablename, attname, null_frac::text, n_distinct::text, avg_width::text \
         FROM pg_stats WHERE schemaname = {schema}"
    ))?;
    let mut by_table: BTreeMap<(String, String), (f64, f64, f64)> = BTreeMap::new();
    for r in &columns {
        let (Some(t), Some(c)) = (cell(r, 0), cell(r, 1)) else {
            continue;
        };
        by_table.insert(
            (t.to_string(), c.to_string()),
            (
                float(r, 2).unwrap_or(0.0),
                float(r, 3).unwrap_or(0.0),
                float(r, 4).unwrap_or(0.0),
            ),
        );
    }
    let rows: BTreeMap<String, (Option<i64>, Option<String>)> = tables
        .iter()
        .filter_map(|r| {
            let name = cell(r, 0)?.to_string();
            let n = cell(r, 1)
                .and_then(|v| v.parse::<i64>().ok())
                .filter(|n| *n >= 0);
            Some((name, (n, cell(r, 2).map(str::to_string))))
        })
        .collect();
    for o in catalog.objects.iter_mut().filter(|o| o.kind == "table") {
        let (n, analyzed) = rows.get(&o.name).cloned().unwrap_or_default();
        if let Some(n) = n {
            o.detail["num_rows"] = n.into();
            o.detail["profile"] = json!({
                "tier": "p0", "row_count": n, "row_count_exact": false,
            });
        }
        if let Some(t) = &analyzed {
            o.detail["last_analyzed"] = t.clone().into();
        }
        let table = o.name.clone();
        let Some(cols) = o.detail["columns"].as_array_mut() else {
            continue;
        };
        for col in cols {
            let name = col["name"].as_str().unwrap_or_default().to_string();
            let Some((null_frac, n_distinct, width)) = by_table.get(&(table.clone(), name)) else {
                continue;
            };
            // A P2 scan later makes `nulls` exact; `distinct` stays the planner's estimate.
            let mut p = json!({
                "tier": "p0", "nulls_exact": false, "distinct_exact": false,
                "avg_length": *width as i64,
            });
            if let Some(n) = n {
                p["nulls"] = ((null_frac * n as f64).round() as i64).into();
                // PostgreSQL's convention: a negative `n_distinct` is a fraction of the rows.
                let distinct = if *n_distinct < 0.0 {
                    Some((-n_distinct * n as f64).round() as i64)
                } else if *n_distinct > 0.0 {
                    Some(n_distinct.round() as i64)
                } else {
                    None
                };
                if let Some(d) = distinct {
                    p["distinct"] = d.into();
                }
            }
            if let Some(t) = &analyzed {
                p["analyzed"] = t.clone().into();
            }
            col["profile"] = p;
        }
    }
    Ok(())
}

/// The connection is gone; every later query would fail the same way. A statement timeout or a
/// refused table leaves the session usable.
fn session_lost(message: &str) -> bool {
    let m = message.to_ascii_lowercase();
    m.contains("connection closed") || m.contains("terminating connection")
}

/// P1 / P2 over every table of `catalog`. Returns what was measured; failures are recorded on
/// their table.
pub fn scan(
    catalog: &mut OwnerCatalog,
    tier: ProfileTier,
    redaction: &RedactionConfig,
    run: &mut Run<'_>,
) -> ProfileStats {
    let mut stats = ProfileStats::new(tier);
    if tier == ProfileTier::P0 {
        return stats;
    }
    let owner = catalog.owner.clone();
    let mut lost = false;
    for o in catalog.objects.iter_mut().filter(|o| o.kind == "table") {
        if lost {
            profile::record_skipped(&mut o.detail);
            stats.record(&TableOutcome::Skipped);
            continue;
        }
        let mut one = |sql: &str| -> Result<Vec<Option<String>>, String> {
            run(sql)?
                .into_iter()
                .next()
                .ok_or_else(|| "an aggregate query returned no row".to_string())
        };
        match profile::profile_table(
            Flavor::Postgres,
            &owner,
            &o.name,
            &mut o.detail,
            tier,
            redaction,
            &mut one,
        ) {
            Ok(outcome) => stats.record(&outcome),
            Err(e) => {
                lost = session_lost(&e);
                stats.record(&TableOutcome::Failed);
            }
        }
    }
    stats
}

/// Both steps for one schema: statistics always, then the tier's scans.
pub fn profile_owner(
    catalog: &mut OwnerCatalog,
    tier: ProfileTier,
    redaction: &RedactionConfig,
    run: &mut Run<'_>,
) -> Result<(), String> {
    statistics(catalog, run)?;
    if tier != ProfileTier::P0 {
        catalog.profile = Some(scan(catalog, tier, redaction, run));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_observation_sdk::live_catalog::LiveObject;

    fn catalog() -> OwnerCatalog {
        OwnerCatalog {
            owner: "public".into(),
            family: "pg_catalog".into(),
            objects: vec![LiveObject {
                kind: "table".into(),
                owner: "public".into(),
                name: "ar".into(),
                status: None,
                detail: json!({ "columns": [
                    { "name": "id", "data_type": "integer", "nullable": false },
                    { "name": "invnumber", "data_type": "text", "nullable": true },
                    { "name": "transdate", "data_type": "date", "nullable": true },
                ] }),
            }],
            ..Default::default()
        }
    }

    fn some(v: &[&str]) -> Vec<Option<String>> {
        v.iter()
            .map(|s| (!s.is_empty()).then(|| s.to_string()))
            .collect()
    }

    #[test]
    fn p0_reads_counts_from_the_planner_never_values() {
        let mut seen = Vec::new();
        let mut c = catalog();
        statistics(&mut c, &mut |sql: &str| {
            seen.push(sql.to_string());
            Ok(if sql.contains("pg_stats") {
                vec![
                    some(&["ar", "id", "0", "-1", "4"]),
                    some(&["ar", "invnumber", "0.25", "-0.5", "9"]),
                ]
            } else {
                vec![some(&["ar", "200", "2026-11-01 10:00:00+00"])]
            })
        })
        .unwrap();
        for s in &seen {
            assert!(
                !s.contains("most_common_vals") && !s.contains("histogram_bounds"),
                "{s}"
            );
        }
        let t = &c.objects[0].detail;
        assert_eq!(t["num_rows"], 200);
        assert_eq!(t["profile"]["row_count_exact"], false);
        assert_eq!(t["columns"][0]["profile"]["distinct"], 200);
        assert_eq!(t["columns"][1]["profile"]["nulls"], 50);
        assert_eq!(t["columns"][1]["profile"]["distinct"], 100);
        assert_eq!(t["columns"][1]["profile"]["distinct_exact"], false);
        assert!(t["columns"][2].get("profile").is_none(), "not analyzed");
    }

    #[test]
    fn a_never_analyzed_table_gets_no_row_estimate() {
        let mut c = catalog();
        statistics(&mut c, &mut |sql: &str| {
            Ok(if sql.contains("pg_stats") {
                vec![]
            } else {
                vec![some(&["ar", "-1", ""])]
            })
        })
        .unwrap();
        assert!(c.objects[0].detail.get("num_rows").is_none());
    }

    #[test]
    fn p2_scans_each_table_once_and_records_the_summary() {
        let mut c = catalog();
        let mut seen = Vec::new();
        profile_owner(
            &mut c,
            ProfileTier::P2,
            &RedactionConfig::default(),
            &mut |sql: &str| {
                seen.push(sql.to_string());
                Ok(if sql.starts_with("SELECT count(*)") {
                    // count(*), id count/min/max, invnumber count/len, transdate count/min/max
                    vec![some(&[
                        "12",
                        "12",
                        "1",
                        "12",
                        "9",
                        "14",
                        "12",
                        "2020-01-01",
                        "2026-10-30",
                    ])]
                } else {
                    vec![]
                })
            },
        )
        .unwrap();
        let scans: Vec<&String> = seen
            .iter()
            .filter(|s| s.contains("\"public\".\"ar\""))
            .collect();
        assert_eq!(scans.len(), 1, "{seen:?}");
        let t = &c.objects[0].detail;
        assert_eq!(t["profile"]["row_count"], 12);
        assert_eq!(t["profile"]["row_count_exact"], true);
        assert_eq!(t["columns"][1]["profile"]["nulls"], 3);
        assert_eq!(t["columns"][1]["profile"]["max_length"], 14);
        assert_eq!(t["columns"][2]["profile"]["max"], "2026-10-30");
        let stats = c.profile.unwrap();
        assert_eq!((stats.tier, stats.tables_profiled), (ProfileTier::P2, 1));
    }

    #[test]
    fn a_lost_connection_skips_the_remaining_tables() {
        let mut c = catalog();
        let mut second = c.objects[0].clone();
        second.name = "ap".into();
        c.objects.push(second);
        let stats = scan(
            &mut c,
            ProfileTier::P1,
            &RedactionConfig::default(),
            &mut |_| Err("connection closed".into()),
        );
        assert_eq!((stats.tables_failed, stats.tables_skipped), (1, 1));
    }
}
