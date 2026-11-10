//! RFC 0252 §3.5 live suite against `docker-compose.sqlserver.yml` (repo root). Skipped unless
//! `EKOS_SQLSERVER_LIVE=1`. Needs the fixture loaded and two variables holding the fixture's
//! passwords: `EKOS_SQLSERVER_PW` (the read-only login `ekos_ro`) and `EKOS_SQLSERVER_SA_PW`
//! (`sa`, used only to show that a writable login is reported and that a write is rolled back).
//! `EKOS_SQLSERVER_CONNECT` overrides the address
//! (default `127.0.0.1:51433/shop?trust-server-certificate=true`).

use ekos_common::redaction::RedactionConfig;
use ekos_plugin_sqlserver::session::Session;
use ekos_plugin_sqlserver::{ProfileTier, SqlServerConnection, probe::SqlServerProbe, read};
use ekos_sql_dialect_sdk::probe::Probe;

fn live() -> bool {
    std::env::var("EKOS_SQLSERVER_LIVE").as_deref() == Ok("1")
}

fn connect_string() -> String {
    std::env::var("EKOS_SQLSERVER_CONNECT")
        .unwrap_or_else(|_| "127.0.0.1:51433/shop?trust-server-certificate=true".into())
}

fn reader() -> SqlServerConnection {
    SqlServerConnection::parse(&connect_string(), "ekos_ro", "EKOS_SQLSERVER_PW").unwrap()
}

fn admin() -> SqlServerConnection {
    SqlServerConnection::parse(&connect_string(), "sa", "EKOS_SQLSERVER_SA_PW").unwrap()
}

fn schemas() -> Vec<String> {
    vec!["sales".into(), "hr".into()]
}

fn count(session: &mut Session, sql: &str) -> i64 {
    session.rows(sql).unwrap()[0][0]
        .as_deref()
        .unwrap()
        .parse()
        .unwrap()
}

#[test]
fn the_catalog_and_a_p2_profile_match_direct_queries() {
    if !live() {
        eprintln!("skipped: EKOS_SQLSERVER_LIVE is not 1");
        return;
    }
    let r = read(
        &reader(),
        &schemas(),
        "live-test",
        &RedactionConfig::default(),
        ProfileTier::P2,
    )
    .unwrap();
    let sales = r.catalogs.iter().find(|c| c.owner == "sales").unwrap();
    let hr = r.catalogs.iter().find(|c| c.owner == "hr").unwrap();
    let listed = sales.listed();
    assert_eq!(listed["table"], 5);
    assert_eq!(
        (
            listed["view"],
            listed["procedure"],
            listed["function"],
            listed["trigger"]
        ),
        (1, 1, 1, 1)
    );
    assert_eq!(listed["foreign_key"], 3);
    assert_eq!(hr.listed()["table"], 1);
    assert_eq!(hr.listed()["foreign_key"], 1, "a self-reference");

    // Every count against a direct query on a separate session.
    let mut direct = Session::connect(&admin(), "live-test-direct").unwrap();
    for (table, expected_rows) in [("customers", 3), ("orders", 4), ("order_lines", 4)] {
        let n = count(
            &mut direct,
            &format!("SELECT COUNT_BIG(*) FROM sales.{table}"),
        );
        assert_eq!(n, expected_rows);
        let t = sales.objects.iter().find(|o| o.name == table).unwrap();
        assert_eq!(t.detail["profile"]["row_count"], n, "{table}");
        assert_eq!(t.detail["profile"]["row_count_exact"], true);
    }
    let customers = sales
        .objects
        .iter()
        .find(|o| o.name == "customers")
        .unwrap();
    assert_eq!(
        customers.detail["comment"],
        "People and companies who buy from the shop"
    );
    let col = |name: &str| {
        customers.detail["columns"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == name)
            .unwrap()
            .clone()
    };
    let nulls = count(
        &mut direct,
        "SELECT COUNT_BIG(*) FROM sales.customers WHERE email IS NULL",
    );
    assert_eq!(col("email")["profile"]["nulls"], nulls);
    assert_eq!(col("email")["profile"]["max_length"], 16);
    assert!(
        col("email")["profile"].get("min").is_none(),
        "never a text value"
    );
    assert_eq!(col("name")["comment"], "Shown on invoices");
    assert_eq!(col("id")["identity"], "IDENTITY");
    let orders = sales.objects.iter().find(|o| o.name == "orders").unwrap();
    let placed = orders.detail["columns"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "placed_at")
        .unwrap();
    assert_eq!(placed["profile"]["min"], "2026-01-05T10:00:00");
    assert_eq!(placed["profile"]["max"], "2026-04-02T12:00:00");
    let stats = sales.profile.as_ref().unwrap();
    assert_eq!((stats.tables_profiled, stats.tables_failed), (5, 0));
    assert_eq!(
        r.write_check.as_ref().unwrap()["result"],
        "refused_by_server"
    );
    let proc = sales
        .objects
        .iter()
        .find(|o| o.kind == "procedure")
        .unwrap();
    assert!(
        proc.detail["text"]
            .as_str()
            .unwrap()
            .contains("UPDATE sales.orders")
    );
    assert_eq!(
        proc.detail["comment"],
        "Cancels an order and drops its lines"
    );
}

#[test]
fn a_writable_login_is_reported_and_a_write_never_stays() {
    if !live() {
        eprintln!("skipped: EKOS_SQLSERVER_LIVE is not 1");
        return;
    }
    let r = read(
        &admin(),
        &schemas(),
        "live-test",
        &RedactionConfig::default(),
        ProfileTier::P0,
    )
    .unwrap();
    assert_eq!(r.write_check.as_ref().unwrap()["result"], "user_can_write");
    // P0: estimates only.
    let sales = r.catalogs.iter().find(|c| c.owner == "sales").unwrap();
    let orders = sales.objects.iter().find(|o| o.name == "orders").unwrap();
    assert_eq!(orders.detail["profile"]["row_count_exact"], false);

    // Every statement runs in a transaction that is rolled back, even one that writes.
    let mut s = Session::connect(&admin(), "live-test-write").unwrap();
    let before = count(&mut s, "SELECT COUNT_BIG(*) FROM sales.order_log");
    s.query("INSERT INTO sales.order_log (order_id) VALUES (999)")
        .unwrap();
    let after = count(&mut s, "SELECT COUNT_BIG(*) FROM sales.order_log");
    assert_eq!(before, after, "the insert was rolled back");

    let refused = SqlServerProbe::connect(&admin(), &schemas(), "live-test", false)
        .err()
        .unwrap();
    assert!(refused.contains("can write"), "{refused}");
}

#[test]
fn the_probe_previews_a_select_and_refuses_anything_else() {
    if !live() {
        eprintln!("skipped: EKOS_SQLSERVER_LIVE is not 1");
        return;
    }
    let p = SqlServerProbe::connect(&reader(), &schemas(), "live-test", false).unwrap();
    let preview = p
        .preview("SELECT id, status FROM sales.orders ORDER BY id", 2)
        .unwrap();
    assert_eq!(preview.columns, ["id", "status"]);
    assert_eq!(preview.rows.len(), 2);
    assert!(preview.truncated);
    assert_eq!(preview.rows[0][0].as_deref(), Some("1"));
    let e = p.preview("DELETE FROM sales.orders", 2).unwrap_err();
    assert!(e.starts_with("not sent"), "{e}");
}
