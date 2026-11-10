//! `ekos database list|add|remove|test` — RFC 0223; PostgreSQL and ClickHouse kinds, RFC 0235.
//!
//! `[[databases]]` entries edited through `toml_edit` (comments and the rest of the file kept, the
//! edited file must still load), and a connection test that reads the catalog the way `ekos build`
//! does, without profiling. A password is only ever an environment variable's name here.

use super::store::open_store_read_only;
use anyhow::{Context, Result, bail};
use ekos_compiler_core::{
    DatabaseConfig, DatabaseDictionary, DatabaseKind, DatabaseProfile, EkosConfig,
};
use ekos_kir::ObjectKind;
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::Path;

/// One `[[databases]]` entry as `list` prints it.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct DatabaseInfo {
    pub name: String,
    pub kind: &'static str,
    pub connect: String,
    pub user: String,
    pub password_env: String,
    /// Whether that variable is set in this process's environment (its value is never read).
    pub password_set: bool,
    pub owners: Vec<String>,
    pub profile: &'static str,
    pub dictionary: Option<&'static str>,
}

fn profile_str(p: DatabaseProfile) -> &'static str {
    match p {
        DatabaseProfile::P0 => "p0",
        DatabaseProfile::P1 => "p1",
        DatabaseProfile::P2 => "p2",
    }
}

pub fn infos(config: &EkosConfig) -> Vec<DatabaseInfo> {
    config
        .databases
        .iter()
        .map(|d| DatabaseInfo {
            name: d.name.clone(),
            kind: d.kind.as_str(),
            connect: d.connect.clone(),
            user: d.user.clone(),
            password_env: d.password_env.clone(),
            password_set: std::env::var_os(&d.password_env).is_some(),
            owners: d.owners.clone(),
            profile: profile_str(d.profile),
            dictionary: d.dictionary.map(|_| "idempiere"),
        })
        .collect()
}

/// `ekos database list`
pub fn list(config: &EkosConfig, json_out: bool) -> Result<()> {
    let rows = infos(config);
    if json_out {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({ "databases": rows }))?
        );
        return Ok(());
    }
    if rows.is_empty() {
        println!(
            "No databases. Add one: ekos database add <name> [--kind oracle|postgres|clickhouse] --connect <address> --user <u> --password-env <VAR> --owner <SCHEMA>"
        );
    }
    for d in rows {
        println!(
            "{:<16} {} {}@{} owners {} profile {} password ${} ({})",
            d.name,
            d.kind,
            d.user,
            d.connect,
            d.owners.join(","),
            d.profile,
            d.password_env,
            if d.password_set { "set" } else { "NOT SET" }
        );
    }
    Ok(())
}

/// `config_text` with one more `[[databases]]` entry.
pub fn with_database(config_text: &str, d: &DatabaseConfig) -> Result<String> {
    let mut doc: toml_edit::DocumentMut = config_text.parse().context("parsing ekos.toml")?;
    let mut t = toml_edit::Table::new();
    t["name"] = toml_edit::value(d.name.as_str());
    t["kind"] = toml_edit::value(d.kind.as_str());
    t["connect"] = toml_edit::value(d.connect.as_str());
    t["user"] = toml_edit::value(d.user.as_str());
    t["password-env"] = toml_edit::value(d.password_env.as_str());
    let mut owners = toml_edit::Array::new();
    for o in &d.owners {
        owners.push(o.as_str());
    }
    t["owners"] = toml_edit::value(owners);
    t["profile"] = toml_edit::value(profile_str(d.profile));
    if d.dictionary.is_some() {
        t["dictionary"] = toml_edit::value("idempiere");
    }
    let entry = doc
        .entry("databases")
        .or_insert(toml_edit::Item::ArrayOfTables(Default::default()));
    let Some(array) = entry.as_array_of_tables_mut() else {
        bail!("ekos.toml: `databases` is not a list of [[databases]] tables");
    };
    array.push(t);
    checked(doc.to_string())
}

