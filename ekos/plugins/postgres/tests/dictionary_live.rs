//! RFC 0256 live: iDempiere's application dictionary read from PostgreSQL, as a read-only role.
//!
//! Runs only with `EKOS_IDEMPIERE_PG_LIVE=1` against an iDempiere database on PostgreSQL:
//! `EKOS_IDEMPIERE_PG_CONNECT` (`host:port/database`, default `127.0.0.1:56544/idempiere`),
//! `EKOS_IDEMPIERE_PG_USER` (default `ekos_ro`), the password in `EKOS_IDEMPIERE_PG_PASSWORD`, and
//! `EKOS_IDEMPIERE_PG_SCHEMA` (default `adempiere`). Reads only.

use ekos_common::redaction::RedactionConfig;
use ekos_observation_sdk::app_dictionary::DictionaryKind;
use ekos_observation_sdk::profile::ProfileTier;
use ekos_plugin_postgres::{PgConnection, read_with};

fn live() -> bool {
    std::env::var("EKOS_IDEMPIERE_PG_LIVE").is_ok_and(|v| v == "1")
}

fn env_or(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.into())
}

fn connection() -> PgConnection {
    PgConnection::parse(
        &env_or("EKOS_IDEMPIERE_PG_CONNECT", "127.0.0.1:56544/idempiere"),
        &env_or("EKOS_IDEMPIERE_PG_USER", "ekos_ro"),
        "EKOS_IDEMPIERE_PG_PASSWORD",
    )
    .unwrap()
}

/// One number from a read-only session: what the dictionary reader must agree with.
fn count(sql: &str) -> u64 {
    let c = connection();
    let conn = ekos_migrate::ConnectionRef {
        engine: ekos_migrate::EngineKind::Postgres,
        alias: c.host.clone(),
        database: c.database.clone(),
        secret_env: Some(c.password_env.clone()),
        secret_file: None,
    };
    let s = ekos_pg_live::PgSource::connect(
        &conn,
        &c.host,
        c.port,
        &c.user,
        "dictionary-live",
        &Default::default(),
    )
    .unwrap();
    s.preview(sql, 1).unwrap().rows[0][0]
        .as_deref()
        .unwrap()
        .parse()
        .unwrap()
}

#[test]
fn the_dictionary_on_postgres_matches_a_direct_count_and_the_role_cannot_write() {
    if !live() {
        eprintln!("skipped: set EKOS_IDEMPIERE_PG_LIVE=1 with an iDempiere PostgreSQL");
        return;
    }
    let schema = env_or("EKOS_IDEMPIERE_PG_SCHEMA", "adempiere");
    let reading = read_with(
        &connection(),
        std::slice::from_ref(&schema),
        "dictionary-live",
        &RedactionConfig::default(),
        ProfileTier::P0,
        Some(DictionaryKind::Idempiere),
    )
    .unwrap();
    assert_eq!(
        reading.write_check.as_ref().unwrap()["result"],
        "refused_by_server",
        "a read-only role"
    );
    let (flags, processes) = &reading.dictionaries[&schema];
    let n = |what: &str| count(&format!("SELECT count(*) FROM \"{schema}\".{what}"));
    let deleteable = flags.iter().filter(|f| f.deleteable == Some(true)).count() as u64;
    let logged = flags.iter().filter(|f| f.change_log == Some(true)).count() as u64;
    let schedules: u64 = processes.iter().map(|p| u64::from(p.schedules)).sum();
    eprintln!(
        "dictionary on postgres: {} tables ({deleteable} deletable, {logged} change-logged), \
         {} processes ({} procedures, {} classes), {schedules} active schedules",
        flags.len(),
        processes.len(),
        processes
            .iter()
            .filter(|p| p.procedure_name.is_some())
            .count(),
        processes.iter().filter(|p| p.classname.is_some()).count(),
    );
    assert_eq!(flags.len() as u64, n("ad_table"));
    assert_eq!(deleteable, n("ad_table WHERE isdeleteable = 'Y'"));
    assert_eq!(logged, n("ad_table WHERE ischangelog = 'Y'"));
    assert_eq!(
        processes.len() as u64,
        n("ad_process WHERE procedurename IS NOT NULL OR classname IS NOT NULL")
    );
    assert_eq!(
        schedules,
        n(&format!(
            "ad_scheduler s JOIN \"{schema}\".ad_process p USING (ad_process_id) \
             WHERE s.isactive = 'Y' AND (p.procedurename IS NOT NULL OR p.classname IS NOT NULL)"
        ))
    );
    assert!(flags.iter().any(|f| f.table_name == "C_Order"));
}
