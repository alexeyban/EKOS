//! RFC 0155 — the generated SQL, pinned.
//!
//! These expressions are the *other half* of the canonical form: tiers V1–V3 push the work into
//! the engines rather than pulling a hundred million rows across a network to hash them. The Rust
//! implementation and the pushed-down SQL must produce identical bytes, and today only the Rust
//! side is proven against the golden literals (no driver exists until RFC 0157).
//!
//! So this snapshot does the one thing that can be done without a live engine: it freezes the SQL,
//! so a change to a dialect expression is a deliberate, reviewed diff rather than a silent drift
//! away from a canonical form that two other systems depend on. When the driver lands, the same
//! strings get executed and compared against the same golden hashes.

use ekos_migrate_validate::dialect::{
    ColumnRule, Dialect, bucket_checksum_query, canon_expr, row_hash_expr,
};

fn rules() -> Vec<(&'static str, ColumnRule)> {
    vec![
        ("txt", ColumnRule::Text),
        ("ch", ColumnRule::Char),
        ("flag", ColumnRule::Bool),
        ("n", ColumnRule::Int),
        ("amt", ColumnRule::Decimal(2)),
        ("ts", ColumnRule::TimestampUtc),
        ("ts_naive", ColumnRule::TimestampNaive),
        ("d", ColumnRule::Date),
        ("t", ColumnRule::Time),
        ("u", ColumnRule::Uuid),
        ("b", ColumnRule::Bytes),
        ("ip", ColumnRule::Inet),
    ]
}

fn render(d: Dialect) -> String {
    let mut out = String::new();
    for (col, rule) in rules() {
        out.push_str(&format!("{rule:?}\n  {}\n", canon_expr(d, col, rule)));
    }
    let exprs: Vec<String> = rules()
        .into_iter()
        .map(|(c, r)| canon_expr(d, c, r))
        .collect();
    out.push_str(&format!("ROW_HASH\n  {}\n", row_hash_expr(d, &exprs[..2])));
    out.push_str(&format!(
        "BUCKET_QUERY\n  {}\n",
        bucket_checksum_query(
            d,
            "s.orders",
            &canon_expr(d, "id", ColumnRule::Int),
            &exprs[..2],
            4096
        )
    ));
    out
}

/// Regenerate deliberately with `UPDATE_SNAPSHOTS=1 cargo test -p ekos-migrate-validate`.
fn check(name: &str, actual: &str) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/snapshots")
        .join(name);
    if std::env::var("UPDATE_SNAPSHOTS").is_ok() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "missing snapshot {}: {e}. Run with UPDATE_SNAPSHOTS=1",
            path.display()
        )
    });
    assert_eq!(
        actual, expected,
        "generated SQL drifted from the snapshot. If deliberate, re-run with UPDATE_SNAPSHOTS=1 \
         and review the diff — these expressions are a wire format shared with another engine."
    );
}

#[test]
fn postgres_sql_is_pinned() {
    check("postgres.sql.txt", &render(Dialect::Postgres));
}

#[test]
fn clickhouse_sql_is_pinned() {
    check("clickhouse.sql.txt", &render(Dialect::ClickHouse));
}

/// Every `ColumnRule` variant appears above. A new rule with no pinned SQL would otherwise ship
/// unreviewed.
#[test]
fn every_column_rule_is_pinned() {
    let covered = rules().len();
    let src = include_str!("../src/dialect.rs");
    let variants = src
        .split("pub enum ColumnRule {")
        .nth(1)
        .unwrap()
        .split('}')
        .next()
        .unwrap()
        .lines()
        .filter(|l| {
            let l = l.trim();
            !l.is_empty() && !l.starts_with("//") && !l.starts_with("/*")
        })
        .count();
    assert_eq!(variants, covered, "a ColumnRule variant has no pinned SQL");
}
