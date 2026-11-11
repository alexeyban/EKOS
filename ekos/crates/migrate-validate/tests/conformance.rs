//! RFC 0255 §3.4 — the conformance kit, offline and live.
//!
//! Offline: a registered (out-of-tree) dialect is reached through every free function of
//! `dialect`, and the built-in dialects' SQL is unchanged (pinned by `sql_snapshot.rs`).
//!
//! Live (`EKOS_MIGRATE_LIVE=1` with `docker-compose.migrate.yml` up): the kit passes on
//! PostgreSQL and ClickHouse with the same readers `live_engines.rs` uses. Trino and Oracle run it
//! in the `ekos-lakehouse` repository, which has their clients.

use ekos_migrate_validate::conformance::{self, Literals};
use ekos_migrate_validate::dialect::{
    CanonDialect, ColumnRule, Dialect, aggregate_query, bucket_checksum_query, canon_expr,
    column_digest_expr, count_query, row_hash_expr, sub_bucket_checksum_query,
};
use ekos_migrate_validate::{EngineReader, ReadError};
use std::process::Command;

/// A PostgreSQL-flavoured dialect written as an out-of-tree engine would write it.
struct Portable;

impl CanonDialect for Portable {
    fn name(&self) -> &str {
        "portable"
    }
    fn canon_body(&self, expr: &str, rule: ColumnRule) -> String {
        match rule {
            ColumnRule::Int => format!("CAST({expr} AS VARCHAR(40))"),
            _ => format!(
                "REPLACE(REPLACE(CAST({expr} AS VARCHAR(4000)), '\\', '\\\\'), CHR(31), '\\x1f')"
            ),
        }
    }
    fn hash_hex_expr(&self, text: &str) -> String {
        format!("LOWER(HEX(SHA256({text})))")
    }
    fn prefix60_expr(&self, hash: &str) -> String {
        format!("HEX_TO_INT(SUBSTR({hash}, 1, 15))")
    }
    fn digest_columns(&self) -> bool {
        true
    }
}

static PORTABLE: Portable = Portable;

#[test]
fn a_registered_dialect_is_reached_through_every_function() {
    let d = Dialect::Registered(&PORTABLE);
    assert_eq!(d.name(), "portable");
    assert!(d.digest_columns());
    assert_eq!(d, Dialect::Registered(&PORTABLE));
    assert_ne!(d, Dialect::Postgres);
    assert_eq!(format!("{d:?}"), "Registered(\"portable\")");
    assert_eq!(
        canon_expr(d, "c", ColumnRule::Int),
        "(CASE WHEN \"c\" IS NULL THEN '\\N' ELSE CAST(\"c\" AS VARCHAR(40)) END)"
    );
    assert_eq!(
        column_digest_expr(d, "x"),
        "SUBSTR(LOWER(HEX(SHA256(x))), 1, 16)"
    );
    assert_eq!(
        row_hash_expr(d, &["a".into(), "b".into()]),
        "LOWER(HEX(SHA256(a || CHR(31) || b)))"
    );
    let q = bucket_checksum_query(d, "t", "pk", &["a".into()], 64);
    assert!(
        q.starts_with("SELECT bucket, COUNT(*) AS row_count, SUM(CAST(h AS DECIMAL(38, 0)))"),
        "{q}"
    );
    assert!(
        q.contains("MOD(HEX_TO_INT(SUBSTR(LOWER(HEX(SHA256(pk))), 1, 15)), 64) AS bucket"),
        "{q}"
    );
    assert!(
        q.ends_with("FROM t) b GROUP BY bucket ORDER BY bucket"),
        "{q}"
    );
    let sub = sub_bucket_checksum_query(d, "t", "pk", &["a".into()], 16, 3, 256);
    assert!(sub.contains(", 16) = 3)"), "{sub}");
    assert_eq!(count_query(d, "t"), "SELECT count(*) FROM t");
    assert!(aggregate_query(d, "t", &["e".into()]).contains("sum(OCTET_LENGTH(e)) AS len_0"));
}

/// The kit has no literal table for a registered dialect: its author writes one.
#[test]
fn a_registered_dialect_brings_its_own_literals() {
    assert!(conformance::builtin(Dialect::Registered(&PORTABLE)).is_none());
}

fn live() -> bool {
    std::env::var("EKOS_MIGRATE_LIVE").is_ok()
}

/// PostgreSQL through `psql`, fields and records split by the ASCII separators.
struct Psql;