/// `config_text` without the `[[databases]]` entry called `name`.
pub fn without_database(config_text: &str, name: &str) -> Result<String> {
    let mut doc: toml_edit::DocumentMut = config_text.parse().context("parsing ekos.toml")?;
    let array = doc
        .get_mut("databases")
        .and_then(toml_edit::Item::as_array_of_tables_mut);
    let position = array.as_ref().and_then(|a| {
        a.iter()
            .position(|t| t.get("name").and_then(toml_edit::Item::as_str) == Some(name))
    });
    let (Some(array), Some(position)) = (array, position) else {
        bail!("no database {name:?} in this project");
    };
    array.remove(position);
    if array.is_empty() {
        doc.remove("databases");
    }
    checked(doc.to_string())
}

fn checked(text: String) -> Result<String> {
    let config: EkosConfig = toml::from_str(&text).context("the edited ekos.toml is invalid")?;
    config.check_sources()?;
    Ok(text)
}

/// The arguments of `ekos database add`.
pub struct AddArgs {
    pub name: String,
    /// `oracle` (default), `postgres` or `clickhouse`.
    pub kind: Option<String>,
    pub connect: String,
    pub user: String,
    pub password_env: String,
    pub owners: Vec<String>,
    pub profile: Option<String>,
    pub dictionary: Option<String>,
}

/// `ekos database add`
pub fn add(config_path: &Path, a: AddArgs) -> Result<()> {
    let profile = match a.profile.as_deref().unwrap_or("p0") {
        "p0" => DatabaseProfile::P0,
        "p1" => DatabaseProfile::P1,
        "p2" => DatabaseProfile::P2,
        other => bail!("--profile is p0, p1 or p2, not {other:?}"),
    };
    let dictionary = match a.dictionary.as_deref() {
        None => None,
        Some("idempiere") => Some(DatabaseDictionary::Idempiere),
        Some(other) => bail!("--dictionary is idempiere, not {other:?}"),
    };
    if !a
        .password_env
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_')
        || a.password_env.is_empty()
        || a.password_env.starts_with(|c: char| c.is_ascii_digit())
    {
        bail!(
            "--password-env is the NAME of an environment variable (letters, digits, `_`), never the password itself"
        );
    }
    let kind = parse_kind(a.kind.as_deref())?;
    check_connect(kind, &a.connect)?;
    let d = DatabaseConfig {
        name: a.name.clone(),
        kind,
        connect: a.connect,
        user: a.user,
        password_env: a.password_env,
        // Oracle's dictionary spells owners in upper case; PostgreSQL and ClickHouse names are
        // case-sensitive as written.
        owners: match kind {
            DatabaseKind::Oracle => a.owners.iter().map(|o| o.to_ascii_uppercase()).collect(),
            _ => a.owners.clone(),
        },
        profile,
        dictionary,
    };
    let text = std::fs::read_to_string(config_path)
        .with_context(|| format!("reading {}", config_path.display()))?;
    std::fs::write(config_path, with_database(&text, &d)?)?;
    println!(
        "Added database {} ({}@{}, owners {}). Test it: ekos database test {}",
        d.name,
        d.user,
        d.connect,
        d.owners.join(","),
        d.name
    );
    Ok(())
}

fn parse_kind(kind: Option<&str>) -> Result<DatabaseKind> {
    Ok(match kind.unwrap_or("oracle") {
        "oracle" => DatabaseKind::Oracle,
        "postgres" | "postgresql" => DatabaseKind::Postgres,
        "clickhouse" => DatabaseKind::Clickhouse,
        "sqlserver" | "mssql" => DatabaseKind::Sqlserver,
        "informix" => DatabaseKind::Informix,
        other => {
            bail!("--kind is oracle, postgres, clickhouse, sqlserver or informix, not {other:?}")
        }
    })
}

