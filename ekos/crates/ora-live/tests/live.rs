//! RFC 0211 live suite — runs only with `EKOS_ORACLE_LIVE=1` and the `oracle` feature, against
//! `docker-compose.oracle.yml` after `deploy/fixtures/idempiere-oracle.sh`:
//!
//! ```text
//! LD_LIBRARY_PATH=<instant client> EKOS_ORACLE_LIVE=1 EKOS_RO_PASSWORD=ekos-ro-local-only \
//!   IDEMPIERE_DB_PASSWORD=adempiere \
//!   cargo test -p ekos-ora-live --features oracle --test live -- --nocapture --test-threads 1
//! ```
#![cfg(feature = "oracle")]

use ekos_common::redaction::RedactionConfig;
use ekos_ora_live::{
    ObjectKind, OraConnection, OraSource, ProfileTier, SessionPolicy, ViewFamily, WriteCheck,
    introspect, profile,
};

fn live() -> bool {
    std::env::var("EKOS_ORACLE_LIVE").is_ok_and(|v| v == "1")
}

fn connect_string() -> String {
    std::env::var("EKOS_ORACLE_CONNECT").unwrap_or_else(|_| "localhost:51521/XEPDB1".into())
}

fn read_only() -> OraSource {
    let c = OraConnection {
        connect_string: connect_string(),
        user: "ekos_ro".into(),
        password_env: "EKOS_RO_PASSWORD".into(),
    };
    OraSource::connect(&c, "live-suite", &SessionPolicy::default()).expect("connect as ekos_ro")
}

#[test]
fn the_session_guards_are_in_force() {
    if !live() {
        return;
    }
    let s = read_only();
    let nls = s
        .rows_of(
            "SELECT parameter, value FROM nls_session_parameters \
             WHERE parameter IN ('NLS_DATE_FORMAT', 'NLS_NUMERIC_CHARACTERS') ORDER BY 1",
            &[],
        )
        .unwrap();
    assert_eq!(nls[0][1].as_deref(), Some("YYYY-MM-DD\"T\"HH24:MI:SS"));
    assert_eq!(nls[1][1].as_deref(), Some(".,"));
    let tz = s.rows_of("SELECT sessiontimezone FROM dual", &[]).unwrap();
    assert_eq!(tz[0][0].as_deref(), Some("UTC"));
    let who = s
        .rows_of(
            "SELECT module, action FROM v$session WHERE sid = SYS_CONTEXT('USERENV', 'SID')",
            &[],
        )
        .unwrap();
    assert_eq!(who[0][0].as_deref(), Some("ekos"));
    assert_eq!(who[0][1].as_deref(), Some("live-suite"));
}

#[test]
fn the_read_only_user_is_refused_a_write_and_the_schema_owner_is_not() {
    if !live() {
        return;
    }
    let ro = read_only()
        .check_write_refused("ADEMPIERE", "C_INVOICE")
        .unwrap();
    assert!(ro.is_safe(), "{ro:?}");
    let owner = OraSource::connect(
        &OraConnection {
            connect_string: connect_string(),
            user: "adempiere".into(),
            password_env: "IDEMPIERE_DB_PASSWORD".into(),
        },
        "live-suite",
        &SessionPolicy::default(),
    )
    .unwrap();
    assert_eq!(
        owner.check_write_refused("ADEMPIERE", "C_INVOICE").unwrap(),
        WriteCheck::UserCanWrite
    );
    // Inside a read-only transaction even the owner is refused, with ORA-01456 rather than 01031.
    owner.begin_read_only().unwrap();
    let err = owner
        .check_write_refused("ADEMPIERE", "C_INVOICE")
        .unwrap_err();
    assert!(err.to_string().contains("ORA-01456"), "{err}");
    owner.end_transaction().unwrap();
    let privs = read_only().session_privileges().unwrap();
    assert!(privs.iter().all(|p| !p.contains("ANY TABLE")), "{privs:?}");
}

