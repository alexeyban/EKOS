//! RFC 0259 D5 against a real DuckDB file (needs `--features duckdb`): the catalog read
//! reconciles, the write probe is refused, the profile and RFC 0252's signal queries run in DuckDB
//! SQL, and the file is byte-identical afterwards.
#![cfg(feature = "duckdb")]

use ekos_common::redaction::RedactionConfig;
use ekos_observation_sdk::probe::Probe;
use ekos_observation_sdk::profile::{Flavor, ProfileTier};
use ekos_observation_sdk::{overlap, probe, signals};
use ekos_plugin_duckdb::catalog::{self, read};
use ekos_plugin_duckdb::probe::DEFAULT_DEADLINE;
use ekos_plugin_duckdb::session::Session;
use std::path::{Path, PathBuf};

fn mart(dir: &Path) -> PathBuf {
    let path = dir.join("marts.duckdb");
    let c = duckdb::Connection::open(&path).unwrap();
    c.execute_batch(
        "CREATE SCHEMA gold;
         CREATE TABLE gold.dim_status (code VARCHAR PRIMARY KEY, label VARCHAR);
         CREATE TABLE gold.fct_invoice (
             id BIGINT PRIMARY KEY,
             docstatus VARCHAR REFERENCES gold.dim_status (code),
             dateinvoiced DATE,
             total DECIMAL(18, 2) CHECK (total >= 0),
             email VARCHAR);
         COMMENT ON TABLE gold.fct_invoice IS 'Invoices by status';
         INSERT INTO gold.dim_status VALUES ('CO', 'Completed'), ('DR', 'Drafted');
         INSERT INTO gold.fct_invoice SELECT i, CASE WHEN i % 3 = 0 THEN 'DR' ELSE 'CO' END,
             DATE '2024-01-01' + CAST(i AS INTEGER), i * 1.25, 'p' || i || '@example.com'
             FROM range(1, 301) t(i);
         CREATE VIEW gold.v_open AS SELECT * FROM gold.fct_invoice WHERE docstatus <> 'CO';
         CREATE TABLE main.other (x INTEGER);
         CHECKPOINT;",
    )
    .unwrap();
    c.close().map_err(|(_, e)| e).unwrap();
    path
}

#[test]
fn the_catalog_reconciles_profiles_and_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let path = mart(dir.path());
    let before = std::fs::read(&path).unwrap();
    let r = read(
        &path,
        &["gold".to_string()],
        &RedactionConfig::default(),
        ProfileTier::P2,
    )
    .unwrap();
    assert_eq!(r.catalogs.len(), 1, "main is not listed");
    let gold = &r.catalogs[0];
    assert_eq!(gold.census["TABLE"].valid, 2);
    assert_eq!(gold.census["VIEW"].valid, 1);
    let kinds: Vec<(&str, &str)> = gold
        .objects
        .iter()
        .map(|o| (o.kind.as_str(), o.name.as_str()))
        .collect();
    for want in [
        ("table", "fct_invoice"),
        ("table", "dim_status"),
        ("view", "v_open"),
    ] {
        assert!(kinds.contains(&want), "{want:?} in {kinds:?}");
    }
    for kind in ["primary_key", "check_constraint", "foreign_key"] {
        assert!(kinds.iter().any(|k| k.0 == kind), "{kind} in {kinds:?}");
    }
    let fct = gold
        .objects
        .iter()
        .find(|o| o.name == "fct_invoice")
        .unwrap();
    assert_eq!(fct.detail["comment"], "Invoices by status");
    let profile = gold.profile.as_ref().unwrap();
    assert_eq!(profile.tables_profiled, 2, "{profile:?}");
    // The no-value rule: a text column's bounds are never read, a personal column not at all.
    let text = fct.detail.to_string();
    assert!(!text.contains("@example.com"), "{text}");
    assert!(!text.contains("\"CO\""), "{text}");
    assert!(
        text.contains("2024-01-02"),
        "a date bound is measured: {text}"
    );
    assert_eq!(
        r.write_check.as_ref().unwrap()["result"],
        "refused_by_server",
        "{:?}",
        r.write_check
    );
    let report = catalog::test_report("duckdb:x", &r);
    assert_eq!(report["owners"][0]["listed"]["table"], 2);
    assert_eq!(std::fs::read(&path).unwrap(), before, "the file changed");
}

/// RFC 0252's builders render DuckDB SQL the engine runs, and what comes back is counts, code
/// literals and MD5 minimums — never another value.
#[test]
fn the_signal_queries_run_in_duckdb() {
    let dir = tempfile::tempdir().unwrap();
    let path = mart(dir.path());
    let s = Session::open(&path, DEFAULT_DEADLINE).unwrap();
    let f = Flavor::DuckDb;
    let lits = vec!["'CO'".to_string(), "'DR'".to_string()];
    let run = |sql: String| {
        s.query_text(&sql)
            .unwrap_or_else(|e| panic!("{e}\n{sql}"))
            .rows
    };
    let cov = run(signals::literal_coverage_sql(
        f,
        "gold",
        "fct_invoice",
        "docstatus",
        &lits,
        Some("dateinvoiced"),
    )
    .unwrap());
    assert!(!cov.is_empty());
    let monthly = run(signals::literal_monthly_sql(
        f,
        "gold",
        "fct_invoice",
        "docstatus",
        &lits,
        "dateinvoiced",
    )
    .unwrap());
    assert!(
        monthly.iter().any(|r| r[1].as_deref() == Some("2024-01")),
        "{monthly:?}"
    );
    run(signals::unexplained_distinct_sql(f, "gold", "fct_invoice", "docstatus", &lits).unwrap());
    run(signals::conformance_sql(
        f,
        "gold",
        "fct_invoice",
        "docstatus",
        signals::CONVERT_NUMBER,
        None,
    )
    .unwrap());
    run(signals::conditional_sql(
        f,
        "gold",
        "fct_invoice",
        "docstatus",
        &lits,
        &["total".to_string()],
    )
    .unwrap());
    let sig =
        run(overlap::signature_sql(f, "gold", "fct_invoice", "docstatus", "salt", false).unwrap());
    let parsed = overlap::parse_signature(&sig[0]).unwrap();
    let parent =
        run(overlap::signature_sql(f, "gold", "dim_status", "code", "salt", false).unwrap());
    let parent = overlap::parse_signature(&parent[0]).unwrap();
    assert!(
        overlap::containment(&parsed, &parent) > 0.99,
        "same codes, same signatures"
    );
    run(overlap::inclusion_sql(
        f,
        ("gold", "fct_invoice", "docstatus"),
        ("gold", "dim_status", "code"),
    )
    .unwrap());
    let columns = vec![
        serde_json::json!({ "name": "docstatus", "data_type": "VARCHAR" }),
        serde_json::json!({ "name": "dateinvoiced", "data_type": "DATE" }),
    ];
    let p: Probe = serde_json::from_value(serde_json::json!({
        "template": "over_time", "column": "docstatus", "time_column": "dateinvoiced", "grain": "month"
    }))
    .unwrap();
    run(probe::render(f, "gold", "fct_invoice", &columns, &p, None).unwrap());
}