/// RFC 0235: `connect` per kind (the observers parse it again, the same way).
fn check_connect(kind: DatabaseKind, connect: &str) -> Result<()> {
    match kind {
        DatabaseKind::Oracle if !connect.contains('/') => {
            bail!("--connect is an Easy Connect string: host:port/service")
        }
        DatabaseKind::Oracle => Ok(()),
        DatabaseKind::Postgres => ekos_plugin_postgres::PgConnection::parse(connect, "", "")
            .map(|_| ())
            .map_err(|e| anyhow::anyhow!("--{e}")),
        DatabaseKind::Clickhouse => {
            ekos_plugin_clickhouse::catalog::ChConnection::parse(connect, "", "")
                .map(|_| ())
                .map_err(|e| anyhow::anyhow!("--{e}"))
        }
        DatabaseKind::Sqlserver => {
            ekos_plugin_sqlserver::SqlServerConnection::parse(connect, "", "")
                .map(|_| ())
                .map_err(|e| anyhow::anyhow!("--{e}"))
        }
        DatabaseKind::Informix => ekos_plugin_informix::InformixConnection::parse(connect, "", "")
            .map(|_| ())
            .map_err(|e| anyhow::anyhow!("--{e}")),
    }
}

/// `ekos database remove`
pub fn remove(config_path: &Path, name: &str) -> Result<()> {
    let text = std::fs::read_to_string(config_path)
        .with_context(|| format!("reading {}", config_path.display()))?;
    std::fs::write(config_path, without_database(&text, name)?)?;
    println!(
        "Removed database {name}. Objects already compiled from it stay in the ledger: the ledger is append-only."
    );
    Ok(())
}

/// The census kinds compared with the ledger: catalog `OBJECT_TYPE`s → the ledger kind they compile to.
const COMPARED: &[(&str, &[&str], &str)] = &[
    ("tables", &["TABLE"], "Table"),
    ("views", &["VIEW"], "View"),
    ("routines", &["FUNCTION", "PROCEDURE"], "Procedure"),
    ("triggers", &["TRIGGER"], "Trigger"),
];

/// Pure: census vs compiled rows. `census` is `OBJECT_TYPE` → count (all owners); `compiled` is
/// ledger kind → count.
pub fn compare(
    census: &BTreeMap<String, usize>,
    compiled: Option<&BTreeMap<String, usize>>,
) -> Vec<Value> {
    COMPARED
        .iter()
        .map(|(what, types, kind)| {
            let c: usize = types.iter().filter_map(|t| census.get(*t)).sum();
            let l = compiled.map(|m| m.get(*kind).copied().unwrap_or(0));
            json!({
                "what": what,
                "census": c,
                "compiled": l,
                "equal": l.map(|l| l == c),
            })
        })
        .collect()
}

/// Objects read live from database `name`, by kind: from the latest compiled model, which holds
/// only what the newest catalog read listed. The ledger is append-only, so counting it would keep
/// an object dropped from the database (RFC 0235 §4, found live); it is the fallback when no model
/// has been compiled.
fn compiled_counts(
    config: &EkosConfig,
    cwd: &Path,
    name: &str,
) -> Result<Option<BTreeMap<String, usize>>> {
    let live = |properties: &std::collections::HashMap<String, Value>| {
        properties.get("origin").and_then(Value::as_str) == Some("live")
            && properties.get("database").and_then(Value::as_str) == Some(name)
    };
    // An Oracle or SQL Server trigger carries its body inline, which compiles to a `Procedure`
    // named after the trigger (`returns: trigger`) beside the `Trigger`: counted with the
    // triggers, not the routines the census counts. A PostgreSQL trigger *function* is a real
    // routine, in the census as one.
    let kind_name = |k: &ObjectKind, properties: &std::collections::HashMap<String, Value>| {
        let text = |key: &str| properties.get(key).and_then(Value::as_str);
        match k {
            ObjectKind::Custom(k)
                if k == "Procedure"
                    && text("returns") == Some("trigger")
                    && matches!(text("language"), Some("plsql" | "tsql" | "informix")) =>
            {
                "TriggerBody".to_string()
            }
            ObjectKind::Custom(k) => k.clone(),
            k => k.to_string(),
        }
    };
    let mut out = BTreeMap::new();
    let model_path = config.ekos_dir(cwd).join("ckm").join("model.json");
    if ekos_common::compress::resolve_auto(&model_path).is_some() {
        let model: ekos_semantic::CkModel = ekos_common::compress::read_json_auto(&model_path)?;
        for o in model.objects.iter().filter(|o| live(&o.properties)) {
            *out.entry(kind_name(&o.kind, &o.properties)).or_insert(0) += 1;
        }
    } else {
        let Ok(ledger) = open_store_read_only(config, cwd) else {
            return Ok(None);
        };
        for o in ledger.all_objects()?.iter().filter(|o| live(&o.properties)) {
            *out.entry(kind_name(&o.kind, &o.properties)).or_insert(0) += 1;
        }
    }
    Ok(if out.is_empty() { None } else { Some(out) })
}

