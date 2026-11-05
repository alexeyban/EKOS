//! RFC 0243: checking SQL against a live Oracle database — the text-to-SQL probes run through
//! `ekos-ora-live`'s session, each inside `SET TRANSACTION READ ONLY` (the server refuses any
//! write in it, whatever the user's privileges). The text must also be one SELECT
//! (`guard::single_select`) before anything is sent. Without the `oracle` feature the driver is
//! not built and connecting says so.

use ekos_ora_live::{OraConnection, OraSource, SessionPolicy};
use ekos_sql_dialect_sdk::RowLimit;
use ekos_sql_dialect_sdk::guard::{single_select, wrap_rows};
use ekos_sql_dialect_sdk::probe::{Preview, Probe};
use sqlparser::dialect::GenericDialect;

/// A probe over one read-only session. Use it on the thread that connected.
pub struct OraProbe {
    source: OraSource,
}

impl OraProbe {
    pub fn connect(connection: &OraConnection, run_id: &str) -> Result<OraProbe, String> {
        let source = OraSource::connect(connection, run_id, &SessionPolicy::default())
            .map_err(|e| e.to_string())?;
        Ok(OraProbe { source })
    }
}

impl Probe for OraProbe {
    fn preview(&self, sql: &str, max_rows: usize) -> Result<Preview, String> {
        // The Oracle dialect plugin parses with sqlparser's generic dialect too.
        single_select(sql, &GenericDialect {})?;
        let wrapped = wrap_rows(sql, RowLimit::FetchFirst, max_rows + 1);
        let r = self
            .source
            .preview(&wrapped, max_rows)
            .map_err(|e| e.to_string())?;
        Ok(Preview {
            columns: r.columns,
            rows: r.rows,
            truncated: r.truncated,
        })
    }
}