impl EngineReader for Psql {
    fn dialect(&self) -> Dialect {
        Dialect::Postgres
    }
    fn label(&self) -> &str {
        "psql:localhost:55432/ekos_migrate_fixtures"
    }
    fn query(&self, sql: &str) -> Result<Vec<Vec<String>>, ReadError> {
        let out = Command::new("psql")
            .args([
                "-h",
                "localhost",
                "-p",
                "55432",
                "-U",
                "ekos",
                "-d",
                "ekos_migrate_fixtures",
                "-tA",
                "-F",
                "\u{1e}",
                "-R",
                "\u{1d}",
                "-c",
                sql,
            ])
            .env("PGPASSWORD", "ekos-local-only")
            .output()
            .map_err(|e| ReadError::Query {
                engine: "psql".into(),
                message: e.to_string(),
            })?;
        if !out.status.success() {
            return Err(ReadError::Query {
                engine: "psql".into(),
                message: String::from_utf8_lossy(&out.stderr).into_owned(),
            });
        }
        let text = String::from_utf8_lossy(&out.stdout);
        let text = text.strip_suffix('\n').unwrap_or(&text);
        // `-tA` prints nothing both for no row and for one empty value; every query the kit
        // sends returns at least one row, so nothing is one empty value.
        Ok(text
            .split('\u{1d}')
            .map(|r| r.split('\u{1e}').map(str::to_string).collect())
            .collect())
    }
}

/// ClickHouse over HTTP, rows as JSON arrays of strings.
struct ClickHouseHttp;

impl EngineReader for ClickHouseHttp {
    fn dialect(&self) -> Dialect {
        Dialect::ClickHouse
    }
    fn label(&self) -> &str {
        "http://localhost:58123"
    }
    fn query(&self, sql: &str) -> Result<Vec<Vec<String>>, ReadError> {
        let out = Command::new("curl")
            .args([
                "-s",
                "--fail-with-body",
                "http://localhost:58123/?user=ekos&password=ekos-local-only&output_format_json_quote_64bit_integers=1&output_format_json_quote_decimals=1",
                "--data-binary",
                &format!("{sql} FORMAT JSONCompactStrings"),
            ])
            .output()
            .map_err(|e| ReadError::Query {
                engine: "clickhouse".into(),
                message: e.to_string(),
            })?;
        let body = String::from_utf8_lossy(&out.stdout).into_owned();
        if !out.status.success() {
            return Err(ReadError::Query {
                engine: "clickhouse".into(),
                message: body,
            });
        }
        let v: serde_json::Value = serde_json::from_str(&body).map_err(|e| ReadError::Query {
            engine: "clickhouse".into(),
            message: format!("{e}: {body}"),
        })?;
        Ok(v["data"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|row| {
                row.as_array()
                    .into_iter()
                    .flatten()
                    .map(|c| c.as_str().unwrap_or_default().to_string())
                    .collect()
            })
            .collect())
    }
}

fn check(reader: &dyn EngineReader, l: &dyn Literals, cases: usize) {
    let report = conformance::run(reader, l);
    eprintln!(
        "RFC 0255 conformance {}: {} cases, {} hashes, buckets {}, passed {}, id {}",
        report.dialect,
        report.cases_checked,
        report.hashes_checked,
        report.buckets_checked,
        report.passed(),
        report.id
    );
    assert!(report.passed(), "{:#?}", report);
    assert_eq!(report.cases_checked, cases);
}

#[test]
fn the_kit_passes_on_postgres() {
    if !live() {
        eprintln!("skipped: set EKOS_MIGRATE_LIVE=1 with the sandboxes up");
        return;
    }
    let l = conformance::builtin(Dialect::Postgres).unwrap();
    check(&Psql, l.as_ref(), 31);
}

#[test]
fn the_kit_passes_on_clickhouse() {
    if !live() {
        eprintln!("skipped: set EKOS_MIGRATE_LIVE=1 with the sandboxes up");
        return;
    }
    let l = conformance::builtin(Dialect::ClickHouse).unwrap();
    check(&ClickHouseHttp, l.as_ref(), 29);
}

/// The kit is not fooled: PostgreSQL read through a reader that claims to be another engine's
/// dialect — here the canonical forms of Trino, whose functions PostgreSQL partly lacks — fails.
#[test]
fn the_kit_fails_a_dialect_the_engine_does_not_speak() {
    if !live() {
        return;
    }
    let l = conformance::builtin(Dialect::Trino).unwrap();
    let report = conformance::run(&Psql, l.as_ref());
    assert!(!report.passed());
    assert!(!report.mismatches.is_empty());
}
