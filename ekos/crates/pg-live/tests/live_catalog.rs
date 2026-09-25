//! RFC 0157 — the connector against a real PostgreSQL.
//!
//! ```text
//! docker compose -f docker-compose.migrate.yml up -d
//! EKOS_MIGRATE_LIVE=1 cargo test -p ekos-pg-live
//! ```
//!
//! The fixture schema contains **one object of every `ObjectKind`**, because the coverage guarantee
//! in RFC 0154 is only as good as the introspector's denominator, and a kind nobody put in a fixture
//! is a kind nobody knows is missing.

use ekos_common::redaction::RedactionConfig;
use ekos_migrate::ConnectionRef;
use ekos_migrate_validate::EngineReader;
use ekos_pg_live::catalog::ObjectKind;
use ekos_pg_live::{PgSource, SessionPolicy, WriteCheck, introspect, reconcile};

const SCHEMA: &str = "ekos_cat";

fn live() -> bool {
    std::env::var("EKOS_MIGRATE_LIVE").is_ok()
}

fn connect() -> PgSource {
    // SAFETY: test-only. Rust 2024 makes env mutation unsafe; the harness is single-threaded here
    // because each test connects before doing anything else.
    unsafe { std::env::set_var("EKOS_TEST_PG_PASSWORD", "ekos-local-only") };
    let conn = ConnectionRef::parse("postgres://local/ekos_migrate_fixtures")
        .unwrap()
        .with_secret_env(Some("EKOS_TEST_PG_PASSWORD".into()));
    PgSource::connect(
        &conn,
        "localhost",
        55432,
        "ekos",
        "run-live-catalog",
        &SessionPolicy::default(),
    )
    .expect("connect")
}

/// A schema with one of everything. Created through a *separate* privileged session, because the
/// session under test is read-only by construction — which is itself part of the point.
///
/// `Once`, because the DDL starts with `DROP SCHEMA … CASCADE` and cargo runs tests in parallel:
/// five tests each dropping and recreating the same schema is a lock fight that fails for a reason
/// having nothing to do with what any of them is testing.
fn create_fixture() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(create_fixture_inner);
}

