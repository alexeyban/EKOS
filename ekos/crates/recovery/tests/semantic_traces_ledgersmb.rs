//! RFC 0170 — the business-meaning traces LedgerSMB's SQL leaves: how many column-vs-literal
//! predicates the views, routines and `CHECK` constraints carry, how many lookup tables are
//! seeded, and how they distribute. The floors are a ratchet (first run, 2026-10-03: 433 sites,
//! 318 resolved in place + 56 with in-scope candidates, 19 seeded tables); the distribution is
//! printed so the next change starts from evidence.
//!
//! ```text
//! EKOS_LEDGERSMB_DIR=/path/to/LedgerSMB cargo test -p ekos-recovery --test semantic_traces_ledgersmb -- --nocapture
//! ```

use ekos_recovery::parse_ddl_structural;
use ekos_recovery::plpgsql_analyzer::recover_routines;
use ekos_recovery::sql_dialect_registry::build_dialect_registry;
use ekos_recovery::view_analyzer::recover_views;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;

#[test]
fn ledgersmb_predicates() {
    let Some(root) = std::env::var_os("EKOS_LEDGERSMB_DIR").map(PathBuf::from) else {
        eprintln!("skipped: set EKOS_LEDGERSMB_DIR to a LedgerSMB checkout");
        return;
    };
    let registry = build_dialect_registry();
    let pg = registry.get("postgres").expect("postgres dialect");
    let dir = root.join("sql/modules");
    let mut files: Vec<(String, String)> = std::fs::read_to_string(dir.join("LOADORDER"))
        .expect("LOADORDER")
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|m| {
            (
                format!("sql/modules/{m}"),
                std::fs::read_to_string(dir.join(m)).unwrap(),
            )
        })
        .collect();
    files.push((
        "sql/Pg-database.sql".into(),
        std::fs::read_to_string(root.join("sql/Pg-database.sql")).unwrap(),
    ));

    let mut sites: Vec<Value> = Vec::new();
    let mut seeded = 0usize;
    for (path, sql) in &files {
        let (g, _) = recover_routines(path, sql);
        let (v, _) = recover_views(path, sql, pg.sqlparser_dialect().as_ref());
        for o in g.objects.iter().chain(v.objects.iter()) {
            if let Some(Value::Array(a)) = o.properties.get("predicates") {
                sites.extend(a.iter().cloned());
            }
        }
        let t = parse_ddl_structural(&pg.preprocess(sql), path, pg.sqlparser_dialect().as_ref());
        for o in &t.objects {
            if let Some(Value::Array(rows)) = o.properties.get("seed_rows") {
                eprintln!(
                    "seed: {} x{} e.g. {}",
                    o.name,
                    rows.len(),
                    rows[0]["values"]
                );
                seeded += 1;
            }
            if let Some(Value::Array(cs)) = o.properties.get("check_constraints") {
                for c in cs {
                    if let Some(Value::Array(a)) = c.get("predicates") {
                        sites.extend(a.iter().cloned());
                    }
                }
            }
        }
    }
    let mut by_clause: BTreeMap<String, usize> = BTreeMap::new();
    let mut by_op: BTreeMap<String, usize> = BTreeMap::new();
    let mut canon: BTreeMap<String, usize> = BTreeMap::new();
    let mut resolved = 0;
    let mut scoped = 0;
    for s in &sites {
        *by_clause
            .entry(s["clause"].as_str().unwrap().into())
            .or_default() += 1;
        *by_op.entry(s["op"].as_str().unwrap().into()).or_default() += 1;
        if s.get("relation").is_some() {
            resolved += 1;
        } else if s.get("scope").is_some() {
            scoped += 1;
        }
        let p: ekos_recovery::sql_predicates::PredicateSite =
            serde_json::from_value(s.clone()).unwrap();
        *canon.entry(p.canonical()).or_default() += 1;
    }
    eprintln!(
        "sites: {} ({resolved} resolved to a relation, {scoped} with in-scope candidates)",
        sites.len()
    );
    eprintln!("by clause: {by_clause:?}");
    eprintln!("by op: {by_op:?}");
    let mut top: Vec<_> = canon.into_iter().collect();
    top.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    for (c, n) in top.iter().take(80) {
        eprintln!("{n:4}  {c}");
    }
    for s in sites.iter().filter(|s| s.get("label").is_some()).take(60) {
        eprintln!(
            "label: {} {}.{} {:?} -> {}",
            s["clause"], s["relation"], s["column"], s["values"], s["label"]
        );
    }
    // PL/pgSQL parameters (`in_from_date IS NULL`) must never count as columns.
    assert!(
        !sites
            .iter()
            .any(|s| s["column"].as_str().is_some_and(|c| c.starts_with("in_"))),
        "a routine parameter leaked in as a column"
    );
    assert!(
        sites.len() >= 420,
        "predicate sites fell below the floor: {}",
        sites.len()
    );
    assert!(
        resolved >= 300,
        "resolved sites fell below the floor: {resolved}"
    );
    assert!(
        seeded >= 18,
        "seeded lookup tables fell below the floor: {seeded}"
    );
}
