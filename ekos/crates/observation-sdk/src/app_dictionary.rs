//! RFC 0212 §3.3 / RFC 0256 open question 4 — an application dictionary read from a live
//! database, the same for every engine that hosts one.
//!
//! iDempiere-family applications (iDempiere, ADempiere, Compiere) keep their dictionary in their
//! own tables, on Oracle or on PostgreSQL. Two things are read from it and nothing else:
//!
//! - `AD_Table`'s flags about each table: whether users may delete its rows (`IsDeleteable`),
//!   whether deletes are recorded in `AD_ChangeLog` (`IsChangeLog`), view, active;
//! - `AD_Process`: each process that runs a database procedure or a Java class, with how many
//!   active `AD_Scheduler` entries run it.
//!
//! Names, flags and class names describe the application, not its data. The queries, the row
//! mapping and the artifact are here so the Oracle and PostgreSQL connectors read and emit exactly
//! the same thing; each connector runs the two queries in one read-only transaction of its own.

use crate::live_catalog::catalog_target;
use ekos_artifact::ObservationArtifact;
use serde::{Deserialize, Serialize};

/// Which dictionary to read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DictionaryKind {
    /// iDempiere / ADempiere / Compiere: `AD_Table`, `AD_Process`, `AD_Scheduler`.
    Idempiere,
}

/// Rows as a connector returns them: every value as text, `None` for NULL.
pub type Rows = Vec<Vec<Option<String>>>;

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

/// One application process: what it runs, and how many active schedules run it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppProcess {
    /// The process's search key (`C_Invoice_Generate`), unique per client in the dictionary.
    pub value: String,
    pub name: String,
    /// The database procedure it calls, when it is one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub procedure_name: Option<String>,
    /// The Java class it runs, when it is one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub classname: Option<String>,
    pub active: Option<bool>,
    /// Active `AD_Scheduler` entries that run it.
    pub schedules: u32,
}

/// A schema / owner name that is safe to quote into a query: scans take no binds. Letters,
/// digits, `_`, `$`, `#`, at most 128 bytes.
pub fn is_safe_identifier(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'$' | b'#'))
}

fn guarded(owner: &str) -> Result<&str, String> {
    if is_safe_identifier(owner) {
        Ok(owner)
    } else {
        Err(format!("{owner:?} is not a plain identifier"))
    }
}

/// The table-flags query for `owner`'s dictionary. The owner is quoted exactly as given (Oracle's
/// `ADEMPIERE`, PostgreSQL's `adempiere`); every other identifier is unquoted, so each engine
/// folds it to its own case and the same text runs on both.
pub fn tables_query(kind: DictionaryKind, owner: &str) -> Result<String, String> {
    let o = guarded(owner)?;
    Ok(match kind {
        DictionaryKind::Idempiere => format!(
            "SELECT TableName, IsDeleteable, IsChangeLog, IsView, IsActive \
             FROM \"{o}\".AD_Table ORDER BY TableName"
        ),
    })
}

/// The processes query for `owner`'s dictionary: only processes that run a procedure or a class.
pub fn processes_query(kind: DictionaryKind, owner: &str) -> Result<String, String> {
    let o = guarded(owner)?;
    Ok(match kind {
        DictionaryKind::Idempiere => format!(
            "SELECT p.Value, p.Name, p.ProcedureName, p.Classname, p.IsActive, \
             (SELECT COUNT(*) FROM \"{o}\".AD_Scheduler s \
              WHERE s.AD_Process_ID = p.AD_Process_ID AND s.IsActive = 'Y') \
             FROM \"{o}\".AD_Process p \
             WHERE p.ProcedureName IS NOT NULL OR p.Classname IS NOT NULL ORDER BY p.Value"
        ),
    })
}

fn yes_no(v: Option<&str>) -> Option<bool> {
    match v.map(str::trim) {
        Some("Y") => Some(true),
        Some("N") => Some(false),
        _ => None,
    }
}

