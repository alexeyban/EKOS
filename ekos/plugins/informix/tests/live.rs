//! RFC 0252 §3.5 live suite against `docker-compose.informix.yml` (repo root). Skipped unless
//! `EKOS_INFORMIX_LIVE=1`, and built only with the crate feature `informix`:
//!
//! ```text
//! EKOS_INFORMIX_LIVE=1 EKOS_INFORMIX_PW=… EKOS_INFORMIX_ADMIN_PW=… \
//!   cargo test -p ekos-plugin-informix --features informix --test live
//! ```
//!
//! Needs the fixture loaded, an Informix-capable ODBC driver, and two variables holding the
//! fixture's passwords: `EKOS_INFORMIX_PW` (the read-only user `ekos_ro`) and
//! `EKOS_INFORMIX_ADMIN_PW` (`informix`, the database's DBA, used only to count rows directly, to
//! show that a writable user is reported, and that a write is not kept). `EKOS_INFORMIX_CONNECT`
//! sets the address (default `127.0.0.1:59088/shop@informix`, the Client SDK over SQLI; for the
//! IBM Data Server Driver over DRDA, `drda://127.0.0.1:59089/shop?driver=<path>/libdb2.so`).
#![cfg(feature = "informix")]

use ekos_common::redaction::RedactionConfig;
use ekos_plugin_informix::session::Session;
use ekos_plugin_informix::{InformixConnection, ProfileTier, probe::InformixProbe, read};
use ekos_sql_dialect_sdk::probe::Probe;

fn live() -> bool {
    std::env::var("EKOS_INFORMIX_LIVE").as_deref() == Ok("1")
}

fn connect_string() -> String {
    std::env::var("EKOS_INFORMIX_CONNECT")
        .unwrap_or_else(|_| "127.0.0.1:59088/shop@informix".into())
}

fn reader() -> InformixConnection {
    InformixConnection::parse(&connect_string(), "ekos_ro", "EKOS_INFORMIX_PW").unwrap()
}

fn admin() -> InformixConnection {
    InformixConnection::parse(&connect_string(), "informix", "EKOS_INFORMIX_ADMIN_PW").unwrap()
}

fn owners() -> Vec<String> {
    vec!["sales".into(), "hr".into()]
}

fn count(session: &mut Session, sql: &str) -> i64 {
    session.rows(sql).unwrap()[0][0]
        .as_deref()
        .unwrap()
        .trim()
        .parse()
        .unwrap()
}

fn text(session: &mut Session, sql: &str) -> Option<String> {
    session.rows(sql).unwrap()[0][0].clone()
}