#[test]
fn the_catalog_reconciles_against_the_dictionary() {
    if !live() {
        return;
    }
    let s = read_only();
    assert_eq!(
        s.family(),
        ViewFamily::Dba,
        "ekos_ro holds SELECT_CATALOG_ROLE"
    );
    let t = std::time::Instant::now();
    let snap = introspect(&s, "ADEMPIERE", s.family(), &RedactionConfig::default()).unwrap();
    println!("introspected in {:?}", t.elapsed());
    println!("census: {:?}", snap.census);
    println!("listed: {:?}", snap.counts());
    println!(
        "columns {}, folded not-null checks {}, other constraints {:?}",
        snap.columns, snap.folded_not_null, snap.other_constraints
    );
    let truncated = snap
        .of_kind(ObjectKind::View)
        .filter(|v| v.detail.get("text_truncated").is_some())
        .count();
    println!("views with truncated text: {truncated}");
    assert!(
        snap.of_kind(ObjectKind::Table)
            .any(|t| t.name == "C_INVOICE")
    );
}

#[test]
fn an_all_family_read_shrinks_list_and_count_together() {
    if !live() {
        return;
    }
    // ALL_OBJECTS and ALL_SOURCE both hide routines the user may not execute, so an ALL_* snapshot
    // reconciles with itself and is still short. This is why the snapshot records its family and
    // DBA_* is preferred whenever SELECT_CATALOG_ROLE allows it.
    let s = read_only();
    let all = introspect(
        &s,
        "ADEMPIERE",
        ViewFamily::All,
        &RedactionConfig::default(),
    )
    .unwrap();
    let dba = introspect(
        &s,
        "ADEMPIERE",
        ViewFamily::Dba,
        &RedactionConfig::default(),
    )
    .unwrap();
    let functions = |snap: &ekos_ora_live::CatalogSnapshot| {
        snap.counts()
            .get(&ObjectKind::Function)
            .copied()
            .unwrap_or(0)
    };
    println!("functions: all {} dba {}", functions(&all), functions(&dba));
    assert_eq!(all.family, ViewFamily::All);
    assert!(functions(&all) < functions(&dba));
    assert_eq!(
        all.of_kind(ObjectKind::Table).count(),
        dba.of_kind(ObjectKind::Table).count(),
        "SELECT on every table keeps the tables visible"
    );
}

#[test]
fn long_columns_read_in_full() {
    if !live() {
        return;
    }
    let s = read_only();
    let rows = s
        .rows_of(
            "SELECT view_name, text_length, text FROM dba_views WHERE owner = :1 \
             AND text_length > 4000 ORDER BY text_length DESC FETCH FIRST 1 ROWS ONLY",
            &["ADEMPIERE"],
        )
        .unwrap();
    let len: usize = rows[0][1].as_deref().unwrap().parse().unwrap();
    let text = rows[0][2].as_deref().unwrap();
    println!(
        "{} declares {len}, read {}",
        rows[0][0].as_deref().unwrap(),
        text.len()
    );
    assert_eq!(text.len(), len);
    let d = s
        .rows_of(
            "SELECT data_default FROM dba_tab_columns WHERE owner = :1 \
             AND table_name = 'C_INVOICE' AND column_name = 'ISACTIVE'",
            &["ADEMPIERE"],
        )
        .unwrap();
    println!("default {:?}", d[0][0]);
}

/// A plain driver connection as the schema owner, for seeding only. `OraSource` has no way to
/// write, and should not gain one for a test.
fn schema_owner() -> oracle::Connection {
    let password = std::env::var("IDEMPIERE_DB_PASSWORD").expect("IDEMPIERE_DB_PASSWORD");
    oracle::Connection::connect("adempiere", password, connect_string())
        .expect("connect as the schema owner")
}

const MARKERS: &[&str] = &[
    "ekos-marker-name-5f1c",
    "ekos-marker-password-9b2e",
    "ekos-marker-user-41d7",
    "https://ekos-marker-url-c08a.example",
    "/ekos-marker-folder-77e3",
];

/// Removes the marker row however the test ends.
struct MarkerRow(oracle::Connection);

