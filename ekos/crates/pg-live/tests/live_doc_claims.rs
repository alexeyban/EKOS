//! RFC 0172 Phase 3 (RFC 0158 `DQ.CONSIST.DOC`) — documented claims checked against a real
//! PostgreSQL: the claims read from real column comments, and every generated count query run for
//! real (quoting, `::text` on `char`/`int`, numeric bounds), with the counts it must return.
//!
//! ```text
//! docker compose -f docker-compose.migrate.yml up -d
//! EKOS_MIGRATE_LIVE=1 cargo test -p ekos-pg-live --test live_doc_claims
//! ```

use ekos_common::redaction::RedactionConfig;
use ekos_migrate::ConnectionRef;
use ekos_migrate_dq::doc_claims::extract;
use ekos_pg_live::catalog::ObjectKind;
use ekos_pg_live::{PgSource, SessionPolicy};

const SCHEMA: &str = "ekos_docclaims";
const TABLE: &str = "ekos_docclaims.invoice";

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
        "run-live-doc-claims",
        &SessionPolicy::default(),
    )
    .expect("connect")
}

fn fixture() {
    let ddl = format!(
        r#"
DROP SCHEMA IF EXISTS {SCHEMA} CASCADE;
CREATE SCHEMA {SCHEMA};
CREATE TABLE {TABLE} (
  id int PRIMARY KEY,
  "Inv No" text,
  status text,
  discount numeric(5,2),
  qty int,
  category char(1),
  ref text NOT NULL,
  note text
);
COMMENT ON COLUMN {TABLE}."Inv No" IS 'Invoice number. Never null, unique.';
COMMENT ON COLUMN {TABLE}.status IS 'One of open, closed or void.';
COMMENT ON COLUMN {TABLE}.discount IS 'Discount percentage between 0 and 100';
COMMENT ON COLUMN {TABLE}.qty IS 'Quantity, always positive';
COMMENT ON COLUMN {TABLE}.category IS 'A=asset,L=liability';
COMMENT ON COLUMN {TABLE}.ref IS 'External reference, never null';
COMMENT ON COLUMN {TABLE}.note IS 'Free text notes';
INSERT INTO {TABLE} VALUES
 (1,'INV-1','open',10,1,'A','r1','x'),
 (2,'INV-2','closed',0,2,'L','r2',NULL),
 (3,NULL,'void',5,3,'A','r3',NULL),
 (4,'INV-4','draft',150,0,'X','r4',NULL),
 (5,'INV-4','open',20,5,'L','r5',NULL),
 (6,'INV-6','open',100.00,1,'A','r6',NULL),
 (7,'INV-7','open',NULL,NULL,NULL,'r7',NULL);
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
}

#[test]
fn documented_claims_are_counted_against_the_data() {
    if !live() {
        eprintln!("skipped: set EKOS_MIGRATE_LIVE=1 with docker-compose.migrate.yml up");
        return;
    }
    fixture();
    let src = connect();
    let catalog =
        ekos_pg_live::introspect(&src, &[SCHEMA.to_string()], &RedactionConfig::default())
            .expect("introspect");

    let prefix = format!("{TABLE}.");
    let mut counts: Vec<(String, &'static str, i64)> = Vec::new();
    for col in catalog
        .of_kind(ObjectKind::Column)
        .filter(|c| c.qualified_name.starts_with(&prefix))
    {
        let column = &col.qualified_name[prefix.len()..];
        let ty = col.detail["type"].as_str().unwrap_or_default();
        let Some(comment) = col.detail["comment"].as_str() else {
            continue;
        };
        for claim in extract(comment) {
            let Some(sql) = claim.claim.violation_sql(TABLE, column, ty) else {
                continue;
            };
            let rows = src.raw_query(&sql).unwrap_or_else(|e| panic!("{sql}: {e}"));
            let n: i64 = rows[0][0].trim().parse().unwrap();
            counts.push((column.to_string(), claim.claim.key(), n));
        }
    }
    counts.sort();
    let expected: Vec<(String, &'static str, i64)> = vec![
        ("Inv No".into(), "not_null", 1),
        ("Inv No".into(), "unique", 1),
        ("category".into(), "one_of", 1), // 'X'; NULL is not a violation
        ("discount".into(), "range", 1),  // 150; 100.00 is inside the inclusive bound
        ("qty".into(), "range", 1),       // 0 is not positive
        ("ref".into(), "not_null", 0),    // declared NOT NULL: the data cannot disagree
        ("status".into(), "one_of", 1),   // 'draft'
    ];
    assert_eq!(counts, expected);
}
