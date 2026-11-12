//! RFC 0256 — iDempiere's application dictionary when the application runs on PostgreSQL.
//!
//! The Oracle connector reads `AD_Table`'s flags (RFC 0212) and `AD_Process` / `AD_Scheduler`
//! (RFC 0256 open question 4) from the dictionary; iDempiere runs on PostgreSQL as well, with the
//! same tables in the application's schema. The queries, the row mapping and the types are
//! [`ekos_observation_sdk::app_dictionary`]'s, so both engines produce the same artifact: the only
//! thing here is running the two queries in one read-only snapshot.

use crate::PgError;
use crate::session::PgSource;
use ekos_observation_sdk::app_dictionary::{self as shared, Rows};

pub use shared::{AppProcess, DictionaryKind, TableFlags};

/// Where dictionary rows come from: the live session, or recorded rows in tests.
pub trait DictionarySource {
    fn begin_read_only(&self) -> Result<(), PgError>;
    fn rows(&self, sql: &str) -> Result<Rows, PgError>;
    fn end_transaction(&self) -> Result<(), PgError>;
}

impl DictionarySource for PgSource {
    fn begin_read_only(&self) -> Result<(), PgError> {
        self.begin_read_only_snapshot()
    }
    fn rows(&self, sql: &str) -> Result<Rows, PgError> {
        Ok(self.preview(sql, usize::MAX)?.rows)
    }
    fn end_transaction(&self) -> Result<(), PgError> {
        PgSource::end_transaction(self)
    }
}

fn refused(schema: &str) -> PgError {
    PgError::Query {
        sql: String::new(),
        message: format!(
            "refusing to read a dictionary from schema {schema:?}: not a plain identifier"
        ),
    }
}

/// The table flags and the processes of `schema`'s dictionary, in one read-only snapshot. The
/// schema is spelled as PostgreSQL stores it (`adempiere`).
pub fn read_all(
    source: &dyn DictionarySource,
    kind: DictionaryKind,
    schema: &str,
) -> Result<(Vec<TableFlags>, Vec<AppProcess>), PgError> {
    let tables_sql = shared::tables_query(kind, schema).map_err(|_| refused(schema))?;
    let processes_sql = shared::processes_query(kind, schema).map_err(|_| refused(schema))?;
    source.begin_read_only()?;
    let tables = source.rows(&tables_sql);
    let procs = tables.is_ok().then(|| source.rows(&processes_sql));
    let ended = source.end_transaction();
    let tables = tables?;
    let procs = procs.transpose()?.unwrap_or_default();
    ended?;
    Ok((shared::table_flags(tables), shared::processes(procs)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    struct Recorded {
        seen: RefCell<Vec<String>>,
        fail_processes: bool,
    }

    fn row(v: &[&str]) -> Vec<Option<String>> {
        v.iter()
            .map(|s| (!s.is_empty()).then(|| s.to_string()))
            .collect()
    }

    impl DictionarySource for Recorded {
        fn begin_read_only(&self) -> Result<(), PgError> {
            self.seen.borrow_mut().push("BEGIN".into());
            Ok(())
        }
        fn rows(&self, sql: &str) -> Result<Rows, PgError> {
            self.seen.borrow_mut().push(sql.to_string());
            if sql.contains("AD_Process") {
                if self.fail_processes {
                    return Err(PgError::Query {
                        sql: sql.into(),
                        message: "relation \"adempiere.ad_process\" does not exist".into(),
                    });
                }
                return Ok(vec![
                    // PostgreSQL's iDempiere keeps an empty string where Oracle has NULL.
                    row(&[
                        "C_Invoice_Generate",
                        "Generate Invoices",
                        "",
                        "org.compiere.process.InvoiceGenerate",
                        "Y",
                        "1",
                    ]),
                    row(&["AD_Synchronize", "Sync", "AD_Synchronize", "", "Y", "0"]),
                ]);
            }
            Ok(vec![
                row(&["C_Order", "Y", "Y", "N", "Y"]),
                row(&["AD_Org", "N", "Y", "N", "Y"]),
            ])
        }
        fn end_transaction(&self) -> Result<(), PgError> {
            self.seen.borrow_mut().push("END".into());
            Ok(())
        }
    }

    #[test]
    fn both_queries_run_in_one_read_only_snapshot_and_map_like_oracle() {
        let src = Recorded::default();
        let (t, p) = read_all(&src, DictionaryKind::Idempiere, "adempiere").unwrap();
        assert_eq!(t.len(), 2);
        assert_eq!(
            (t[0].table_name.as_str(), t[0].deleteable),
            ("C_Order", Some(true))
        );
        assert_eq!(t[1].deleteable, Some(false));
        assert_eq!(
            p[0].classname.as_deref(),
            Some("org.compiere.process.InvoiceGenerate")
        );
        assert_eq!((p[0].procedure_name.as_deref(), p[0].schedules), (None, 1));
        assert_eq!(
            (p[1].procedure_name.as_deref(), p[1].classname.as_deref()),
            (Some("AD_Synchronize"), None)
        );
        let seen = src.seen.borrow();
        assert_eq!((seen[0].as_str(), seen[3].as_str()), ("BEGIN", "END"));
        assert_eq!(
            seen[1],
            shared::tables_query(DictionaryKind::Idempiere, "adempiere").unwrap()
        );
        assert!(seen[2].contains("FROM \"adempiere\".AD_Process p"));
    }

    #[test]
    fn a_failed_read_still_ends_the_transaction() {
        let src = Recorded {
            fail_processes: true,
            ..Default::default()
        };
        assert!(read_all(&src, DictionaryKind::Idempiere, "adempiere").is_err());
        assert_eq!(src.seen.borrow().last().map(String::as_str), Some("END"));
    }

    #[test]
    fn an_unsafe_schema_is_refused_before_any_query() {
        let src = Recorded::default();
        assert!(read_all(&src, DictionaryKind::Idempiere, "x\" --").is_err());
        assert!(src.seen.borrow().is_empty());
    }
}
