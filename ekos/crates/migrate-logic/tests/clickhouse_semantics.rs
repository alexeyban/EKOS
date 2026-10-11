//! The engine differences `lower` rewrites for, measured on a real ClickHouse.
//!
//! The unit tests prove the lowering is deterministic and that the target grammar reads back what
//! was written. They cannot prove ClickHouse *behaves* the way the rewrite assumes — that `sum`
//! over no rows is `0`, that an outer join fills with defaults, that a `SELECT` alias shadows a
//! column in `WHERE`. This runs each assumption, and two lowered views, through `clickhouse-local`.
//!
//! Live, so off by default, like the other Migrate suites:
//! `EKOS_MIGRATE_LIVE=1 cargo test -p ekos-migrate-logic --test clickhouse_semantics`.
//! Needs Docker and the image named below (the one `docker-compose.migrate.yml` pins).

use std::io::Write;
use std::process::{Command, Stdio};

const IMAGE: &str = "clickhouse/clickhouse-server:24.8-alpine";

#[test]
fn clickhouse_behaves_the_way_the_lowering_assumes() {
    if std::env::var("EKOS_MIGRATE_LIVE").as_deref() != Ok("1") {
        eprintln!("skipped: set EKOS_MIGRATE_LIVE=1 to run against {IMAGE}");
        return;
    }
    let mut child = Command::new("docker")
        .args([
            "run",
            "--rm",
            "-i",
            IMAGE,
            "clickhouse-local",
            "--multiquery",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("docker is required for the live suite");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(include_bytes!("clickhouse_semantics.sql"))
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        include_str!("clickhouse_semantics.expected")
    );
}
