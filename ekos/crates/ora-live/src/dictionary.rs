//! RFC 0212 §3.3 — the application dictionary's flags about tables, opt-in.
//!
//! iDempiere-family applications decide in their dictionary, not in code, whether users may delete
//! a table's rows (`AD_Table.IsDeleteable`, checked by the UI) and whether deletes are recorded in
//! `AD_ChangeLog` (`IsChangeLog`, checked by `PO.delete`). This reads exactly those flags — one
//! query naming its table and columns, inside a read-only transaction — and nothing else from the
//! dictionary. The flags describe tables; they are not business data.

use crate::OraError;
use crate::catalog::Rows;
use crate::profile::ScanSource;
use crate::session::guard_identifier;
use serde::{Deserialize, Serialize};

/// Which dictionary to read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DictionaryKind {
    /// iDempiere / ADempiere / Compiere: `AD_Table`.
    Idempiere,
}

/// One table's flags. `None` when the column held something other than `Y` / `N`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableFlags {
    /// As the dictionary writes it (`C_Order`).
    pub table_name: String,
    pub deleteable: Option<bool>,
    pub change_log: Option<bool>,
    pub view: Option<bool>,
    pub active: Option<bool>,
}

fn yes_no(v: Option<&str>) -> Option<bool> {
    match v.map(str::trim) {
        Some("Y") => Some(true),
        Some("N") => Some(false),
        _ => None,
    }
}

/// The query for `owner`'s dictionary. The owner is a checked identifier, so it is quoted into the
/// text: scans take no binds.
pub fn query(kind: DictionaryKind, owner: &str) -> Result<String, OraError> {
    let o = guard_identifier(owner)?;
    Ok(match kind {
        DictionaryKind::Idempiere => format!(
            "SELECT TableName, IsDeleteable, IsChangeLog, IsView, IsActive \
             FROM \"{o}\".AD_Table ORDER BY TableName"
        ),
    })
}

fn flags(rows: Rows) -> Vec<TableFlags> {
    rows.into_iter()
        .filter_map(|r| {
            let cell = |i: usize| r.get(i).and_then(|c| c.as_deref());
            Some(TableFlags {
                table_name: cell(0)?.to_string(),
                deleteable: yes_no(cell(1)),
                change_log: yes_no(cell(2)),
                view: yes_no(cell(3)),
                active: yes_no(cell(4)),
            })
        })
        .collect()
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
    }
}
