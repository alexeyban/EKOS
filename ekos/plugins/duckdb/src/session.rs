//! The one way this crate talks to DuckDB (feature `duckdb`): a read-only connection whose
//! session is locked before the first statement, and every query under a deadline.

use crate::probe::SESSION_LOCKS;
use duckdb::{AccessMode, Config, Connection};
use ekos_sql_dialect_sdk::RowLimit;
use ekos_sql_dialect_sdk::guard::wrap_rows;
use ekos_sql_dialect_sdk::probe::Preview;
use std::path::Path;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

/// A locked, read-only session.
pub struct Session {
    conn: Connection,
    deadline: Duration,
}

impl Session {
    /// Opens an existing file read-only and locks the session.
    pub fn open(path: &Path, deadline: Duration) -> Result<Session, String> {
        if !path.is_file() {
            return Err(format!("{} is not a DuckDB file", path.display()));
        }
        let config = Config::default()
            .access_mode(AccessMode::ReadOnly)
            .and_then(|c| c.enable_external_access(false))
            .and_then(|c| c.enable_autoload_extension(false))
            .map_err(|e| e.to_string())?;
        let conn = Connection::open_with_flags(path, config)
            .map_err(|e| format!("cannot open {} read-only: {e}", path.display()))?;
        Session::lock(conn, deadline)
    }

    /// An empty in-memory database, locked the same way (tests and the conformance kit).
    pub fn open_in_memory(deadline: Duration) -> Result<Session, String> {
        let conn = Connection::open_in_memory().map_err(|e| e.to_string())?;
        Session::lock(conn, deadline)
    }

    fn lock(conn: Connection, deadline: Duration) -> Result<Session, String> {
        for s in SESSION_LOCKS {
            conn.execute_batch(s)
                .map_err(|e| format!("cannot lock the session ({s}): {e}"))?;
        }
        Ok(Session { conn, deadline })
    }

    /// Every row of `sql`, each cell as DuckDB renders it as text; `None` is NULL. Not guarded:
    /// callers send only their own catalog queries or text [`crate::guard::probe_guard`] passed.
    pub fn query_text(&self, sql: &str) -> Result<Preview, String> {
        self.rows(sql, usize::MAX)
    }

    /// At most `max_rows` rows of a guarded query.
    pub fn preview(&self, sql: &str, max_rows: usize) -> Result<Preview, String> {
        self.rows(&wrap_rows(sql, RowLimit::Limit, max_rows + 1), max_rows)
    }

    fn rows(&self, sql: &str, max_rows: usize) -> Result<Preview, String> {
        // DuckDB renders every type as text itself, so no Rust-side formatting can disagree with
        // what the engine prints.
        let text = format!("SELECT COLUMNS(*)::VARCHAR FROM ({sql}) ekos_text");
        self.with_deadline(|| {
            let mut stmt = self.conn.prepare(&text).map_err(|e| e.to_string())?;
            let mut rows = stmt.query([]).map_err(|e| e.to_string())?;
            let columns = rows.as_ref().map(|s| s.column_names()).unwrap_or_default();
            let mut out = Preview {
                columns,
                ..Preview::default()
            };
            while let Some(row) = rows.next().map_err(|e| e.to_string())? {
                if out.rows.len() == max_rows {
                    out.truncated = true;
                    break;
                }
                let cells = (0..out.columns.len())
                    .map(|i| row.get::<_, Option<String>>(i).map_err(|e| e.to_string()))
                    .collect::<Result<Vec<_>, _>>()?;
                out.rows.push(cells);
            }
            Ok(out)
        })
    }

    /// Runs `f`, interrupting the connection when the deadline passes first.
    fn with_deadline<T>(&self, f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
        let handle = self.conn.interrupt_handle();
        let (done, wait) = mpsc::channel::<()>();
        let deadline = self.deadline;
        let timer = std::thread::spawn(move || {
            let fired = wait.recv_timeout(deadline) == Err(RecvTimeoutError::Timeout);
            if fired {
                handle.interrupt();
            }
            fired
        });
        let out = f();
        drop(done);
        let fired = timer.join().unwrap_or(false);
        match out {
            Err(e) if fired => Err(format!(
                "interrupted at the {}s deadline: {e}",
                deadline.as_secs_f64()
            )),
            other => other,
        }
    }