#[test]
fn the_catalog_and_a_p2_profile_match_direct_queries() {
    if !live() {
        eprintln!("skipped: EKOS_INFORMIX_LIVE is not 1");
        return;
    }
    let r = read(
        &reader(),
        &owners(),
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
    assert_eq!(listed["primary_key"], 4);
    assert_eq!(
        listed["unique_constraint"], 2,
        "uq_customers_email + ux_orders_ref"
    );
    assert_eq!(listed["check_constraint"], 2);
    assert_eq!(hr.listed()["table"], 1);
    assert_eq!(hr.listed()["foreign_key"], 1, "a self-reference");

    let obj = |name: &str| sales.objects.iter().find(|o| o.name == name).unwrap();
    // Types decoded from coltype / collength.
    let customers = &obj("customers").detail;
    let col = |t: &serde_json::Value, n: usize| t["columns"][n].clone();
    assert_eq!(col(customers, 0)["data_type"], "serial");
    assert_eq!(col(customers, 0)["identity"], "SERIAL");
    assert_eq!(col(customers, 4)["data_type"], "datetime year to second");
    assert_eq!(col(customers, 4)["default"], "CURRENT");
    let products = &obj("products").detail;
    assert_eq!(col(products, 1)["data_type"], "lvarchar(200)");
    assert_eq!(col(products, 2)["data_type"], "decimal(10,2)");
    assert_eq!(col(products, 3)["data_type"], "boolean");
    let lines_fk = obj("order_lines.fk_lines_orders");
    assert_eq!(lines_fk.detail["columns"], serde_json::json!(["order_id"]));
    assert_eq!(
        lines_fk.detail["references_columns"],
        serde_json::json!(["id"])
    );
    assert_eq!(lines_fk.detail["on_delete"], "CASCADE");
    let pk = obj("order_lines.pk_order_lines");
    assert_eq!(
        pk.detail["columns"],
        serde_json::json!(["order_id", "line_no"])
    );
    let ck = obj("products.ck_products_price");
    assert!(
        ck.detail["condition"]
            .as_str()
            .unwrap()
            .contains("price >= 0")
    );
    let view = obj("big_orders");
    assert!(
        view.detail["text"]
            .as_str()
            .unwrap()
            .to_lowercase()
            .contains("having (sum("),
        "{}",
        view.detail["text"]
    );
    let p = obj("close_order");
    assert_eq!(p.detail["comment"], "Closes one order and logs it");
    assert!(p.detail["text"].as_str().unwrap().contains("order_log"));
    let t = obj("orders_audit");
    assert_eq!(t.detail["table"], "orders");
    assert!(t.detail["text"].as_str().unwrap().contains("order_log"));

    // Every count and bound against a direct query on a separate session.
    let mut direct = Session::connect(&admin(), "live-test-direct").unwrap();
    for (table, expected_rows) in [
        ("customers", 3),
        ("orders", 4),
        ("order_lines", 4),
        ("products", 3),
    ] {
        let n = count(
            &mut direct,
            &format!("SELECT COUNT(*) FROM \"sales\".{table}"),
        );
        assert_eq!(n, expected_rows);
        let p = &obj(table).detail["profile"];
        assert_eq!(p["row_count"], n, "{table}");
        assert_eq!(p["row_count_exact"], true, "{table}: {p}");
    }
    let orders = &obj("orders").detail["columns"];
    let nulls = count(
        &mut direct,
        "SELECT COUNT(*) FROM \"sales\".orders WHERE note IS NULL",
    );
    assert_eq!(orders[4]["profile"]["nulls"], nulls);
    let longest = count(
        &mut direct,
        "SELECT MAX(LENGTH(note)) FROM \"sales\".orders",
    );
    assert_eq!(orders[4]["profile"]["max_length"], longest);
    assert!(orders[4]["profile"].get("min").is_none(), "no text bound");
    let placed_max = text(
        &mut direct,
        "SELECT CAST(EXTEND(MAX(placed_at), YEAR TO FRACTION(5)) AS LVARCHAR(100)) \
         FROM \"sales\".orders",
    );
    assert_eq!(orders[3]["profile"]["max"], placed_max.unwrap().trim());
    assert!(
        orders[3]["profile"]["max"]
            .as_str()
            .unwrap()
            .starts_with("2026-10-01 08:00:00")
    );
    // A DATE renders as ISO 8601 whatever DBDATE says.
    let hired = &hr
        .objects
        .iter()
        .find(|o| o.name == "employees")
        .unwrap()
        .detail["columns"][3];
    assert_eq!(hired["profile"]["min"], "2019-02-01 00:00:00.00000");
    // birth_date is personal data: counted, never bounded.
    let birth = &customers["columns"][3]["profile"];
    assert_eq!(birth["values_suppressed"], true);
    assert!(birth.get("min").is_none());

    // The read-only user is reported as such, the DBA as able to write.
    assert_eq!(
        r.write_check.as_ref().unwrap()["result"],
        "refused_by_server"
    );
    let w = read(
        &admin(),
        &owners(),
        "live-test-admin",
        &RedactionConfig::default(),
        ProfileTier::P0,
    )
    .unwrap();
    let wc = w.write_check.unwrap();
    assert_eq!(wc["result"], "user_can_write");
    assert_eq!(wc["dba"], true);
    eprintln!("session guards: {}", wc["session"]);
}

#[test]
fn a_write_in_the_read_only_session_is_not_kept() {
    if !live() {
        eprintln!("skipped: EKOS_INFORMIX_LIVE is not 1");
        return;
    }
    // As the DBA, who may write: the session's own locks are what stop it.
    let mut s = Session::connect(&admin(), "live-test-write").unwrap();
    let before = count(&mut s, "SELECT COUNT(*) FROM \"sales\".order_log");
    let attempt = s.rows("INSERT INTO \"sales\".order_log (order_id) VALUES (99)");
    eprintln!("guards: {:?}; insert: {attempt:?}", s.guards());
    let mut other = Session::connect(&admin(), "live-test-write-check").unwrap();
    let after = count(&mut other, "SELECT COUNT(*) FROM \"sales\".order_log");
    assert_eq!(before, after, "the insert was refused or rolled back");
}

#[test]
fn the_probe_previews_a_query_and_refuses_a_write() {
    if !live() {
        eprintln!("skipped: EKOS_INFORMIX_LIVE is not 1");
        return;
    }
    let p = InformixProbe::connect(&reader(), &owners(), "live-test-probe", false).unwrap();
    let r = p
        .preview(
            "SELECT c.name, COUNT(*) AS n FROM \"sales\".customers c \
             JOIN \"sales\".orders o ON o.customer_id = c.id GROUP BY c.name ORDER BY n DESC",
            1,
        )
        .unwrap();
    assert_eq!(r.rows.len(), 1);
    assert!(
        r.truncated,
        "two customers have orders, one row was asked for"
    );
    assert_eq!(r.rows[0][1].as_deref().map(str::trim), Some("2"));
    assert!(
        p.preview("DELETE FROM \"sales\".orders", 5)
            .unwrap_err()
            .starts_with("not sent")
    );
    let refused = InformixProbe::connect(&admin(), &owners(), "live-test-probe-admin", false);
    assert!(refused.err().unwrap().contains("can write"));
}
