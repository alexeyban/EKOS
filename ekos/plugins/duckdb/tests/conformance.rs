//! RFC 0259 §5 (D3): RFC 0255's conformance kit against an in-process DuckDB, through the same
//! locked session the probe uses. Needs the engine (`--features duckdb`).
#![cfg(feature = "duckdb")]

use ekos_migrate_validate::conformance;
use ekos_migrate_validate::duckdb::{DuckDbLiterals, dialect};
use ekos_migrate_validate::{Dialect, EngineReader, ReadError};
use ekos_plugin_duckdb::probe::DEFAULT_DEADLINE;
use ekos_plugin_duckdb::session::Session;

struct InProcess(Session);

impl EngineReader for InProcess {
    fn dialect(&self) -> Dialect {
        dialect()
    }
    fn label(&self) -> &str {
        "duckdb-in-process::memory:"
    }
    fn query(&self, sql: &str) -> Result<Vec<Vec<String>>, ReadError> {
        let p = self.0.query_text(sql).map_err(|message| ReadError::Query {
            engine: "duckdb".into(),
            message,
        })?;
        Ok(p.rows
            .into_iter()
            .map(|r| r.into_iter().map(Option::unwrap_or_default).collect())
            .collect())
    }
}

#[test]
fn the_kit_passes_in_process() {
    let r = InProcess(Session::open_in_memory(DEFAULT_DEADLINE).unwrap());
    let report = conformance::run(&r, &DuckDbLiterals);
    eprintln!(
        "RFC 0255 conformance {} (in process): {} cases, {} hashes, buckets {}, passed {}",
        report.dialect,
        report.cases_checked,
        report.hashes_checked,
        report.buckets_checked,
        report.passed()
    );
    assert!(report.passed(), "{report:#?}");
    assert_eq!(report.cases_checked, 31);
}
