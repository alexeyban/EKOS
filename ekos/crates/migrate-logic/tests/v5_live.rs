//! V5 against real engines, with planted defects.
//!
//! A check that never fails is not a check. Each test here lowers a view, confirms the lowering
//! passes V5 on PostgreSQL and ClickHouse, then damages the lowered statement the way a wrong
//! translation would — a moved boundary, a dropped filter, a different join, a missing setting —
//! and requires V5 to fail on it.
//!
//! Live, so off by default:
//! `docker compose -f docker-compose.migrate.yml up -d`, then
//! `EKOS_MIGRATE_LIVE=1 cargo test -p ekos-migrate-logic --test v5_live`.

use ekos_migrate_logic::lower::{LowerOptions, Lowering, lower_view};
use ekos_migrate_logic::v5::{Sandbox, TableSpec, V5Outcome, V5Report, plan, run};
use ekos_migrate_validate::Dialect;
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

fn live() -> bool {
    std::env::var("EKOS_MIGRATE_LIVE").as_deref() == Ok("1")
}

const MARK: &str = "--ekos-v5-set--";

/// A UTF8 database, created once. The compose sandbox's default database is `SQL_ASCII` (the
/// image runs with `LANG=C`), where PostgreSQL counts bytes and V5 refuses to run.
const DATABASE: &str = "ekos_v5_utf8";

fn ensure_database() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let _ = Command::new("psql")
            .args([
                "-h",
                "localhost",
                "-p",
                "55432",
                "-U",
                "ekos",
                "-d",
                "ekos_migrate_fixtures",
                "-X",
                "-q",
                "-c",
                &format!("CREATE DATABASE {DATABASE} ENCODING 'UTF8' TEMPLATE template0"),
            ])
            .env("PGPASSWORD", "ekos-local-only")
            .output();
    });
}

/// PostgreSQL through `psql`: one transaction in a schema of its own, always rolled back.
struct Pg;

impl Sandbox for Pg {
    fn dialect(&self) -> Dialect {
        Dialect::Postgres
    }
    fn label(&self) -> &str {
        "psql:sandbox"
    }
    fn run(&self, setup: &[String], queries: &[String]) -> Result<Vec<Vec<Vec<String>>>, String> {
        ensure_database();
        let mut script =
            String::from("BEGIN;\nCREATE SCHEMA ekos_v5;\nSET LOCAL search_path TO ekos_v5;\n");
        for s in setup {
            script.push_str(s);
            script.push_str(";\n");
        }
        for q in queries {
            script.push_str(&format!("SELECT '{MARK}';\n{q};\n"));
        }
        script.push_str("ROLLBACK;\n");
        // On stdin, not `-c`: with `-c` psql prints only the last statement's result.
        let mut child = Command::new("psql")
            .args([
                "-h",
                "localhost",
                "-p",
                "55432",
                "-U",
                "ekos",
                "-d",
                DATABASE,
                "-X",
                "-q",
                "-tA",
                "-F",
                "\t",
                "-v",
                "ON_ERROR_STOP=1",
                "-f",
                "-",
            ])
            .env("PGPASSWORD", "ekos-local-only")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| e.to_string())?;
        child
            .stdin
            .take()
            .expect("piped")
            .write_all(script.as_bytes())
            .map_err(|e| e.to_string())?;
        let out = child.wait_with_output().map_err(|e| e.to_string())?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).to_string());
        }
        let text = String::from_utf8_lossy(&out.stdout).to_string();
        Ok(text
            .split(MARK)
            .skip(1)
            .map(|set| {
                set.lines()
                    .filter(|l| !l.is_empty())
                    .map(|l| l.split('\t').map(str::to_string).collect())
                    .collect()
            })
            .collect())
    }
}

/// ClickHouse over HTTP: a database of its own, always dropped.
struct Ch;

static SEQ: AtomicUsize = AtomicUsize::new(0);

fn ch(database: Option<&str>, sql: &str) -> Result<String, String> {
    let mut url =
        "http://localhost:58123/?user=ekos&password=ekos-local-only&format_tsv_null_representation="
            .to_string();
    if let Some(db) = database {
        url.push_str(&format!("&database={db}"));
    }
    let out = Command::new("curl")
        .args(["-s", "--fail-with-body", &url, "--data-binary", sql])
        .output()
        .map_err(|e| e.to_string())?;
    let body = String::from_utf8_lossy(&out.stdout).to_string();
    if out.status.success() {
        Ok(body)
    } else {
        Err(body)
    }
}

impl Sandbox for Ch {
    fn dialect(&self) -> Dialect {
        Dialect::ClickHouse
    }
    fn label(&self) -> &str {
        "clickhouse:sandbox"
    }
    fn run(&self, setup: &[String], queries: &[String]) -> Result<Vec<Vec<Vec<String>>>, String> {
        let db = format!(
            "ekos_v5_{}_{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::SeqCst)
        );
        ch(None, &format!("CREATE DATABASE {db}"))?;
        let result = (|| {
            for s in setup {
                ch(Some(&db), s)?;
            }
            queries
                .iter()
                .map(|q| {
                    let body = ch(Some(&db), &format!("{q} FORMAT TabSeparatedRaw"))?;
                    Ok(body
                        .lines()
                        .filter(|l| !l.is_empty())
                        .map(|l| l.split('\t').map(str::to_string).collect())
                        .collect())
                })
                .collect::<Result<Vec<_>, String>>()
        })();
        let _ = ch(None, &format!("DROP DATABASE IF EXISTS {db}"));
        result
    }
}