/// `ekos database test`
pub async fn test(config: &EkosConfig, cwd: &Path, name: &str, json_out: bool) -> Result<()> {
    let Some(d) = config.databases.iter().find(|d| d.name == name) else {
        bail!("no database {name:?} in this project (ekos database list)");
    };
    let run_id = format!("test-{}", chrono::Utc::now().format("%Y%m%dT%H%M%S"));
    let redaction = config.redaction_config();
    let started = std::time::Instant::now();
    let fail = |e: String| anyhow::anyhow!("database {name}: {e}");
    // Every kind reports the same JSON (RFC 0235 §2.3): label, family, owners[].census,
    // write_check, any_privileges.
    let result: Value = match d.kind {
        DatabaseKind::Oracle => {
            let connection = ekos_plugin_oracle::OraConnection {
                connect_string: d.connect.clone(),
                user: d.user.clone(),
                password_env: d.password_env.clone(),
            };
            let r =
                ekos_plugin_oracle::test_connection(&connection, &d.owners, &run_id, &redaction)
                    .map_err(|e| fail(e.to_string()))?;
            serde_json::to_value(r)?
        }
        DatabaseKind::Postgres => {
            let connection =
                ekos_plugin_postgres::PgConnection::parse(&d.connect, &d.user, &d.password_env)
                    .map_err(fail)?;
            let owners = d.owners.clone();
            // The driver is synchronous and starts its own runtime: off this one.
            tokio::task::spawn_blocking(move || {
                ekos_plugin_postgres::test_connection(&connection, &owners, &run_id, &redaction)
            })
            .await?
            .map_err(|e| fail(e.to_string()))?
        }
        DatabaseKind::Clickhouse => {
            let connection = ekos_plugin_clickhouse::catalog::ChConnection::parse(
                &d.connect,
                &d.user,
                &d.password_env,
            )
            .map_err(fail)?;
            ekos_plugin_clickhouse::catalog::test_connection(&connection, &d.owners, &redaction)
                .await
                .map_err(fail)?
        }
        DatabaseKind::Sqlserver => {
            let connection = ekos_plugin_sqlserver::SqlServerConnection::parse(
                &d.connect,
                &d.user,
                &d.password_env,
            )
            .map_err(fail)?;
            let owners = d.owners.clone();
            // The session runs its own runtime: off this one.
            tokio::task::spawn_blocking(move || {
                ekos_plugin_sqlserver::test_connection(&connection, &owners, &run_id, &redaction)
            })
            .await?
            .map_err(fail)?
        }
        // RFC 0252 §3.5.
        DatabaseKind::Informix => {
            let connection = ekos_plugin_informix::InformixConnection::parse(
                &d.connect,
                &d.user,
                &d.password_env,
            )
            .map_err(fail)?;
            let owners = d.owners.clone();
            tokio::task::spawn_blocking(move || {
                ekos_plugin_informix::test_connection(&connection, &owners, &run_id, &redaction)
            })
            .await?
            .map_err(fail)?
        }
    };
    let ms = started.elapsed().as_millis() as u64;
    let (census, invalid) = census_of(&result);
    let compiled = compiled_counts(config, cwd, name)?;
    let rows = compare(&census, compiled.as_ref());
    let read_only = result["write_check"]["result"]
        .as_str()
        .map(|r| r == "refused_by_server");
    if json_out {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "database": name,
                "kind": d.kind.as_str(),
                "ok": true,
                "ms": ms,
                "read_only": read_only,
                "invalid_objects": invalid,
                "test": result,
                "census_vs_compiled": rows,
            }))?
        );
        return Ok(());
    }
    println!(
        "{name}: connected as {} ({} views) in {ms} ms; writes {}",
        result["label"].as_str().unwrap_or_default(),
        result["family"].as_str().unwrap_or_default(),
        match read_only {
            Some(true) => "refused by the server (read-only)",
            Some(false) => "ALLOWED — use a read-only user",
            None => "not checked (no table)",
        }
    );
    for o in result["owners"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        let total: u64 = o["census"]
            .as_object()
            .map(|m| {
                m.values()
                    .map(|c| c["valid"].as_u64().unwrap_or(0) + c["invalid"].as_u64().unwrap_or(0))
                    .sum()
            })
            .unwrap_or(0);
        println!(
            "  {}: {total} objects, {} columns",
            o["owner"].as_str().unwrap_or_default(),
            o["columns"]
        );
    }
    let any: Vec<&str> = result["any_privileges"]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    if !any.is_empty() {
        println!("  ANY privileges held: {}", any.join(", "));
    }
    println!("  {:<10} {:>8} {:>9}", "", "census", "compiled");
    for r in rows {
        println!(
            "  {:<10} {:>8} {:>9}{}",
            r["what"].as_str().unwrap_or(""),
            r["census"],
            r["compiled"].as_u64().map_or("—".into(), |n| n.to_string()),
            match r["equal"].as_bool() {
                Some(false) => "  ← differs",
                _ => "",
            }
        );
    }
    Ok(())
}