    /// A statement sent with no guard — tests use it to show DuckDB itself refuses what the guard
    /// refuses first.
    #[cfg(test)]
    pub(crate) fn unguarded(&self, sql: &str) -> Result<(), String> {
        self.conn.execute_batch(sql).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::probe::{DEFAULT_DEADLINE, DuckDbProbe};
    use ekos_sql_dialect_sdk::probe::Probe;
    use std::time::Instant;

    /// A small mart file, written and closed before any probe opens it.
    fn mart(dir: &Path) -> std::path::PathBuf {
        let path = dir.join("marts.duckdb");
        let c = Connection::open(&path).unwrap();
        c.execute_batch(
            "CREATE SCHEMA gold;
             CREATE TABLE gold.fct_invoice (docstatus VARCHAR, n BIGINT, total DECIMAL(18, 2));
             INSERT INTO gold.fct_invoice VALUES ('CO', 3333, 20838167.00), ('DR', 10, NULL);
             CREATE VIEW gold.v_open AS SELECT * FROM gold.fct_invoice WHERE docstatus <> 'CO';
             CHECKPOINT;",
        )
        .unwrap();
        c.close().map_err(|(_, e)| e).unwrap();
        path
    }

    fn files(dir: &Path) -> Vec<(String, Vec<u8>)> {
        let mut out: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| {
                let e = e.unwrap();
                (
                    e.file_name().to_string_lossy().into_owned(),
                    std::fs::read(e.path()).unwrap(),
                )
            })
            .collect();
        out.sort();
        out
    }

    /// RFC 0259 §5: a probe answers mart queries, refuses `read_csv`, `ATTACH`, `SET` and `COPY`
    /// — in the guard and, sent unguarded, in DuckDB itself — and leaves the directory, the file
    /// included, byte-identical.
    #[test]
    fn a_probe_reads_refuses_and_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = mart(dir.path());
        let before = files(dir.path());
        let outside = dir.path().parent().unwrap().join(format!(
            "ekos-probe-{}",
            dir.path().file_name().unwrap().to_string_lossy()
        ));

        let probe = DuckDbProbe::open(&path, DEFAULT_DEADLINE).unwrap();
        let p = probe
            .preview(
                "SELECT docstatus, n, total FROM gold.fct_invoice ORDER BY docstatus",
                10,
            )
            .unwrap();
        assert_eq!(p.columns, ["docstatus", "n", "total"]);
        assert_eq!(
            p.rows,
            vec![
                vec![
                    Some("CO".into()),
                    Some("3333".into()),
                    Some("20838167.00".into())
                ],
                vec![Some("DR".into()), Some("10".into()), None],
            ]
        );
        let cut = probe.preview("SELECT * FROM range(5)", 2).unwrap();
        assert_eq!((cut.rows.len(), cut.truncated), (2, true));
        assert_eq!(
            probe
                .preview("SELECT count(*) FROM gold.v_open", 1)
                .unwrap()
                .rows,
            vec![vec![Some("1".into())]]
        );

        let other = format!("{}.duckdb", outside.display());
        let out = format!("{}.csv", outside.display());
        let refused = [
            "SELECT * FROM read_csv('/etc/passwd')".to_string(),
            "SELECT * FROM '/etc/passwd'".to_string(),
            format!("ATTACH '{other}' AS o"),
            "SET enable_external_access = true".to_string(),
            format!("COPY gold.fct_invoice TO '{out}'"),
            "INSERT INTO gold.fct_invoice VALUES ('XX', 1, 1)".to_string(),
            "CREATE TABLE gold.x AS SELECT 1".to_string(),
        ];
        for sql in &refused {
            let e = probe.preview(sql, 5).unwrap_err();
            assert!(e.starts_with("not sent"), "{sql}: {e}");
        }
        // The second lock: the session refuses the same statements on its own.
        let session = Session::open(&path, DEFAULT_DEADLINE).unwrap();
        for sql in &refused {
            assert!(session.unguarded(sql).is_err(), "DuckDB ran {sql}");
        }
        for sql in [
            "SET lock_configuration = false",
            "RESET enable_external_access",
            "INSTALL httpfs",
            "LOAD httpfs",
            "SELECT * FROM read_text('/etc/hostname')",
        ] {
            assert!(session.unguarded(sql).is_err(), "DuckDB ran {sql}");
        }
        drop(session);
        drop(probe);
        assert!(!Path::new(&out).exists());
        assert!(!Path::new(&other).exists());
        assert_eq!(files(dir.path()), before, "the directory changed");
    }

    /// RFC 0259 §5: a long query is interrupted at the deadline, and the session still answers.
    #[test]
    fn a_long_query_is_interrupted_at_the_deadline() {
        let s = Session::open_in_memory(Duration::from_millis(300)).unwrap();
        let start = Instant::now();
        let e = s
            .preview(
                "SELECT count(*) FROM range(1000000000) a, range(1000000) b",
                1,
            )
            .unwrap_err();
        let took = start.elapsed();
        assert!(e.starts_with("interrupted at the 0.3s deadline"), "{e}");
        assert!(took < Duration::from_secs(10), "took {took:?}");
        assert_eq!(
            s.preview("SELECT 42 AS x", 1).unwrap().rows,
            vec![vec![Some("42".into())]]
        );
    }

    #[test]
    fn a_missing_file_is_not_created() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("absent.duckdb");
        assert!(Session::open(&path, DEFAULT_DEADLINE).is_err());
        assert!(!path.exists());
    }
}