impl Drop for MarkerRow {
    fn drop(&mut self) {
        let _ = self.0.execute(
            "DELETE FROM adempiere.ad_storageprovider WHERE ad_storageprovider_id = 999999",
            &[],
        );
        let _ = self.0.commit();
    }
}

/// RFC 0211 §5's gate: after the most invasive profile (P2), nothing that leaves the crate — the
/// snapshot, the profile counts, the log state — holds a text value of a seeded row.
#[test]
fn a_p2_profile_holds_no_text_value_of_a_seeded_row() {
    if !live() {
        return;
    }
    let owner = schema_owner();
    owner
        .execute(
            &format!(
                "INSERT INTO adempiere.ad_storageprovider (ad_storageprovider_id, ad_client_id, \
             ad_org_id, createdby, updatedby, name, password, username, url, folder) \
             VALUES (999999, 0, 0, 100, 100, '{}', '{}', '{}', '{}', '{}')",
                MARKERS[0], MARKERS[1], MARKERS[2], MARKERS[3], MARKERS[4]
            ),
            &[],
        )
        .unwrap();
    owner.commit().unwrap();
    let _cleanup = MarkerRow(owner);

    let s = read_only();
    let mut snap = introspect(&s, "ADEMPIERE", s.family(), &RedactionConfig::default()).unwrap();
    let t = std::time::Instant::now();
    let stats = profile::scan(&s, &mut snap, ProfileTier::P2, &RedactionConfig::default()).unwrap();
    println!("P2 in {:?}: {stats:?}", t.elapsed());
    let log = profile::log_state(&s, "ADEMPIERE");
    println!("log state: {log}");

    let out = serde_json::to_string(&(&snap, &stats, &log)).unwrap();
    for m in MARKERS {
        assert!(!out.contains(m), "{m} reached the snapshot");
    }
    // The row was seen: counted, and its text measured only by length.
    let table = snap
        .of_kind(ObjectKind::Table)
        .find(|t| t.name == "AD_STORAGEPROVIDER")
        .unwrap();
    assert_eq!(table.detail["profile"]["row_count"], 1);
    let password = table.detail["columns"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "PASSWORD")
        .unwrap();
    assert_eq!(password["profile"]["nulls"], 0);
    assert_eq!(password["profile"]["max_length"], MARKERS[1].len());
    let failures: Vec<String> = snap
        .of_kind(ObjectKind::Table)
        .filter_map(|t| {
            t.detail["profile"]["error"]
                .as_str()
                .map(|e| format!("{}: {e}", t.name))
        })
        .collect();
    for f in failures.iter().take(5) {
        println!("failed: {}", f.lines().next().unwrap_or_default());
    }
    assert_eq!(stats.tables_failed, 0);
    assert_eq!(log["log_mode"], "NOARCHIVELOG");
}

/// RFC 0212: the dictionary reader's flags agree with a direct count, as the read-only user.
#[test]
fn the_dictionary_flags_match_a_direct_count() {
    if !live() {
        return;
    }
    let s = read_only();
    let flags =
        ekos_ora_live::dictionary::read(&s, ekos_ora_live::DictionaryKind::Idempiere, "ADEMPIERE")
            .unwrap();
    let direct = s
        .rows_of(
            "SELECT COUNT(*), SUM(CASE WHEN IsDeleteable = 'Y' THEN 1 ELSE 0 END), \
             SUM(CASE WHEN IsChangeLog = 'Y' THEN 1 ELSE 0 END) FROM ADEMPIERE.AD_Table",
            &[],
        )
        .unwrap();
    let n = |i: usize| -> usize { direct[0][i].as_deref().unwrap().parse().unwrap() };
    let deleteable = flags.iter().filter(|f| f.deleteable == Some(true)).count();
    let logged = flags.iter().filter(|f| f.change_log == Some(true)).count();
    println!(
        "dictionary: {} tables, {deleteable} deletable, {logged} change-logged",
        flags.len()
    );
    assert_eq!((flags.len(), deleteable, logged), (n(0), n(1), n(2)));
}
