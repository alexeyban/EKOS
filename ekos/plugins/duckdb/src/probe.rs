//! RFC 0259 D4: checking SQL against a DuckDB file — the text-to-SQL probes run through a
//! read-only, locked session.
//!
//! Four locks, each enough on its own for the common case:
//!
//! 1. the text must be one `SELECT` (`guard::single_select`) that names no file, table function
//!    or scalar reaching outside the database ([`crate::guard::probe_guard`]);
//! 2. the file is opened **read-only** (`access_mode = READ_ONLY`), so DuckDB refuses every write
//!    and writes nothing to the file, not even a WAL;
//! 3. before the first statement the session runs [`SESSION_LOCKS`]: external access off,
//!    extension auto-install and auto-load off, then `lock_configuration`, after which no `SET`
//!    can undo any of it;
//! 4. a deadline: DuckDB has no statement timeout, so a timer thread interrupts the query.
//!
//! The engine is the bundled `duckdb` crate behind this crate's `duckdb` feature (off by default,
//! like `oracle`); without it [`DuckDbProbe::open`] says so and nothing is opened.

use ekos_sql_dialect_sdk::probe::{Preview, Probe};
use std::path::Path;
use std::time::Duration;

/// How long one probe query may run before it is interrupted.
pub const DEFAULT_DEADLINE: Duration = Duration::from_secs(30);

/// Run, in order, before anything else on every session this crate opens.
pub const SESSION_LOCKS: &[&str] = &[
    "SET autoinstall_known_extensions = false",
    "SET autoload_known_extensions = false",
    "SET enable_external_access = false",
    "SET lock_configuration = true",
];

/// A probe over one read-only session. Not `Sync`: use it on the thread that opened it.
pub struct DuckDbProbe {
    #[cfg(feature = "duckdb")]
    session: crate::session::Session,
    label: String,
}

impl DuckDbProbe {
    /// Opens `path` read-only and locks the session. `Err` says why it will not be probed.
    #[cfg(feature = "duckdb")]
    pub fn open(path: &Path, deadline: Duration) -> Result<DuckDbProbe, String> {
        Ok(DuckDbProbe {
            session: crate::session::Session::open(path, deadline)?,
            label: label(path),
        })
    }

    /// Opens `path` read-only and locks the session. `Err` says why it will not be probed.
    #[cfg(not(feature = "duckdb"))]
    pub fn open(path: &Path, _deadline: Duration) -> Result<DuckDbProbe, String> {
        let _ = path;
        Err(crate::NOT_BUILT.into())
    }

    /// `duckdb:<path>`.
    pub fn label(&self) -> &str {
        &self.label
    }
}

#[cfg(feature = "duckdb")]
fn label(path: &Path) -> String {
    format!("duckdb:{}", path.display())
}

impl Probe for DuckDbProbe {
    fn preview(&self, sql: &str, max_rows: usize) -> Result<Preview, String> {
        crate::guard::probe_guard(sql)?;
        #[cfg(feature = "duckdb")]
        {
            self.session.preview(sql, max_rows)
        }
        #[cfg(not(feature = "duckdb"))]
        {
            let _ = max_rows;
            Err(crate::NOT_BUILT.into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refused_query_is_refused_before_the_file_is_opened() {
        // Built with or without the engine, the guard answers first.
        let probe = DuckDbProbe {
            #[cfg(feature = "duckdb")]
            session: crate::session::Session::open_in_memory(DEFAULT_DEADLINE).unwrap(),
            label: "duckdb:test".into(),
        };
        let e = probe.preview("COPY t TO '/tmp/x.csv'", 5).unwrap_err();
        assert!(e.starts_with("not sent"), "{e}");
    }

    #[cfg(not(feature = "duckdb"))]
    #[test]
    fn without_the_engine_nothing_is_opened() {
        let e = DuckDbProbe::open(Path::new("/nonexistent.duckdb"), DEFAULT_DEADLINE)
            .err()
            .unwrap();
        assert!(e.contains("duckdb feature"), "{e}");
    }
}
