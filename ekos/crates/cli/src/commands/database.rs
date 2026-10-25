//! `ekos database list|add|remove|test` — RFC 0223.
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
            kind: match d.kind {
                DatabaseKind::Oracle => "oracle",
            },
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
            "No databases. Add one: ekos database add <name> --connect host:port/service --user <u> --password-env <VAR> --owner <SCHEMA>"
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
    t["kind"] = toml_edit::value("oracle");
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
    if !a.connect.contains('/') {
        bail!("--connect is an Easy Connect string: host:port/service");
    }
    let d = DatabaseConfig {
        name: a.name.clone(),
        kind: DatabaseKind::Oracle,
        connect: a.connect,
        user: a.user,
        password_env: a.password_env,
        owners: a.owners.iter().map(|o| o.to_ascii_uppercase()).collect(),
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

/// Ledger objects read live from database `name`, by kind.
fn compiled_counts(
    config: &EkosConfig,
    cwd: &Path,
    name: &str,
) -> Result<Option<BTreeMap<String, usize>>> {
    let Ok(ledger) = open_store_read_only(config, cwd) else {
        return Ok(None);
    };
    let mut out = BTreeMap::new();
    for o in ledger.all_objects()? {
        let live = o.properties.get("origin").and_then(Value::as_str) == Some("live")
            && o.properties.get("database").and_then(Value::as_str) == Some(name);
        if !live {
            continue;
        }
        let kind = match &o.kind {
            ObjectKind::Custom(k) => k.clone(),
            k => k.to_string(),
        };
        *out.entry(kind).or_insert(0) += 1;
    }
    Ok(if out.is_empty() { None } else { Some(out) })
}

/// `ekos database test`
pub fn test(config: &EkosConfig, cwd: &Path, name: &str, json_out: bool) -> Result<()> {
    let Some(d) = config.databases.iter().find(|d| d.name == name) else {
        bail!("no database {name:?} in this project (ekos database list)");
    };
    let connection = ekos_plugin_oracle::OraConnection {
        connect_string: d.connect.clone(),
        user: d.user.clone(),
        password_env: d.password_env.clone(),
    };
    let run_id = format!("test-{}", chrono::Utc::now().format("%Y%m%dT%H%M%S"));
    let started = std::time::Instant::now();
    let result = ekos_plugin_oracle::test_connection(
        &connection,
        &d.owners,
        &run_id,
        &config.redaction_config(),
    )
    .map_err(|e| anyhow::anyhow!("database {name}: {e}"))?;
    let ms = started.elapsed().as_millis() as u64;
    let mut census: BTreeMap<String, usize> = BTreeMap::new();
    let mut invalid = 0;
    for o in &result.owners {
        for (t, c) in &o.census {
            *census.entry(t.clone()).or_default() += c.valid + c.invalid;
            invalid += c.invalid;
        }
    }
    let compiled = compiled_counts(config, cwd, name)?;
    let rows = compare(&census, compiled.as_ref());
    let read_only = result.write_check.as_ref().map(|w| w.is_safe());
    if json_out {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "database": name,
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
        result.label,
        result.family,
        match read_only {
            Some(true) => "refused by the server (read-only)",
            Some(false) => "ALLOWED — use a read-only user",
            None => "not checked (no table)",
        }
    );
    for o in &result.owners {
        let total: usize = o.census.values().map(|c| c.valid + c.invalid).sum();
        println!("  {}: {total} objects, {} columns", o.owner, o.columns);
    }
    if !result.any_privileges.is_empty() {
        println!(
            "  ANY privileges held: {}",
            result.any_privileges.join(", ")
        );
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
