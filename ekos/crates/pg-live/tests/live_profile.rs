//! RFC 0157 — profiling against a real PostgreSQL.
//!
//! The assertions that matter are the negative ones: that a PII column yields **no** values at any
//! tier, and that the planner's conventions are read correctly rather than plausibly.
//!
//! ```text
//! docker compose -f docker-compose.migrate.yml up -d
//! EKOS_MIGRATE_LIVE=1 cargo test -p ekos-pg-live --test live_profile
//! ```

use ekos_common::redaction::RedactionConfig;
use ekos_migrate::ConnectionRef;
use ekos_pg_live::profile::{
    ProfileTier, distinct_count, estimate_cost, exact_row_count, profile_columns_p0,
    profile_columns_p1, profile_table_p0,
};
use ekos_pg_live::{PgSource, SessionPolicy};

const SCHEMA: &str = "ekos_prof";
const TABLE: &str = "ekos_prof.people";

fn live() -> bool {
    std::env::var("EKOS_MIGRATE_LIVE").is_ok()
}

fn connect() -> PgSource {
    unsafe { std::env::set_var("EKOS_TEST_PG_PASSWORD", "ekos-local-only") };
    let conn = ConnectionRef::parse("postgres://local/ekos_migrate_fixtures")
        .unwrap()
        .with_secret_env(Some("EKOS_TEST_PG_PASSWORD".into()));
    PgSource::connect(
        &conn,
        "localhost",
        55432,
        "ekos",
        "run-live-profile",
        &SessionPolicy::default(),
    )
    .expect("connect")
}

fn fixture() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let ddl = format!(
            r#"
DROP SCHEMA IF EXISTS {SCHEMA} CASCADE;
CREATE SCHEMA {SCHEMA};
CREATE TABLE {TABLE} (
    id           bigint PRIMARY KEY,
    email        text NOT NULL,
    full_name    text NOT NULL,
    country      text NOT NULL,
    amount       numeric NOT NULL,
    created_at   timestamptz NOT NULL,
    note         text
);
INSERT INTO {TABLE}
SELECT g,
       'user' || g || '@example.com',
       'Person ' || g,
       (ARRAY['GB','US','DE','FR'])[1 + (g % 4)],
       -- max precision 6, max scale 2: comfortably inside Decimal(18,2)
       ((1000 + g) || '.' || lpad((g % 100)::text, 2, '0'))::numeric,
       timestamptz '2026-01-01 00:00:00+00' + (g || ' seconds')::interval,
       CASE WHEN g % 5 = 0 THEN NULL ELSE 'note ' || g END
FROM generate_series(1, 2000) g;
ANALYZE {TABLE};
"#
        );
        let out = std::process::Command::new("psql")
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
                &ddl,
            ])
            .env("PGPASSWORD", "ekos-local-only")
            .output()
            .expect("psql");
        assert!(
            out.status.success(),
            "fixture failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    });
}

#[test]
fn p0_reads_the_catalog_without_scanning() {
    if !live() {
        eprintln!("skipped: set EKOS_MIGRATE_LIVE=1 with the sandboxes up");
        return;
    }
    fixture();
    let src = connect();
    let p = profile_table_p0(&src, TABLE).unwrap();

    assert_eq!(p.tier, ProfileTier::P0);
    assert!(
        !p.row_count_is_exact,
        "reltuples is an estimate and must say so"
    );
    assert!(
        (1900..=2100).contains(&p.row_count),
        "estimate should be near 2000, got {}",
        p.row_count
    );
    assert!(p.total_bytes > 0);
    assert!(p.last_analyze.is_some(), "the fixture ran ANALYZE");
    assert!(p.inserts >= 2000, "pg_stat_user_tables should see the load");
    assert!(
        !p.has_updates(),
        "no updates — this table wants MergeTree, not Replacing"
    );
}

/// The convention that turns a primary key into a `LowCardinality` mapping if read naively.
#[test]
fn n_distinct_conventions_are_read_correctly() {
    if !live() {
        return;
    }
    fixture();
    let src = connect();
    let cols = profile_columns_p0(&src, TABLE, &RedactionConfig::default()).unwrap();
    let get = |n: &str| {
        cols.iter()
            .find(|c| c.qualified_name.ends_with(&format!(".{n}")))
            .unwrap_or_else(|| panic!("no column {n}"))
            .clone()
    };

    // `country` has 4 values in 2000 rows — a positive count.
    let country = distinct_count(get("country").distinct_estimate, 2000).unwrap();
    assert!(
        (country - 4.0).abs() < 1.0,
        "country distinct ≈ 4, got {country}"
    );

    // `id` is unique — PostgreSQL records -1, meaning "a fraction of rows equal to 1".
    let id = distinct_count(get("id").distinct_estimate, 2000).unwrap();
    assert!(
        id > 1500.0,
        "a unique column must resolve to ~rowcount, not to 1 — got {id}"
    );
}

