//! RFC 0163 — `PlPgSqlAnalyzerPass` over LedgerSMB's installed modules: how much of the SQL
//! embedded in real routines parses, and what fails.
//!
//! The parser corpus (`ekos-plpgsql/tests/ledgersmb_corpus.rs`) proves every routine's *structure*
//! recovers. This proves the SQL *inside* the statements parses well enough to say which tables a
//! routine reads and writes — the floor below is a ratchet, and every failure is printed with its
//! parser error so the next fix starts from evidence.
//!
//! ```text
//! EKOS_LEDGERSMB_DIR=/path/to/LedgerSMB cargo test -p ekos-recovery --test plpgsql_ledgersmb -- --nocapture
//! ```

use ekos_kir::ObjectKind;
use ekos_recovery::plpgsql_analyzer::recover_routines;
use std::collections::BTreeMap;
use std::path::PathBuf;

#[test]
fn ledgersmb_embedded_sql_parses_above_the_floor() {
    let Some(root) = std::env::var_os("EKOS_LEDGERSMB_DIR").map(PathBuf::from) else {
        eprintln!("skipped: set EKOS_LEDGERSMB_DIR to a LedgerSMB checkout");
        return;
    };
    let dir = root.join("sql/modules");
    let modules: Vec<String> = std::fs::read_to_string(dir.join("LOADORDER"))
        .expect("LOADORDER")
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_string)
        .collect();

    let mut status: BTreeMap<String, usize> = BTreeMap::new();
    let mut routine_status: BTreeMap<String, usize> = BTreeMap::new();
    let mut failures = Vec::new();
    for m in &modules {
        let sql = std::fs::read_to_string(dir.join(m)).expect("module");
        let (graph, _) = recover_routines(&format!("sql/modules/{m}"), &sql);
        for o in &graph.objects {
            let ObjectKind::Custom(kind) = &o.kind else {
                continue;
            };
            let s = o
                .properties
                .get("footprint")
                .and_then(|v| v.as_str())
                .unwrap_or("?")
                .to_string();
            match kind.as_str() {
                "ProcedureStatement" => {
                    *status.entry(s.clone()).or_default() += 1;
                    if s == "unparsed" || s == "partial" {
                        failures.push(format!(
                            "{} line {}: {}",
                            m,
                            o.properties["line"],
                            o.properties
                                .get("footprint_errors")
                                .map(|v| v.to_string())
                                .unwrap_or_default()
                        ));
                    }
                }
                "Procedure" => *routine_status.entry(s).or_default() += 1,
                _ => {}
            }
        }
    }
    let attempted: usize = status
        .iter()
        .filter(|(k, _)| k.as_str() != "none")
        .map(|(_, v)| v)
        .sum();
    let parsed = status.get("parsed").copied().unwrap_or(0);
    eprintln!("statements by footprint: {status:?}; routines: {routine_status:?}");
    for f in &failures {
        eprintln!("  {f}");
    }

    // The ratchet: statements with SQL or expressions whose every fragment parsed. 1137/1141 at
    // LedgerSMB `544bcd947`; the four left are sqlparser 0.53 grammar gaps, not EKOS bugs —
    // `ON CONFLICT (expression)`, a data-modifying CTE, and `OVERRIDING SYSTEM VALUE`.
    let floor = 1137;
    assert!(
        parsed >= floor,
        "{parsed}/{attempted} statements' SQL parsed (floor {floor})"
    );
}