fn tables() -> Vec<TableSpec> {
    let t = |name: &str, cols: &[(&str, &str)]| TableSpec {
        name: name.into(),
        columns: cols
            .iter()
            .map(|(c, t)| (c.to_string(), t.to_string()))
            .collect(),
    };
    vec![
        t(
            "invoice",
            &[
                ("id", "int"),
                ("due", "int"),
                ("status", "text"),
                ("note", "text"),
            ],
        ),
        t(
            "line",
            &[("invoice_id", "int"), ("amount", "numeric(12,2)")],
        ),
    ]
}

fn lowered(source: &str) -> String {
    match lower_view(source, &LowerOptions::default()) {
        Lowering::Lowered(l) => l.sql,
        Lowering::Blocked { blockers } => panic!("{blockers:#?}"),
    }
}

fn v5(source: &str, candidate: &str) -> V5Report {
    let plan = plan(source, candidate, &tables()).unwrap_or_else(|o| panic!("{o:?}"));
    match run(&plan, &Pg, &Ch) {
        V5Outcome::Ran(report) => *report,
        other => panic!("{other:#?}"),
    }
}

/// The lowering passes; each `(from, to)` edit of it must fail.
fn planted(source: &str, defects: &[(&str, &str)]) {
    let good = lowered(source);
    let clean = v5(source, &good);
    assert!(clean.passed, "the faithful lowering failed V5: {clean:#?}");
    assert!(
        clean.source_rows > 0,
        "an empty result proves nothing: {clean:#?}"
    );
    assert_eq!(
        clean.coverage.covered, clean.coverage.measured,
        "the fixture left a comparison untaken: {:?}",
        clean.coverage.uncovered
    );
    for (from, to) in defects {
        assert!(good.contains(from), "the lowering has no {from:?}: {good}");
        let bad = good.replacen(from, to, 1);
        let report = v5(source, &bad);
        assert!(
            !report.passed,
            "V5 missed {from:?} -> {to:?}: {}",
            report.verdict()
        );
    }
}

#[test]
fn planted_defects_in_filters_and_joins_are_caught() {
    if !live() {
        eprintln!("skipped: set EKOS_MIGRATE_LIVE=1");
        return;
    }
    planted(
        "CREATE VIEW overdue AS SELECT i.id, i.status, sum(l.amount) AS total, \
         count(l.invoice_id) AS n FROM invoice i LEFT JOIN line l ON l.invoice_id = i.id \
         WHERE i.due < 30 AND i.status IN ('open', 'sent') GROUP BY i.id, i.status",
        &[
            ("(`i`.`due` < 30)", "(`i`.`due` <= 30)"),
            ("(`i`.`due` < 30)", "(`i`.`due` < 31)"),
            ("(`i`.`due` < 30)", "(`i`.`due` > 30)"),
            ("((`i`.`due` < 30) AND ", "("),
            ("IN ('open', 'sent')", "IN ('open')"),
            ("LEFT OUTER JOIN", "INNER JOIN"),
            ("sumOrNull(`l`.`amount`)", "maxOrNull(`l`.`amount`)"),
        ],
    );
}

#[test]
fn a_missing_alias_setting_and_a_byte_count_are_caught() {
    if !live() {
        eprintln!("skipped: set EKOS_MIGRATE_LIVE=1");
        return;
    }
    // The alias `due` shadows the column in WHERE unless the setting says otherwise.
    planted(
        "CREATE VIEW shifted AS SELECT id, due + 100 AS due FROM invoice WHERE due < 30",
        &[(" SETTINGS prefer_column_name_to_alias = 1", "")],
    );
    // `length` counts bytes in ClickHouse; the fixture holds one non-ASCII value.
    planted(
        "CREATE VIEW sized AS SELECT id, length(note) AS n FROM invoice WHERE status = 'open'",
        &[("char_length(`note`)", "length(`note`)")],
    );
}

#[test]
fn nothing_is_left_behind_on_either_engine() {
    if !live() {
        eprintln!("skipped: set EKOS_MIGRATE_LIVE=1");
        return;
    }
    let source = "CREATE VIEW plain AS SELECT id FROM invoice WHERE due >= 0";
    assert!(v5(source, &lowered(source)).passed);
    // Asked outside the sandbox path, which would create the schema it is looking for.
    let out = Command::new("psql")
        .args([
            "-h",
            "localhost",
            "-p",
            "55432",
            "-U",
            "ekos",
            "-d",
            DATABASE,
            "-X",
            "-tA",
            "-c",
            "SELECT count(*) FROM pg_namespace WHERE nspname = 'ekos_v5'",
        ])
        .env("PGPASSWORD", "ekos-local-only")
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "0");
    // The other tests run beside this one and hold a database each while they do, so wait for
    // them rather than racing them: every run must end with none.
    let mut left = String::new();
    for _ in 0..40 {
        left = ch(
            None,
            "SELECT count() FROM system.databases WHERE name LIKE 'ekos_v5_%'",
        )
        .unwrap();
        if left.trim() == "0" {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    assert_eq!(left.trim(), "0");
}