/// The load-bearing test of this module. `pg_stats` holds literal values, so "free" does not mean
/// "safe", and a PII column must yield nothing at any tier.
#[test]
fn a_pii_column_yields_no_values_at_any_tier() {
    if !live() {
        return;
    }
    fixture();
    let src = connect();
    let p0 = profile_columns_p0(&src, TABLE, &RedactionConfig::default()).unwrap();
    let p1 = profile_columns_p1(&src, TABLE, p0.clone(), 100.0, 200).unwrap();

    for (tier, cols) in [("P0", &p0), ("P1", &p1)] {
        for name in ["email", "full_name"] {
            let c = cols
                .iter()
                .find(|c| c.qualified_name.ends_with(&format!(".{name}")))
                .unwrap();
            assert!(c.pii.is_some(), "{tier}: {name} must be classified");
            assert!(c.values_suppressed, "{tier}: {name} must suppress values");
            assert_eq!(c.min, None, "{tier}: {name} leaked a min");
            assert_eq!(c.max, None, "{tier}: {name} leaked a max");
        }
        // And the whole serialized profile contains no address from the table.
        let dump = serde_json::to_string(cols).unwrap();
        assert!(
            !dump.contains("@example.com"),
            "{tier}: an email reached the profile"
        );
        assert!(
            !dump.contains("Person "),
            "{tier}: a name reached the profile"
        );
    }
}

/// A non-PII numeric column keeps its bounds, because RFC 0158's range rules need them.
#[test]
fn a_non_pii_numeric_column_keeps_its_bounds() {
    if !live() {
        return;
    }
    fixture();
    let src = connect();
    let cols = profile_columns_p0(&src, TABLE, &RedactionConfig::default()).unwrap();
    let amount = cols
        .iter()
        .find(|c| c.qualified_name.ends_with(".amount"))
        .unwrap();
    assert!(!amount.values_suppressed);
    assert!(
        amount.min.is_some() && amount.max.is_some(),
        "a numeric bound is what tells RFC 0158 whether the target type fits"
    );
}

/// A *text* column keeps no bounds even when it is not PII: a min or a max is a value.
#[test]
fn a_text_column_keeps_no_bounds_even_when_not_pii() {
    if !live() {
        return;
    }
    fixture();
    let src = connect();
    let cols = profile_columns_p0(&src, TABLE, &RedactionConfig::default()).unwrap();
    let note = cols
        .iter()
        .find(|c| c.qualified_name.ends_with(".note"))
        .unwrap();
    assert!(note.pii.is_none(), "note is not PII");
    assert_eq!(note.min, None, "a text min is a row value");
    assert_eq!(note.max, None);
    assert!(note.null_fraction > 0.1, "every fifth row is null");
}

/// The measurement RFC 0159 needs to offer `narrowing-safe` instead of `Decimal(76, 20)`.
#[test]
fn p1_measures_the_numeric_scale_actually_used() {
    if !live() {
        return;
    }
    fixture();
    let src = connect();
    let p0 = profile_columns_p0(&src, TABLE, &RedactionConfig::default()).unwrap();
    let p1 = profile_columns_p1(&src, TABLE, p0, 100.0, 200).unwrap();
    let amount = p1
        .iter()
        .find(|c| c.qualified_name.ends_with(".amount"))
        .unwrap();

    assert_eq!(amount.tier, ProfileTier::P1);
    assert_eq!(amount.numeric_scale_used, Some(2));
    assert!(
        amount.numeric_precision_used.is_some_and(|p| p <= 8),
        "precision used should be small, got {:?}",
        amount.numeric_precision_used
    );
    assert_eq!(amount.fits_decimal(18, 2), Some(true));
    assert_eq!(
        amount.fits_decimal(18, 1),
        Some(false),
        "scale 2 does not fit scale 1"
    );
}

#[test]
fn p1_finds_the_watermark_candidates() {
    if !live() {
        return;
    }
    fixture();
    let src = connect();
    let p0 = profile_columns_p0(&src, TABLE, &RedactionConfig::default()).unwrap();
    let p1 = profile_columns_p1(&src, TABLE, p0, 100.0, 500).unwrap();
    for name in ["id", "created_at"] {
        let c = p1
            .iter()
            .find(|c| c.qualified_name.ends_with(&format!(".{name}")))
            .unwrap();
        assert_eq!(
            c.monotonic,
            Some(true),
            "{name} is inserted in order and should read as a watermark candidate"
        );
    }
}

#[test]
fn p2_asks_the_planner_before_it_scans() {
    if !live() {
        return;
    }
    fixture();
    let src = connect();

    let est = estimate_cost(&src, &format!("SELECT count(*) FROM {TABLE}")).unwrap();
    assert!(est.total_cost > 0.0, "the planner should cost a full scan");

    // Within budget: the scan runs and the count is exact.
    match exact_row_count(&src, TABLE, 1_000_000.0).unwrap() {
        Ok(n) => assert_eq!(n, 2000, "an exact count must be exact"),
        Err(e) => panic!("should have been within budget: {e:?}"),
    }

    // Above budget: refused, with the estimate attached so a human approving it sees the number.
    match exact_row_count(&src, TABLE, 1.0).unwrap() {
        Ok(n) => panic!("a scan above budget must not run, got {n}"),
        Err(e) => {
            assert_eq!(e.budget_rows, 1.0);
            assert!(e.estimate.estimated_rows > 1.0);
            assert!(e.sql.contains("count(*)"));
        }
    }
}

/// A primary has no replay lag, so the guard passes and reports `None`. The refusal path is
/// asserted in a unit test rather than here: making a real replica lag on demand is a far heavier
/// fixture than the behaviour warrants.
#[test]
fn the_lag_guard_passes_on_a_primary() {
    if !live() {
        return;
    }
    let src = connect();
    assert_eq!(src.guard_replica_lag(5.0).unwrap(), None);
    // And the LSN is recorded alongside, because a run fact needs both.
    assert!(src.current_lsn().unwrap().contains('/'));
}
