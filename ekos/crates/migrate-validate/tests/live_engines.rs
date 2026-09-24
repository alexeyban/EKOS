//! RFC 0155's actual acceptance criterion: **three-way agreement**.
//!
//! PostgreSQL, ClickHouse and the Rust implementation must each produce the canonical form that
//! `golden.rs` froze as a literal. Two engines agreeing with each other proves nothing if both are
//! wrong the same way, so every assertion here is against the committed literal, never against the
//! other engine.
//!
//! **Running it.** Requires the sandboxes and is skipped without them:
//! ```text
//! docker compose -f docker-compose.migrate.yml up -d
//! EKOS_MIGRATE_LIVE=1 cargo test -p ekos-migrate-validate --test live_engines
//! ```
//!
//! **Why `psql` and `curl` rather than a driver.** Choosing between `tokio-postgres` and `sqlx` is
//! RFC 0157's decision, and it is entangled with the still-open question of how a chunk-parallel
//! executor coexists with the non-`Sync` `KnowledgeStore`. Pulling that decision forward just to
//! get a test running would be the tail wagging the dog. Shelling out is confined to this file,
//! which is a test — RFC 0147's no-shell-out rule is about the *recovery* path, where determinism
//! and offline operation are the point.
//!
//! The harness applies `canon_expr_of` to a typed literal, so it exercises the exact expression
//! tiers V1–V3 push down. Rebuilding the expression here would only test a second copy of it.

use ekos_migrate_validate::{Dialect, canon_expr_of, fixtures};
use std::process::Command;

fn live() -> bool {
    std::env::var("EKOS_MIGRATE_LIVE").is_ok()
}

fn pg(sql: &str) -> String {
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
            "-c",
            sql,
        ])
        .env("PGPASSWORD", "ekos-local-only")
        .output()
        .expect("psql not runnable — is the sandbox up?");
    assert!(
        out.status.success(),
        "psql failed for {sql}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout)
        .trim_end_matches('\n')
        .to_string()
}

fn ch(sql: &str) -> String {
    let sql = sql.to_string();
    let out = Command::new("curl")
        .args([
            "-s",
            "--fail-with-body",
            "http://localhost:58123/?user=ekos&password=ekos-local-only",
            // Raw, because the default TabSeparated *escapes* backslashes on output — the harness
            // would then compare an escaped rendering against an unescaped literal and disagree
            // for a reason that has nothing to do with the canonical form.
            "--data-binary",
            &format!("{sql} FORMAT TabSeparatedRaw"),
        ])
        .output()
        .expect("curl not runnable — is the sandbox up?");
    let body = String::from_utf8_lossy(&out.stdout)
        .trim_end_matches('\n')
        .to_string();
    assert!(out.status.success(), "clickhouse failed for {sql}\n{body}");
    body
}

/// The frozen canonical form for a case, read from the same fixture table the golden test pins.
fn expected(name: &str) -> String {
    let (_, value, spec) = fixtures::cases()
        .into_iter()
        .find(|(n, _, _)| *n == name)
        .unwrap_or_else(|| panic!("no fixture named {name}"));
    ekos_migrate_validate::canon(&value, &spec).unwrap()
}

#[test]
fn postgres_reproduces_every_canonical_form() {
    if !live() {
        eprintln!("skipped: set EKOS_MIGRATE_LIVE=1 with the sandboxes up");
        return;
    }
    let mut checked = 0;
    for case in fixtures::live_cases() {
        let Some(lit) = case.postgres else { continue };
        let expr = canon_expr_of(Dialect::Postgres, lit, case.rule);
        let actual = pg(&format!("SELECT {expr}"));
        assert_eq!(
            actual,
            expected(case.name),
            "postgres disagrees on '{}'",
            case.name
        );
        checked += 1;
    }
    // An exact count, not a floor: a case silently dropping off the live path is the failure this
    // whole file exists to prevent, and `every_fixture_is_either_live_or_explained` guards the
    // other direction.
    assert_eq!(checked, 31, "the postgres live-case count changed");
}

#[test]
fn clickhouse_reproduces_every_canonical_form() {
    if !live() {
        eprintln!("skipped: set EKOS_MIGRATE_LIVE=1 with the sandboxes up");
        return;
    }
    let mut checked = 0;
    for case in fixtures::live_cases() {
        let Some(lit) = case.clickhouse else { continue };
        let expr = canon_expr_of(Dialect::ClickHouse, lit, case.rule);
        let actual = ch(&format!("SELECT {expr}"));
        assert_eq!(
            actual,
            expected(case.name),
            "clickhouse disagrees on '{}'",
            case.name
        );
        checked += 1;
    }
    assert_eq!(checked, 29, "the clickhouse live-case count changed");
}