/// A test report's census summed over owners (`TYPE` → count), and the invalid objects.
fn census_of(result: &Value) -> (BTreeMap<String, usize>, usize) {
    let mut census: BTreeMap<String, usize> = BTreeMap::new();
    let mut invalid = 0;
    for o in result["owners"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        for (t, c) in o["census"].as_object().into_iter().flatten() {
            let n = |k: &str| c[k].as_u64().unwrap_or(0) as usize;
            *census.entry(t.clone()).or_default() += n("valid") + n("invalid");
            invalid += n("invalid");
        }
    }
    (census, invalid)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str =
        "# my project\n[workspace]\nroot = \".\"\n\n[[sources]]\nname = \"code\"\npath = \".\"\n";

    fn db(name: &str) -> DatabaseConfig {
        DatabaseConfig {
            name: name.into(),
            kind: DatabaseKind::Oracle,
            connect: "localhost:1521/XEPDB1".into(),
            user: "ekos_ro".into(),
            password_env: "ERP_PW".into(),
            owners: vec!["ERP".into()],
            profile: DatabaseProfile::P2,
            dictionary: Some(DatabaseDictionary::Idempiere),
        }
    }

    #[test]
    fn adding_and_removing_a_database_keeps_the_rest_of_the_file() {
        let added = with_database(BASE, &db("erp")).unwrap();
        assert!(added.starts_with("# my project\n"), "{added}");
        let c: EkosConfig = toml::from_str(&added).unwrap();
        assert_eq!(c.databases, vec![db("erp")]);
        assert!(
            with_database(&added, &db("erp")).is_err(),
            "a duplicate name"
        );
        assert!(
            with_database(&added, &db("code")).is_err(),
            "a source's name"
        );
        let removed = without_database(&added, "erp").unwrap();
        assert_eq!(
            toml::from_str::<EkosConfig>(&removed).unwrap().databases,
            vec![]
        );
        assert!(without_database(&removed, "erp").is_err());
    }

    #[test]
    fn list_names_the_variable_never_a_value() {
        let c: EkosConfig = toml::from_str(&with_database(BASE, &db("erp")).unwrap()).unwrap();
        let v = serde_json::to_value(infos(&c)).unwrap();
        assert_eq!(v[0]["password_env"], "ERP_PW");
        assert!(v[0].get("password").is_none());
    }

    #[test]
    fn add_refuses_a_password_where_its_name_belongs() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("ekos.toml");
        std::fs::write(&p, BASE).unwrap();
        let args = |pw: &str| AddArgs {
            name: "erp".into(),
            kind: None,
            connect: "h:1521/S".into(),
            user: "u".into(),
            password_env: pw.into(),
            owners: vec!["erp".into()],
            profile: None,
            dictionary: None,
        };
        assert!(add(&p, args("s3cret!pass")).is_err());
        add(&p, args("ERP_PW")).unwrap();
        let c: EkosConfig = toml::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(
            c.databases[0].owners,
            vec!["ERP".to_string()],
            "owners upper-cased"
        );
    }

    /// RFC 0235: each kind checks its own connect form and keeps owners as written.
    #[test]
    fn add_takes_postgres_and_clickhouse_with_their_own_connect_form() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("ekos.toml");
        std::fs::write(&p, BASE).unwrap();
        let args = |name: &str, kind: &str, connect: &str| AddArgs {
            name: name.into(),
            kind: Some(kind.into()),
            connect: connect.into(),
            user: "ro".into(),
            password_env: "PW".into(),
            owners: vec!["Sales".into()],
            profile: None,
            dictionary: None,
        };
        assert!(add(&p, args("pg", "postgres", "https://h:8123")).is_err());
        assert!(add(&p, args("ch", "clickhouse", "h:5432/shop")).is_err());
        assert!(add(&p, args("x", "mysql", "h/x")).is_err());
        add(&p, args("pg", "postgres", "db.local:5432/shop")).unwrap();
        add(&p, args("ch", "clickhouse", "http://ch.local:8123")).unwrap();
        let c: EkosConfig = toml::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(c.databases[0].kind, DatabaseKind::Postgres);
        assert_eq!(
            c.databases[0].owners,
            vec!["Sales".to_string()],
            "not upper-cased"
        );
        assert_eq!(c.databases[1].kind, DatabaseKind::Clickhouse);
        assert_eq!(infos(&c)[1].kind, "clickhouse");
        // RFC 0252: every kind is profiled; the dictionary stays Oracle-only.
        let mut p2 = args("pg2", "postgres", "h/db");
        p2.profile = Some("p2".into());
        add(&p, p2).unwrap();
        let c: EkosConfig = toml::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(c.databases[2].profile, DatabaseProfile::P2);
        let mut d = args("pg3", "postgres", "h/db");
        d.dictionary = Some("idempiere".into());
        assert!(add(&p, d).is_err());
    }

    #[test]
    fn a_test_report_sums_its_census_over_owners() {
        let r = json!({ "owners": [
            { "census": { "TABLE": { "valid": 2, "invalid": 1 }, "VIEW": { "valid": 1, "invalid": 0 } } },
            { "census": { "TABLE": { "valid": 3, "invalid": 0 } } },
        ] });
        let (census, invalid) = census_of(&r);
        assert_eq!((census["TABLE"], census["VIEW"], invalid), (6, 1, 1));
    }

    #[test]
    fn census_and_compiled_counts_line_up_by_kind() {
        let census: BTreeMap<String, usize> = [
            ("TABLE".into(), 903),
            ("VIEW".into(), 160),
            ("FUNCTION".into(), 48),
            ("PROCEDURE".into(), 14),
            ("INDEX".into(), 2109),
        ]
        .into();
        let compiled: BTreeMap<String, usize> = [
            ("Table".into(), 903),
            ("View".into(), 160),
            ("Procedure".into(), 61),
        ]
        .into();
        let rows = compare(&census, Some(&compiled));
        assert_eq!(
            rows[0],
            json!({"what": "tables", "census": 903, "compiled": 903, "equal": true})
        );
        assert_eq!(rows[2]["census"], 62);
        assert_eq!(rows[2]["equal"], false);
        assert_eq!(compare(&census, None)[0]["compiled"], Value::Null);
    }
}
