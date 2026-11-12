//! RFC 0212 §3.3 — the application dictionary's flags about tables, opt-in.
//!
//! iDempiere-family applications decide in their dictionary, not in code, whether users may delete
//! a table's rows (`AD_Table.IsDeleteable`, checked by the UI) and whether deletes are recorded in
//! `AD_ChangeLog` (`IsChangeLog`, checked by `PO.delete`). This reads exactly those flags — one
//! query naming its table and columns, inside a read-only transaction — and nothing else from the
//! dictionary. The flags describe tables; they are not business data.
//!
//! RFC 0256 open question 4: the same dictionary names the application's processes
//! (`AD_Process`) — the database procedure or Java class each one runs — and which are scheduled
//! (`AD_Scheduler`). [`read_all`] reads those too, in the same read-only transaction: a process is
//! a fifth kind of actor in the interaction map. Names and class names describe the application,
//! not its data.

use crate::OraError;
use crate::catalog::Rows;
use crate::profile::ScanSource;
use crate::session::guard_identifier;
use ekos_observation_sdk::app_dictionary as shared;

/// The types are shared with every engine that hosts the dictionary (RFC 0256: PostgreSQL too).
pub use shared::{AppProcess, DictionaryKind, TableFlags};

/// The query for `owner`'s dictionary. The owner is a checked identifier, so it is quoted into the
/// text: scans take no binds.
pub fn query(kind: DictionaryKind, owner: &str) -> Result<String, OraError> {
    let o = guard_identifier(owner)?;
    shared::tables_query(kind, o).map_err(|_| OraError::UnsafeIdentifier(o.to_string()))
}

fn flags(rows: Rows) -> Vec<TableFlags> {
    shared::table_flags(rows)
}

/// The query for `owner`'s processes: only those that run a procedure or a class.
pub fn processes_query(kind: DictionaryKind, owner: &str) -> Result<String, OraError> {
    let o = guard_identifier(owner)?;
    shared::processes_query(kind, o).map_err(|_| OraError::UnsafeIdentifier(o.to_string()))
}

fn processes(rows: Rows) -> Vec<AppProcess> {
    shared::processes(rows)
}

/// The table flags and the processes, in one read-only transaction.
pub fn read_all(
    source: &dyn ScanSource,
    kind: DictionaryKind,
    owner: &str,
) -> Result<(Vec<TableFlags>, Vec<AppProcess>), OraError> {
    let tables_sql = query(kind, owner)?;
    let processes_sql = processes_query(kind, owner)?;
    source.begin_read_only()?;
    let tables = source.scan(&tables_sql);
    let procs = tables.is_ok().then(|| source.scan(&processes_sql));
    let ended = source.end_transaction();
    let tables = tables?;
    let procs = procs.transpose()?.unwrap_or_default();
    ended?;
    Ok((flags(tables), processes(procs)))
}

/// Read the dictionary's table flags in one read-only transaction.
pub fn read(
    source: &dyn ScanSource,
    kind: DictionaryKind,
    owner: &str,
) -> Result<Vec<TableFlags>, OraError> {
    let sql = query(kind, owner)?;
    source.begin_read_only()?;
    let rows = source.scan(&sql);
    let ended = source.end_transaction();
    let rows = rows?;
    ended?;
    Ok(flags(rows))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    struct Recorded(Rows, RefCell<Vec<String>>);

    impl ScanSource for Recorded {
        fn scan(&self, sql: &str) -> Result<Rows, OraError> {
            self.1.borrow_mut().push(sql.to_string());
            Ok(self.0.clone())
        }
        fn begin_read_only(&self) -> Result<(), OraError> {
            self.1.borrow_mut().push("BEGIN".into());
            Ok(())
        }
        fn end_transaction(&self) -> Result<(), OraError> {
            self.1.borrow_mut().push("END".into());
            Ok(())
        }
    }

    fn row(v: &[&str]) -> Vec<Option<String>> {
        v.iter()
            .map(|s| (!s.is_empty()).then(|| s.to_string()))
            .collect()
    }

    #[test]
    fn flags_are_read_in_one_read_only_query_naming_its_columns() {
        let src = Recorded(
            vec![
                row(&["C_Order", "Y", "Y", "N", "Y"]),
                row(&["AD_Org", "N", "Y", "N", "Y"]),
                row(&["Odd", "?", "", "N", "Y"]),
            ],
            RefCell::default(),
        );
        let f = read(&src, DictionaryKind::Idempiere, "ADEMPIERE").unwrap();
        assert_eq!(f[0].deleteable, Some(true));
        assert_eq!(f[1].deleteable, Some(false));
        assert_eq!((f[2].deleteable, f[2].change_log), (None, None));
        let seen = src.1.borrow();
        assert_eq!(seen[0], "BEGIN");
        assert!(seen[1].starts_with(
            "SELECT TableName, IsDeleteable, IsChangeLog, IsView, IsActive FROM \"ADEMPIERE\".AD_Table"
        ));
        assert_eq!(seen[2], "END");
    }

    #[test]
    fn an_unsafe_owner_is_refused_before_any_query() {
        assert!(query(DictionaryKind::Idempiere, "X\" --").is_err());
        assert!(processes_query(DictionaryKind::Idempiere, "X\" --").is_err());
    }

    #[test]
    fn processes_are_read_with_their_schedules_in_the_same_transaction() {
        struct Two(RefCell<Vec<String>>);
        impl ScanSource for Two {
            fn scan(&self, sql: &str) -> Result<Rows, OraError> {
                self.0.borrow_mut().push(sql.to_string());
                Ok(if sql.contains("AD_Process") {
                    vec![
                        row(&[
                            "C_Invoice_Generate",
                            "Generate Invoices",
                            "",
                            "org.compiere.process.InvoiceGenerate",
                            "Y",
                            "1",
                        ]),
                        row(&["AD_Synchronize", "Sync", "AD_Synchronize", "", "Y", "0"]),
                    ]
                } else {
                    vec![row(&["C_Order", "Y", "Y", "N", "Y"])]
                })
            }
            fn begin_read_only(&self) -> Result<(), OraError> {
                self.0.borrow_mut().push("BEGIN".into());
                Ok(())
            }
            fn end_transaction(&self) -> Result<(), OraError> {
                self.0.borrow_mut().push("END".into());
                Ok(())
            }
        }
        let src = Two(RefCell::default());
        let (t, p) = read_all(&src, DictionaryKind::Idempiere, "ADEMPIERE").unwrap();
        assert_eq!(t.len(), 1);
        assert_eq!(
            p[0].classname.as_deref(),
            Some("org.compiere.process.InvoiceGenerate")
        );
        assert_eq!((p[0].procedure_name.as_deref(), p[0].schedules), (None, 1));
        assert_eq!(p[1].procedure_name.as_deref(), Some("AD_Synchronize"));
        let seen = src.0.borrow();
        assert_eq!((seen[0].as_str(), seen[3].as_str()), ("BEGIN", "END"));
        assert!(seen[2].contains("FROM \"ADEMPIERE\".AD_Process p"));
    }
}