/// The rows of [`tables_query`]. A row without a table name is skipped.
pub fn table_flags(rows: Rows) -> Vec<TableFlags> {
    rows.into_iter()
        .filter_map(|r| {
            let cell = |i: usize| r.get(i).and_then(Option::as_deref);
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

/// The rows of [`processes_query`]. Blank procedure and class names are absent; PostgreSQL's
/// empty string and Oracle's NULL read the same.
pub fn processes(rows: Rows) -> Vec<AppProcess> {
    let text = |v: Option<&str>| v.map(str::trim).filter(|s| !s.is_empty()).map(String::from);
    rows.into_iter()
        .filter_map(|r| {
            let cell = |i: usize| r.get(i).and_then(Option::as_deref);
            Some(AppProcess {
                value: cell(0)?.to_string(),
                name: cell(1).unwrap_or_default().to_string(),
                procedure_name: text(cell(2)),
                classname: text(cell(3)),
                active: yes_no(cell(4)),
                schedules: cell(5).and_then(|n| n.trim().parse().ok()).unwrap_or(0),
            })
        })
        .collect()
}

/// A process's name is the application's own text: redacted like any other before it is kept.
pub fn redact_names(
    processes: &mut [AppProcess],
    redaction: &ekos_common::redaction::RedactionConfig,
) {
    for p in processes {
        p.name = ekos_common::redaction::redact(&p.name, redaction);
    }
}

/// The target of one owner's dictionary artifact: `<scheme>://<database>/<owner>/dictionary`.
pub fn target(scheme: &str, database: &str, owner: &str) -> String {
    format!("{}/dictionary", catalog_target(scheme, database, owner))
}

/// One owner's dictionary as an artifact, the shape the catalog pass reads for every engine.
pub fn artifact(
    observer: &str,
    producer: &str,
    scheme: &str,
    database: &str,
    owner: &str,
    tables: &[TableFlags],
    processes: &[AppProcess],
) -> ObservationArtifact {
    let data = serde_json::json!({
        "kind": "dictionary",
        "database": database,
        "owner": owner,
        "tables": tables,
        "processes": processes,
    });
    ObservationArtifact::new(observer, target(scheme, database, owner), data)
        .with_producer(producer)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(v: &[&str]) -> Vec<Option<String>> {
        v.iter()
            .map(|s| (!s.is_empty()).then(|| s.to_string()))
            .collect()
    }

    #[test]
    fn the_queries_quote_the_owner_alone_and_refuse_an_unsafe_one() {
        let t = tables_query(DictionaryKind::Idempiere, "adempiere").unwrap();
        assert_eq!(
            t,
            "SELECT TableName, IsDeleteable, IsChangeLog, IsView, IsActive \
             FROM \"adempiere\".AD_Table ORDER BY TableName"
        );
        let p = processes_query(DictionaryKind::Idempiere, "ADEMPIERE").unwrap();
        assert!(p.contains("FROM \"ADEMPIERE\".AD_Process p"));
        assert!(p.contains("FROM \"ADEMPIERE\".AD_Scheduler s"));
        for bad in ["", "x\" --", "a b", "a;b", &"x".repeat(129)] {
            assert!(
                tables_query(DictionaryKind::Idempiere, bad).is_err(),
                "{bad}"
            );
            assert!(
                processes_query(DictionaryKind::Idempiere, bad).is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn rows_map_to_flags_and_processes() {
        let f = table_flags(vec![
            row(&["C_Order", "Y", "Y", "N", "Y"]),
            row(&["Odd", "?", "", "N", "Y"]),
            row(&[]),
        ]);
        assert_eq!(f.len(), 2);
        assert_eq!((f[0].deleteable, f[0].change_log), (Some(true), Some(true)));
        assert_eq!((f[1].deleteable, f[1].change_log), (None, None));
        let p = processes(vec![
            row(&[
                "C_Invoice_Generate",
                "Generate Invoices",
                "",
                "org.x.InvoiceGenerate",
                "Y",
                "2",
            ]),
            vec![
                Some("AD_Sync".into()),
                Some("Sync".into()),
                Some("AD_Sync".into()),
                Some("  ".into()),
                Some("N".into()),
                Some("0".into()),
            ],
        ]);
        assert_eq!(p[0].procedure_name, None);
        assert_eq!(p[0].schedules, 2);
        assert_eq!(p[1].classname, None, "a blank class name is absent");
        assert_eq!(p[1].active, Some(false));
    }

    #[test]
    fn the_artifact_names_its_owner_and_carries_both_lists() {
        let a = artifact(
            "postgres",
            "ekos-plugin-postgres",
            "postgres",
            "erp",
            "adempiere",
            &table_flags(vec![row(&["C_Order", "Y", "N", "N", "Y"])]),
            &[],
        );
        assert_eq!(a.content.target, "postgres://erp/adempiere/dictionary");
        let data = &a.content.data;
        assert_eq!(data["kind"], "dictionary");
        assert_eq!(data["owner"], "adempiere");
        assert_eq!(data["tables"][0]["table_name"], "C_Order");
        assert_eq!(data["processes"], serde_json::json!([]));
    }
}
