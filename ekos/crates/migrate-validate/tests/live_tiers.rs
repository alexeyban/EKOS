//! RFC 0156 — the tiers and bisect, against real engines.
//!
//! A real 5,000-row table is loaded into PostgreSQL and into ClickHouse, then defects are planted in
//! the ClickHouse copy one at a time and the tiers are asked to find them. The assertions are about
//! *which tier* catches *which defect*, because that table in RFC 0156 is a specification, not a
//! wish: a control that fires at a lower tier than claimed means the tier table is wrong, and one
//! that fires at none means the validator is.
//!
//! ```text
//! docker compose -f docker-compose.migrate.yml up -d
//! EKOS_MIGRATE_LIVE=1 cargo test -p ekos-migrate-validate --test live_tiers
//! ```

use ekos_migrate_validate::bisect::{BisectPolicy, bisect_bucket, run_v4};
use ekos_migrate_validate::dialect::{self, ColumnRule, Dialect};
use ekos_migrate_validate::reader::{EngineReader, ReadError};
use ekos_migrate_validate::tiers::{UnitPlan, run_v1, run_v2, run_v3};
use std::process::Command;

const ROWS: i64 = 5_000;
const BUCKETS: u32 = 64;

fn live() -> bool {
    std::env::var("EKOS_MIGRATE_LIVE").is_ok()
}

// ── readers ──────────────────────────────────────────────────────────────────

struct Psql;
impl EngineReader for Psql {
    fn dialect(&self) -> Dialect {
        Dialect::Postgres
    }
    fn label(&self) -> &str {
        "psql:source"
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
                "\t",
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
                message: String::from_utf8_lossy(&out.stderr).to_string(),
            });
        }
        Ok(split_rows(&String::from_utf8_lossy(&out.stdout)))
    }
}

/// The ClickHouse *reader* is a separate user from the writer below — RFC 0156's independent-oracle
/// rule in its smallest honest form: the validator does not read the target through the thing that
/// wrote it.
struct Ch;
impl EngineReader for Ch {
    fn dialect(&self) -> Dialect {
        Dialect::ClickHouse
    }
    fn label(&self) -> &str {
        "clickhouse:reader"
    }
    fn query(&self, sql: &str) -> Result<Vec<Vec<String>>, ReadError> {
        Ok(split_rows(&ch_exec(&format!(
            "{sql} FORMAT TabSeparatedRaw"
        ))?))
    }
}

fn split_rows(s: &str) -> Vec<Vec<String>> {
    s.lines()
        .filter(|l| !l.is_empty())
        .map(|l| l.split('\t').map(str::to_string).collect())
        .collect()
}

fn ch_exec(sql: &str) -> Result<String, ReadError> {
    let out = Command::new("curl")
        .args([
            "-s",
            "--fail-with-body",
            "http://localhost:58123/?user=ekos&password=ekos-local-only",
            "--data-binary",
            sql,
        ])
        .output()
        .map_err(|e| ReadError::Query {
            engine: "clickhouse".into(),
            message: e.to_string(),
        })?;
    let body = String::from_utf8_lossy(&out.stdout).to_string();
    if !out.status.success() {
        return Err(ReadError::Query {
            engine: "clickhouse".into(),
            message: body,
        });
    }
    Ok(body)
}

fn ch(sql: &str) {
    ch_exec(sql).unwrap_or_else(|e| panic!("clickhouse: {e}\nsql: {sql}"));
}

fn pg(sql: &str) {
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
            "-v",
            "ON_ERROR_STOP=1",
            "-q",
            "-c",
            sql,
        ])
        .env("PGPASSWORD", "ekos-local-only")
        .output()
        .expect("psql not runnable");
    assert!(
        out.status.success(),
        "psql: {}\nsql: {sql}",
        String::from_utf8_lossy(&out.stderr)
    );
}

// ── fixture ──────────────────────────────────────────────────────────────────

/// Load the same logical table into both engines.
///
/// `note` is null on every fifth row, so a NULL-handling defect has somewhere to show up.
///
/// `tag_a` and `tag_b` exist for one reason: over `ROWS` rows they have **identical value
/// multisets** (`x0`..`x9`, 500 of each, offset by five). Swapping them per row therefore changes
/// every row while leaving every per-column aggregate — null count, min, max, summed length —
/// exactly as it was. That is what makes it a real test of RFC 0156's claim that a same-typed column
/// swap is invisible below V3. A swap of `name` and `note` is *not* that test: measured live, V2
/// catches it on three aggregates, because their multisets differ.
fn load(suffix: &str) {
    let t = format!("orders_{suffix}");
    pg(&format!("DROP TABLE IF EXISTS {t}"));
    pg(&format!(
        "CREATE TABLE {t} (id bigint primary key, name text not null, \
         amount numeric(12,2) not null, note text, tag_a text not null, tag_b text not null)"
    ));
    pg(&format!(
        "INSERT INTO {t} SELECT g, 'customer-' || g, (1000 + g * 7)::numeric / 100, \
         case when g % 5 = 0 then null else 'customer-' || (g + 1) end, \
         'x' || (g % 10), 'x' || ((g + 5) % 10) \
         FROM generate_series(1, {ROWS}) g"
    ));

    ch(&format!("DROP TABLE IF EXISTS {t}"));
    ch(&format!(
        "CREATE TABLE {t} (id Int64, name String, amount Decimal128(2), note Nullable(String), \
         tag_a String, tag_b String) ENGINE = MergeTree ORDER BY id"
    ));
    ch(&format!(
        "INSERT INTO {t} SELECT number + 1 AS id, concat('customer-', toString(number + 1)), \
         toDecimal128(1000 + (number + 1) * 7, 2) / 100, \
         if((number + 1) % 5 = 0, NULL, concat('customer-', toString(number + 2))), \
         concat('x', toString((number + 1) % 10)), \
         concat('x', toString((number + 6) % 10)) \
         FROM numbers({ROWS})"
    ));
}

