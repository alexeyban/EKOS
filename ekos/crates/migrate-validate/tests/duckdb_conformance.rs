//! RFC 0259 D3 — DuckDB's canonical form against a real DuckDB, through its CLI.
//!
//! Live: `EKOS_DUCKDB_CLI=<path to the duckdb binary>` (1.5.x). Every query runs in an in-memory
//! database; nothing is written. The in-process twin of this suite is in `ekos-plugin-duckdb`
//! (feature `duckdb`).

use ekos_migrate_validate::conformance;
use ekos_migrate_validate::dialect::{ColumnRule, bucket_checksum_query, canon_expr};
use ekos_migrate_validate::duckdb::{DuckDbLiterals, dialect};
use ekos_migrate_validate::{Dialect, EngineReader, ReadError};
use std::process::Command;

/// The DuckDB CLI, fields and records split by the ASCII separators.
struct DuckDbCli {
    bin: String,
    /// Statements run before the query in the same session (`SET TimeZone = …`).
    prelude: String,
}

impl EngineReader for DuckDbCli {
    fn dialect(&self) -> Dialect {
        dialect()
    }
    fn label(&self) -> &str {
        "duckdb-cli::memory:"
    }
    fn query(&self, sql: &str) -> Result<Vec<Vec<String>>, ReadError> {
        let err = |message: String| ReadError::Query {
            engine: "duckdb".into(),
            message,
        };
        let out = Command::new(&self.bin)
            .args([
                "-noheader",
                "-list",
                "-separator",
                "\u{1e}",
                "-newline",
                "\u{1d}",
                "-c",
                &format!("{}{sql}", self.prelude),
            ])
            .output()
            .map_err(|e| err(e.to_string()))?;
        if !out.status.success() {
            return Err(err(String::from_utf8_lossy(&out.stderr).into_owned()));
        }
        let text = String::from_utf8_lossy(&out.stdout);
        let text = text.strip_suffix('\u{1d}').unwrap_or(&text);
        Ok(text
            .split('\u{1d}')
            .map(|r| r.split('\u{1e}').map(str::to_string).collect())
            .collect())
    }
}

fn cli() -> Option<DuckDbCli> {
    match std::env::var("EKOS_DUCKDB_CLI") {
        Ok(bin) => Some(DuckDbCli {
            bin,
            prelude: String::new(),
        }),
        Err(_) => {
            eprintln!("skipped: set EKOS_DUCKDB_CLI to a duckdb 1.5.x binary");
            None
        }
    }
}

#[test]
fn the_kit_passes_on_duckdb() {
    let Some(r) = cli() else { return };
    let report = conformance::run(&r, &DuckDbLiterals);
    eprintln!(
        "RFC 0255 conformance {}: {} cases, {} hashes, buckets {}, passed {}, id {}",
        report.dialect,
        report.cases_checked,
        report.hashes_checked,
        report.buckets_checked,
        report.passed(),
        report.id
    );
    assert!(report.passed(), "{report:#?}");
    assert_eq!(report.cases_checked, 31);
    assert_eq!(report.hashes_checked, 4);
}

/// The session's time zone never reaches the canonical form of a `TIMESTAMPTZ`.
#[test]
fn the_session_time_zone_does_not_change_a_timestamp() {
    let Some(mut r) = cli() else { return };
    let lit = "TIMESTAMPTZ '2026-09-24 12:30:00.123456+00:00'";
    let sql = format!(
        "SELECT {}",
        ekos_migrate_validate::canon_expr_of(dialect(), lit, ColumnRule::TimestampUtc)
    );
    for zone in ["UTC", "America/New_York", "Asia/Kolkata"] {
        r.prelude = format!("SET TimeZone = '{zone}'; ");
        assert_eq!(
            r.one_row(&sql).unwrap(),
            ["2026-09-24T12:30:00.123456"],
            "{zone}"
        );
    }
}

/// `n` invoice rows `(id, total)`, totals in cents; `planted` adds one cent to one row.
fn invoices(n: i64, planted: Option<i64>) -> String {
    let rows: Vec<String> = (1..=n)
        .map(|i| {
            let cents = i * 137 + i64::from(Some(i) == planted);
            format!("({i}, {}.{:02})", cents / 100, cents % 100)
        })
        .collect();
    format!(
        "(SELECT CAST(id AS BIGINT) AS id, CAST(total AS DECIMAL(18, 2)) AS total \
         FROM (VALUES {}) v(id, total)) t",
        rows.join(", ")
    )
}

/// RFC 0259 §5: a mart whose one row is off by one cent fails the bucket checksum against the
/// canonical rows of the clean data, in exactly the bucket holding that row; the clean mart passes.
#[test]
fn a_planted_one_cent_difference_is_found() {
    let Some(r) = cli() else { return };
    let d = dialect();
    let n = 500;
    let pk = canon_expr(d, "id", ColumnRule::Int);
    let cols = vec![pk.clone(), canon_expr(d, "total", ColumnRule::Decimal(2))];
    let canon: Vec<(String, String)> = (1..=n)
        .map(|i| {
            let cents = i * 137;
            (
                i.to_string(),
                format!("{i}\u{1f}{}.{:02}", cents / 100, cents % 100),
            )
        })
        .collect();
    let buckets = 64;
    let want: Vec<Vec<String>> = ekos_migrate_validate::checksum_buckets(
        canon.iter().map(|(k, v)| (k.as_str(), v.as_str())),
        buckets,
    )
    .iter()
    .map(|(b, c)| vec![b.to_string(), c.count.to_string(), c.sum.to_string()])
    .collect();

    let clean = r
        .query(&bucket_checksum_query(
            d,
            &invoices(n, None),
            &pk,
            &cols,
            buckets,
        ))
        .unwrap();
    assert_eq!(clean, want, "the clean mart agrees");

    let planted_id = 237;
    let planted = r
        .query(&bucket_checksum_query(
            d,
            &invoices(n, Some(planted_id)),
            &pk,
            &cols,
            buckets,
        ))
        .unwrap();
    let differing: Vec<&Vec<String>> = planted.iter().filter(|row| !want.contains(row)).collect();
    assert_eq!(differing.len(), 1, "{differing:?}");
    let bucket = ekos_migrate_validate::bucket_of(&planted_id.to_string(), buckets);
    assert_eq!(differing[0][0], bucket.to_string());
    assert_eq!(
        differing[0][1],
        want[differing[0][0].parse::<usize>().unwrap()][1]
    );
}