fn create_fixture_inner() {
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
            DDL,
        ])
        .env("PGPASSWORD", "ekos-local-only")
        .output()
        .expect("psql");
    assert!(
        out.status.success(),
        "fixture DDL failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

const DDL: &str = r#"
DROP SCHEMA IF EXISTS ekos_cat CASCADE;
CREATE SCHEMA ekos_cat;

CREATE TYPE ekos_cat.mood AS ENUM ('ok', 'bad');
CREATE DOMAIN ekos_cat.positive AS integer CHECK (VALUE > 0);
CREATE TYPE ekos_cat.span AS RANGE (subtype = integer);

CREATE SEQUENCE ekos_cat.counter;

CREATE TABLE ekos_cat.customers (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    email text NOT NULL UNIQUE,
    status ekos_cat.mood NOT NULL DEFAULT 'ok',
    -- A default carrying something that must never be persisted.
    api_key text DEFAULT 'sk-live-AKIAIOSFODNN7EXAMPLE',
    score integer CHECK (score >= 0)
);
COMMENT ON TABLE ekos_cat.customers IS 'customers; contact ops@example.com';

CREATE TABLE ekos_cat.orders (
    id bigint PRIMARY KEY,
    customer_id bigint NOT NULL REFERENCES ekos_cat.customers(id),
    total numeric(12,2) NOT NULL
);
-- A NOT VALID foreign key: present, and not a guarantee.
ALTER TABLE ekos_cat.orders
  ADD CONSTRAINT orders_customer_fk2 FOREIGN KEY (customer_id)
  REFERENCES ekos_cat.customers(id) NOT VALID;

CREATE INDEX orders_total_idx ON ekos_cat.orders (total);

CREATE TABLE ekos_cat.events (id bigint, at date NOT NULL) PARTITION BY RANGE (at);
CREATE TABLE ekos_cat.events_2026 PARTITION OF ekos_cat.events
  FOR VALUES FROM ('2026-01-01') TO ('2027-01-01');

CREATE VIEW ekos_cat.big_orders AS SELECT * FROM ekos_cat.orders WHERE total > 100;
CREATE MATERIALIZED VIEW ekos_cat.order_totals AS
  SELECT customer_id, sum(total) AS total FROM ekos_cat.orders GROUP BY customer_id;

CREATE FUNCTION ekos_cat.order_count(cid bigint) RETURNS bigint LANGUAGE sql STABLE AS
  $$ SELECT count(*) FROM ekos_cat.orders WHERE customer_id = cid $$;

CREATE FUNCTION ekos_cat.touch() RETURNS trigger LANGUAGE plpgsql AS
  $$ BEGIN RETURN NEW; END $$;

CREATE PROCEDURE ekos_cat.rebuild() LANGUAGE plpgsql AS
  $$ BEGIN REFRESH MATERIALIZED VIEW ekos_cat.order_totals; END $$;

CREATE TRIGGER orders_touch BEFORE INSERT ON ekos_cat.orders
  FOR EACH ROW EXECUTE FUNCTION ekos_cat.touch();

CREATE TABLE ekos_cat.secrets (id int PRIMARY KEY, owner text);
ALTER TABLE ekos_cat.secrets ENABLE ROW LEVEL SECURITY;
CREATE POLICY owner_only ON ekos_cat.secrets USING (owner = current_user);

CREATE TABLE ekos_cat.rooms (
    id int PRIMARY KEY,
    during ekos_cat.span,
    EXCLUDE USING gist (during WITH &&)
);
"#;

// ── session safety ───────────────────────────────────────────────────────────

#[test]
fn a_session_reports_whether_the_role_can_write() {
    if !live() {
        eprintln!("skipped: set EKOS_MIGRATE_LIVE=1 with the sandboxes up");
        return;
    }
    let src = connect();
    let check = src.check_write_refused().expect("write probe");
    // The sandbox's `ekos` role owns the database, so it *can* write. That is a finding, not a
    // failure — and it is the answer the probe must give: if it reported "safe" here, it would be
    // reporting on the session setting rather than on the role, which is the thing that actually
    // protects the source.
    assert_eq!(
        check,
        WriteCheck::RoleCanWrite,
        "the probe must see through the session's read-only setting to the role's privileges"
    );
    assert!(
        !check.is_safe(),
        "an owner role must not be reported as safe"
    );

    // Whatever the role can do, the probe leaves nothing behind.
    let rows = src
        .raw_query("SELECT count(*) FROM pg_class WHERE relname = 'ekos_migrate_write_probe'")
        .unwrap();
    assert_eq!(rows[0][0], "0", "the write probe must not persist");
}

#[test]
fn the_session_is_read_only_and_times_out() {
    if !live() {
        return;
    }
    let src = connect();
    let rows = src
        .raw_query(
            "SELECT current_setting('default_transaction_read_only'), \
                    current_setting('statement_timeout'), \
                    current_setting('lock_timeout'), \
                    current_setting('application_name')",
        )
        .unwrap();
    assert_eq!(rows[0][0], "on");
    assert_eq!(rows[0][1], "30s");
    assert_eq!(rows[0][2], "5s");
    assert_eq!(
        rows[0][3], "ekos-migrate/run-live-catalog",
        "application_name must carry the run id so a DBA can attribute the load"
    );
}

#[test]
fn a_write_through_the_read_only_session_is_refused() {
    if !live() {
        return;
    }
    let src = connect();
    let err = src
        .raw_query("CREATE TABLE ekos_should_not_exist (x int)")
        .unwrap_err();
    assert!(
        err.to_string().contains("read-only"),
        "expected a read-only refusal, got: {err}"
    );
}

#[test]
fn lsn_and_replica_lag_are_readable() {
    if !live() {
        return;
    }
    let src = connect();
    let lsn = src.current_lsn().unwrap();
    assert!(
        lsn.contains('/'),
        "an LSN looks like 0/1A2B3C4, got {lsn:?}"
    );
    // A primary has no replay lag.
    assert_eq!(src.replica_lag_seconds().unwrap(), None);
}

// ── catalog ──────────────────────────────────────────────────────────────────

#[test]
fn every_object_kind_is_recovered_and_the_counts_reconcile() {
    if !live() {
        return;
    }
    create_fixture();
    let src = connect();
    let schemas = vec![SCHEMA.to_string()];
    let snap = introspect(&src, &schemas, &RedactionConfig::default()).expect("introspect");

    // The reconciliation is the guard that matters: it is computed from a different query shape
    // than the enumeration, so a JOIN that silently drops rows fails here.
    reconcile(&src, &snap, &schemas).expect("catalog counts must reconcile");

    let counts = snap.counts();
    for kind in [
        ObjectKind::Schema,
        ObjectKind::Table,
        ObjectKind::PartitionedTable,
        ObjectKind::Partition,
        ObjectKind::View,
        ObjectKind::MaterializedView,
        ObjectKind::Column,
        ObjectKind::PrimaryKey,
        ObjectKind::ForeignKey,
        ObjectKind::UniqueConstraint,
        ObjectKind::CheckConstraint,
        ObjectKind::ExclusionConstraint,
        ObjectKind::Index,
        ObjectKind::Sequence,
        ObjectKind::Function,
        ObjectKind::Procedure,
        ObjectKind::Trigger,
        ObjectKind::Enum,
        ObjectKind::Domain,
        ObjectKind::RangeType,
    ] {
        assert!(
            counts.get(&kind).copied().unwrap_or(0) > 0,
            "no {} recovered — the fixture has one, so the introspector is short. Counts: {:?}",
            kind.as_str(),
            counts
        );
    }
}

#[test]
fn a_not_valid_foreign_key_is_recorded_as_not_a_guarantee() {
    if !live() {
        return;
    }
    create_fixture();
    let src = connect();
    let snap = introspect(&src, &[SCHEMA.to_string()], &RedactionConfig::default()).unwrap();
    let fk = snap
        .of_kind(ObjectKind::ForeignKey)
        .find(|o| o.qualified_name.ends_with("orders_customer_fk2"))
        .expect("the NOT VALID fk");
    assert_eq!(
        fk.detail["validated"], false,
        "a NOT VALID fk must not look like an enforced one — RFC 0158 needs the difference"
    );
    let valid = snap
        .of_kind(ObjectKind::ForeignKey)
        .find(|o| o.qualified_name.contains("customer_id_fkey"))
        .expect("the ordinary fk");
    assert_eq!(valid.detail["validated"], true);
}

/// The third raw-content entry point. A column default, a comment and a function body are all free
/// text from a real database, and all three go through RFC 0043's redaction before they become facts.
#[test]
fn secrets_in_the_catalog_are_redacted_before_they_become_facts() {
    if !live() {
        return;
    }
    create_fixture();
    let src = connect();
    let snap = introspect(&src, &[SCHEMA.to_string()], &RedactionConfig::default()).unwrap();

    let dump = serde_json::to_string(&snap).unwrap();
    assert!(
        !dump.contains("AKIAIOSFODNN7EXAMPLE"),
        "a key in a column default reached the catalog snapshot"
    );
    // And the column is still there — redaction must not drop the object.
    let col = snap
        .of_kind(ObjectKind::Column)
        .find(|c| c.qualified_name.ends_with(".api_key"))
        .expect("api_key column");
    assert!(
        col.detail.get("default").is_some(),
        "the default was dropped, not redacted"
    );
}

#[test]
fn a_function_body_is_carried_for_rfc_0163_and_redacted() {
    if !live() {
        return;
    }
    create_fixture();
    let src = connect();
    let snap = introspect(&src, &[SCHEMA.to_string()], &RedactionConfig::default()).unwrap();
    let f = snap
        .of_kind(ObjectKind::Function)
        .find(|o| o.qualified_name.contains("order_count"))
        .expect("order_count");
    assert_eq!(f.detail["language"], "sql");
    assert!(
        f.detail["body"].as_str().unwrap().contains("count(*)"),
        "the body must be carried — RFC 0163 parses it"
    );
    let p = snap
        .of_kind(ObjectKind::Procedure)
        .find(|o| o.qualified_name.contains("rebuild"))
        .expect("rebuild procedure");
    assert_eq!(p.detail["language"], "plpgsql");
}

#[test]
fn the_reader_seam_works_against_a_real_source() {
    if !live() {
        return;
    }
    create_fixture();
    let src = connect();
    // The same trait RFC 0156's tiers run over — so the tiers now have a real source, with no
    // shelling out.
    let rows = EngineReader::query(&src, "SELECT 1, 'two'").unwrap();
    assert_eq!(rows, vec![vec!["1".to_string(), "two".to_string()]]);
    assert!(EngineReader::label(&src).starts_with("postgres:"));
}

/// An unknown `relkind` must be an error, not a silently smaller catalog. Foreign tables are the
/// realistic case: a source using `postgres_fdw` has `relkind = 'f'`, and dropping those would
/// under-report the migration's scope.
#[test]
fn an_unhandled_relkind_would_fail_rather_than_shrink_the_catalog() {
    if !live() {
        return;
    }
    // Asserted structurally: the match in `catalog.rs` has no wildcard arm that skips.
    let src = include_str!("../src/catalog.rs");
    let relkind_match = src
        .split("let kind = match relkind {")
        .nth(1)
        .expect("relkind match")
        .split("};")
        .next()
        .unwrap();
    assert!(
        relkind_match.contains("UnknownRelkind"),
        "the relkind match must error on an unknown kind, not skip it"
    );
    assert!(
        !relkind_match.contains("=> continue"),
        "skipping an unknown relkind silently shrinks RFC 0158's denominator"
    );
}

/// The seam end to end: RFC 0156's tiers running over the real driver rather than a shell-out.
///
/// Source and target are both PostgreSQL here — enough to prove the tier stack works through
/// `PgSource`, which is what the driver decision was blocking. A real cross-engine run needs a
/// ClickHouse reader, and `ekos-pg-live` deliberately has no ClickHouse dependency.
#[test]
fn the_tiers_run_over_the_real_driver() {
    if !live() {
        return;
    }
    create_fixture();
    let src = connect();

    use ekos_migrate_validate::dialect::{self, ColumnRule, Dialect};
    use ekos_migrate_validate::tiers::{UnitPlan, run_v1, run_v3};

    let cols = vec![
        dialect::canon_expr(Dialect::Postgres, "id", ColumnRule::Int),
        dialect::canon_expr(Dialect::Postgres, "total", ColumnRule::Decimal(2)),
    ];
    let plan = UnitPlan {
        unit: "ekos_cat.orders".into(),
        source_table: "ekos_cat.orders".into(),
        target_table: "ekos_cat.orders".into(),
        source_pk: dialect::canon_expr(Dialect::Postgres, "id", ColumnRule::Int),
        target_pk: dialect::canon_expr(Dialect::Postgres, "id", ColumnRule::Int),
        source_columns: cols.clone(),
        target_columns: cols,
        buckets: 16,
    };

    // A table compared with itself must be silent at every tier. If the driver mangled a value —
    // rendering every integer as an empty string, say — this would still pass, because both sides
    // mangle identically. So the assertion below that the *values* are real is the load-bearing one.
    let v1 = run_v1(&plan, &src, &src).unwrap();
    assert!(v1.divergences.is_empty(), "{:?}", v1.divergences);
    let v3 = run_v3(&plan, &src, &src).unwrap();
    assert!(v3.divergences.is_empty(), "{:?}", v3.divergences);

    // The check that a self-comparison cannot give: real values, not empty strings.
    let rows = src
        .raw_query("SELECT count(*), 42::bigint, 1.5::numeric, true FROM ekos_cat.orders")
        .unwrap();
    assert_eq!(
        rows[0],
        vec!["0".to_string(), "42".into(), "1.5".into(), "t".into()],
        "non-text columns must render, not come back empty"
    );
}