fn plan(suffix: &str) -> UnitPlan {
    let t = format!("orders_{suffix}");
    let cols = |d: Dialect| {
        vec![
            dialect::canon_expr(d, "id", ColumnRule::Int),
            dialect::canon_expr(d, "name", ColumnRule::Text),
            dialect::canon_expr(d, "amount", ColumnRule::Decimal(2)),
            dialect::canon_expr(d, "note", ColumnRule::Text),
            dialect::canon_expr(d, "tag_a", ColumnRule::Text),
            dialect::canon_expr(d, "tag_b", ColumnRule::Text),
        ]
    };
    UnitPlan {
        unit: t.clone(),
        source_table: t.clone(),
        target_table: t.clone(),
        source_pk: dialect::canon_expr(Dialect::Postgres, "id", ColumnRule::Int),
        target_pk: dialect::canon_expr(Dialect::ClickHouse, "id", ColumnRule::Int),
        source_columns: cols(Dialect::Postgres),
        target_columns: cols(Dialect::ClickHouse),
        buckets: BUCKETS,
    }
}

fn names() -> Vec<String> {
    ["id", "name", "amount", "note", "tag_a", "tag_b"]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

// ── the clean baseline ───────────────────────────────────────────────────────

/// The complement of every control below, and the more important half: a correct migration must
/// produce **zero** divergences at every tier. A validator that cries wolf is abandoned, and an
/// abandoned validator proves nothing.
#[test]
fn a_correct_migration_is_silent_at_every_tier() {
    if !live() {
        eprintln!("skipped: set EKOS_MIGRATE_LIVE=1 with the sandboxes up");
        return;
    }
    load("clean");
    let p = plan("clean");

    let v1 = run_v1(&p, &Psql, &Ch).unwrap();
    assert!(v1.divergences.is_empty(), "V1: {:?}", v1.divergences);

    let v2 = run_v2(&p, &Psql, &Ch, &names()).unwrap();
    assert!(v2.divergences.is_empty(), "V2: {:?}", v2.divergences);

    let v3 = run_v3(&p, &Psql, &Ch).unwrap();
    assert!(v3.divergences.is_empty(), "V3: {:?}", v3.divergences);

    // And the read paths are recorded, which is what makes the independent-oracle rule auditable.
    assert_eq!(v3.source_path, "psql:source");
    assert_eq!(v3.target_path, "clickhouse:reader");
}

// ── controls, with the tier each must be caught by ───────────────────────────

fn failed_buckets(p: &UnitPlan) -> Vec<u32> {
    run_v3(p, &Psql, &Ch)
        .unwrap()
        .divergences
        .iter()
        .map(|d| d.locus.rsplit(':').next().unwrap().parse().unwrap())
        .collect()
}

#[test]
fn a_dropped_row_is_caught_at_v1() {
    if !live() {
        return;
    }
    load("drop1");
    ch("ALTER TABLE orders_drop1 DELETE WHERE id = 2500 SETTINGS mutations_sync = 2");
    let p = plan("drop1");
    assert_eq!(
        run_v1(&p, &Psql, &Ch).unwrap().divergences.len(),
        1,
        "V1 must see a count change"
    );
    assert!(
        !run_v3(&p, &Psql, &Ch).unwrap().divergences.is_empty(),
        "V3 too"
    );
}

#[test]
fn a_null_turned_into_an_empty_string_is_caught_at_v2_and_v3() {
    if !live() {
        return;
    }
    load("nullempty");
    ch(
        "ALTER TABLE orders_nullempty UPDATE note = '' WHERE note IS NULL SETTINGS mutations_sync = 2",
    );
    let p = plan("nullempty");
    assert!(
        run_v1(&p, &Psql, &Ch).unwrap().divergences.is_empty(),
        "the row count is unchanged, so V1 cannot see this — that is why V2 and V3 exist"
    );
    let v2 = run_v2(&p, &Psql, &Ch, &names()).unwrap();
    assert!(
        v2.divergences
            .iter()
            .any(|d| d.locus.contains("note:nulls")),
        "V2 must see the null count change: {:?}",
        v2.divergences
    );
    assert!(!run_v3(&p, &Psql, &Ch).unwrap().divergences.is_empty());
}

/// The control V1 and V2 are both blind to, tested against columns whose value multisets are
/// identical so that no aggregate can move. Row count unchanged, null counts unchanged, min, max and
/// summed length unchanged on every column — and every row different. Only a row-level hash sees it,
/// which is the entire reason V3 exists.
///
/// The assertion on V2 is the load-bearing half: without it this test would pass on a fixture where
/// V2 also catches the swap, and RFC 0156's tier table would be untested rather than verified.
#[test]
fn an_aggregate_invariant_column_swap_is_caught_only_at_v3() {
    if !live() {
        return;
    }
    load("swap");
    // ClickHouse requires a WHERE on ALTER ... UPDATE, and applies every assignment against the
    // *original* row, so this is a true simultaneous swap rather than two sequential writes.
    ch(
        "ALTER TABLE orders_swap UPDATE tag_a = tag_b, tag_b = tag_a WHERE 1 = 1 \
         SETTINGS mutations_sync = 2",
    );
    let p = plan("swap");

    assert!(
        run_v1(&p, &Psql, &Ch).unwrap().divergences.is_empty(),
        "V1 cannot see a column swap"
    );
    let v2 = run_v2(&p, &Psql, &Ch, &names()).unwrap();
    assert!(
        v2.divergences.is_empty(),
        "V2 must be blind to this swap for the test to mean anything — if it is not, the fixture's \
         columns do not have identical multisets and RFC 0156's claim is untested: {:?}",
        v2.divergences.iter().map(|d| &d.locus).collect::<Vec<_>>()
    );
    let v3 = run_v3(&p, &Psql, &Ch).unwrap();
    assert!(
        !v3.divergences.is_empty(),
        "V3 MUST catch an aggregate-invariant column swap — it is the defect that justifies the tier"
    );
}

/// One wrong byte in one row out of five thousand, isolated to the exact key by bisect.
#[test]
fn a_single_corrupted_row_is_bisected_to_its_key() {
    if !live() {
        return;
    }
    load("onebyte");
    ch(
        "ALTER TABLE orders_onebyte UPDATE name = 'customer-1337 ' WHERE id = 1337 SETTINGS mutations_sync = 2",
    );
    let p = plan("onebyte");

    let buckets = failed_buckets(&p);
    assert_eq!(
        buckets.len(),
        1,
        "exactly one bucket should differ, got {buckets:?}"
    );

    let diffs = bisect_bucket(&p, &Psql, &Ch, buckets[0], BisectPolicy::default()).unwrap();
    assert_eq!(
        diffs.len(),
        1,
        "bisect should isolate one key, got {diffs:?}"
    );
    assert_eq!(diffs[0].key(), "1337");

    // And V4 reports it masked, never in the clear.
    let v4 = run_v4(&p, &Psql, &Ch, &buckets, BisectPolicy::default()).unwrap();
    assert_eq!(v4.divergences.len(), 1);
    assert!(
        !v4.divergences[0].locus.contains("1337"),
        "the key must be masked in a fact: {}",
        v4.divergences[0].locus
    );
    assert_eq!(v4.divergences[0].detail, "row differs");
}

/// Bisect must survive a bucket small enough to skip subdivision entirely, and one large enough to
/// need several rounds. A threshold of 1 forces the deep path on a 5,000-row table.
#[test]
fn bisect_reaches_the_key_whatever_the_threshold() {
    if !live() {
        return;
    }
    load("deep");
    ch(
        "ALTER TABLE orders_deep UPDATE amount = amount + 1 WHERE id = 4242 SETTINGS mutations_sync = 2",
    );
    let p = plan("deep");
    let buckets = failed_buckets(&p);
    assert_eq!(buckets.len(), 1);

    for threshold in [1u64, 10, 100, 100_000] {
        let policy = BisectPolicy {
            row_threshold: threshold,
            ..Default::default()
        };
        let diffs = bisect_bucket(&p, &Psql, &Ch, buckets[0], policy)
            .unwrap_or_else(|e| panic!("threshold {threshold}: {e}"));
        assert_eq!(diffs.len(), 1, "threshold {threshold}: {diffs:?}");
        assert_eq!(diffs[0].key(), "4242", "threshold {threshold}");
    }
}

/// A row present only in the target — the direction a naive count check reads as "extra rows are
/// fine" — must be named as such.
#[test]
fn a_row_only_in_the_target_is_identified_by_direction() {
    if !live() {
        return;
    }
    load("extra");
    ch("INSERT INTO orders_extra VALUES (99999, 'ghost', 1.00, NULL, 'x0', 'x5')");
    let p = plan("extra");
    let buckets = failed_buckets(&p);
    assert!(!buckets.is_empty());
    let v4 = run_v4(&p, &Psql, &Ch, &buckets, BisectPolicy::default()).unwrap();
    assert!(
        v4.divergences
            .iter()
            .any(|d| d.detail == "present only in target"),
        "{:?}",
        v4.divergences
    );
}
