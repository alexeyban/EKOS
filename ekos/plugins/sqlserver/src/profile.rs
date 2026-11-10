//! RFC 0211 §3.4 / RFC 0252 §3.1 for SQL Server — the `[[databases]] profile` tiers.
//!
//! - **P0** (always): `sys.partitions.rows` per table, which SQL Server documents as approximate
//!   (`row_count_exact: false`). SQL Server's column statistics live in histograms
//!   (`DBCC SHOW_STATISTICS`), which are sampled *values*: they are never read.
//! - **P1 / P2**: the aggregate queries `ekos_observation_sdk::profile` plans in T-SQL
//!   (`Flavor::SqlServer`), one table at a time on the read-only session.
//!
//! Every function takes the query runner as a closure, so the tiers are tested without a server.

use ekos_common::redaction::RedactionConfig;
use ekos_observation_sdk::live_catalog::OwnerCatalog;
use ekos_observation_sdk::profile::{self, Flavor, ProfileStats, ProfileTier, TableOutcome};
use serde_json::json;
use std::collections::BTreeMap;

use crate::session::{Rows, session_lost};

/// Runs one query; rows of cells, `None` for SQL NULL.
pub type Run<'a> = dyn FnMut(&str) -> Result<Rows, String> + 'a;

/// P0: the row estimates of every table in `catalogs`, from one query over all listed schemas.
pub fn estimates(
    catalogs: &mut [OwnerCatalog],
    schemas: &[String],
    run: &mut Run<'_>,
) -> Result<(), String> {
    let rows = run(&crate::catalog::row_estimates(schemas))?;
    let by_table: BTreeMap<(String, String), i64> = rows
        .iter()
        .filter_map(|r| {
            let n = r.get(2)?.as_deref()?.trim().parse::<i64>().ok()?;
            Some(((r.first()?.clone()?, r.get(1)?.clone()?), n))
        })
        .collect();
    for c in catalogs.iter_mut() {
        for o in c.objects.iter_mut().filter(|o| o.kind == "table") {
            if let Some(n) = by_table.get(&(o.owner.clone(), o.name.clone())) {
                o.detail["num_rows"] = (*n).into();
                o.detail["profile"] = json!({
                    "tier": "p0", "row_count": n, "row_count_exact": false,
                });
            }
        }
    }
    Ok(())
}

/// P1 / P2 over every table of `catalog`. Failures are recorded on their table; after a lost
/// session (a timeout leaves the TDS stream out of step) the remaining tables are skipped.
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
            Flavor::SqlServer,
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

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_observation_sdk::live_catalog::LiveObject;

    fn catalog() -> OwnerCatalog {
        OwnerCatalog {
            owner: "sales".into(),
            family: "sys".into(),
            objects: vec![LiveObject {
                kind: "table".into(),
                owner: "sales".into(),
                name: "orders".into(),
                status: None,
                detail: json!({ "columns": [
                    { "name": "id", "data_type": "bigint", "nullable": false },
                    { "name": "note", "data_type": "nvarchar(200)", "nullable": true },
                    { "name": "placed_at", "data_type": "datetime2(7)", "nullable": true },
                    { "name": "email", "data_type": "varchar(100)", "nullable": true },
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
    fn p0_reads_partition_row_estimates_never_statistics_histograms() {
        let mut cats = vec![catalog()];
        let mut seen = Vec::new();
        estimates(&mut cats, &["sales".into()], &mut |sql: &str| {
            seen.push(sql.to_string());
            Ok(vec![some(&["sales", "orders", "1200"])])
        })
        .unwrap();
        assert!(seen[0].contains("sys.partitions"));
        assert!(
            !seen
                .iter()
                .any(|s| s.contains("SHOW_STATISTICS") || s.contains("histogram"))
        );
        let t = &cats[0].objects[0].detail;
        assert_eq!(t["num_rows"], 1200);
        assert_eq!(t["profile"]["row_count_exact"], false);
    }

    #[test]
    fn p2_scans_each_table_once_with_no_text_value_read() {
        let mut c = catalog();
        let mut seen = Vec::new();
        let stats = scan(
            &mut c,
            ProfileTier::P2,
            &RedactionConfig::default(),
            &mut |sql: &str| {
                seen.push(sql.to_string());
                // COUNT_BIG(*), id count/min/max, note count/len, placed_at count/min/max,
                // email count/len
                Ok(vec![some(&[
                    "10",
                    "10",
                    "1",
                    "10",
                    "7",
                    "42",
                    "9",
                    "2024-01-01T00:00:00",
                    "2026-10-30T12:00:00",
                    "8",
                    "31",
                ])])
            },
        );
        assert_eq!(seen.len(), 1, "{seen:?}");
        assert!(seen[0].ends_with("FROM [sales].[orders]"), "{}", seen[0]);
        assert!(!seen[0].contains("MIN([note])") && !seen[0].contains("MIN([email])"));
        assert_eq!((stats.tables_profiled, stats.columns_measured), (1, 4));
        let t = &c.objects[0].detail;
        assert_eq!(t["profile"]["row_count"], 10);
        assert_eq!(t["profile"]["row_count_exact"], true);
        assert_eq!(t["columns"][1]["profile"]["nulls"], 3);
        assert_eq!(t["columns"][1]["profile"]["max_length"], 42);
        assert_eq!(t["columns"][2]["profile"]["max"], "2026-10-30T12:00:00");
    }

    #[test]
    fn a_timeout_skips_the_remaining_tables_a_refusal_does_not() {
        let mut c = catalog();
        let mut second = c.objects[0].clone();
        second.name = "lines".into();
        c.objects.push(second);
        let stats = scan(
            &mut c,
            ProfileTier::P1,
            &RedactionConfig::default(),
            &mut |_| Err("Operation timed out".into()),
        );
        assert_eq!((stats.tables_failed, stats.tables_skipped), (1, 1));
        let mut c = catalog();
        c.objects.push(c.objects[0].clone());
        let stats = scan(
            &mut c,
            ProfileTier::P1,
            &RedactionConfig::default(),
            &mut |_| Err("The SELECT permission was denied".into()),
        );
        assert_eq!((stats.tables_failed, stats.tables_skipped), (2, 0));
    }
}