/// The row hash itself, end to end: both engines must produce the same `md5` of the same joined
/// canonical row as the Rust implementation.
#[test]
fn both_engines_reproduce_the_row_hash() {
    if !live() {
        return;
    }
    use ekos_migrate_validate::{ColumnRule, Spec, Value, canon, row_canonical, row_hash};

    let cols = vec![
        canon(&Value::Int(42), &Spec::default()).unwrap(),
        canon(&Value::Text("a\u{1f}b".into()), &Spec::default()).unwrap(),
        canon(&Value::Null, &Spec::default()).unwrap(),
    ];
    let want = row_hash(&row_canonical(&cols));

    let pg_exprs = [
        canon_expr_of(Dialect::Postgres, "42::bigint", ColumnRule::Int),
        canon_expr_of(Dialect::Postgres, "E'a\\x1fb'::text", ColumnRule::Text),
        canon_expr_of(Dialect::Postgres, "NULL::text", ColumnRule::Text),
    ];
    let ch_exprs = [
        canon_expr_of(
            Dialect::ClickHouse,
            "CAST(42 AS Nullable(Int64))",
            ColumnRule::Int,
        ),
        canon_expr_of(
            Dialect::ClickHouse,
            "CAST(concat('a', char(31), 'b') AS Nullable(String))",
            ColumnRule::Text,
        ),
        canon_expr_of(
            Dialect::ClickHouse,
            "CAST(NULL AS Nullable(String))",
            ColumnRule::Text,
        ),
    ];

    let pg_hash = pg(&format!(
        "SELECT {}",
        ekos_migrate_validate::dialect::row_hash_expr(Dialect::Postgres, &pg_exprs)
    ));
    let ch_hash = ch(&format!(
        "SELECT {}",
        ekos_migrate_validate::dialect::row_hash_expr(Dialect::ClickHouse, &ch_exprs)
    ));

    assert_eq!(
        pg_hash, want,
        "postgres row hash disagrees with the Rust implementation"
    );
    assert_eq!(
        ch_hash, want,
        "clickhouse row hash disagrees with the Rust implementation"
    );
}

/// The 60-bit prefix: the expression whose endianness differs between the engines, and the one
/// most likely to be "simplified" into a silent total mismatch later.
#[test]
fn both_engines_reproduce_the_hash_prefix() {
    if !live() {
        return;
    }
    let want = ekos_migrate_validate::prefix60(&ekos_migrate_validate::row_hash("abc")).to_string();
    let pg_got = pg(&format!(
        "SELECT {}",
        ekos_migrate_validate::dialect::prefix60_expr(Dialect::Postgres, "md5('abc')")
    ));
    let ch_got = ch(&format!(
        "SELECT {}",
        ekos_migrate_validate::dialect::prefix60_expr(
            Dialect::ClickHouse,
            "lower(hex(MD5('abc')))"
        )
    ));
    assert_eq!(pg_got, want);
    assert_eq!(
        ch_got, want,
        "clickhouse read the prefix with the wrong endianness"
    );
}

/// Every fixture is either exercised live on both engines or named in `UNMAPPED_LIVE_CASES` with a
/// reason. Silent gaps in coverage are how a canonical form quietly stops being verified.
#[test]
fn every_fixture_is_either_live_or_explained() {
    let live_cases = fixtures::live_cases();
    assert_eq!(
        live_cases.len(),
        fixtures::cases().len(),
        "the live table and the fixture table disagree on how many cases exist"
    );
    for case in &live_cases {
        if case.postgres.is_some() && case.clickhouse.is_some() {
            continue;
        }
        assert!(
            fixtures::UNMAPPED_LIVE_CASES
                .iter()
                .any(|(n, _)| *n == case.name),
            "'{}' is not exercised on both engines and has no recorded reason",
            case.name
        );
    }
    // And no stale entries claiming a gap that no longer exists.
    for (name, _) in fixtures::UNMAPPED_LIVE_CASES {
        let case = live_cases.iter().find(|c| c.name == *name).unwrap();
        assert!(
            case.postgres.is_none() || case.clickhouse.is_none(),
            "'{name}' is listed as unmapped but is now exercised on both engines"
        );
    }
}
