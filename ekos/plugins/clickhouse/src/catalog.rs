//! RFC 0235 — a ClickHouse server as a project database (`[[databases]] kind = "clickhouse"`).
//!
//! Unlike RFC 0056's environment-variable observer (one database, `ObjectKind::Table` artifacts of
//! its own shape), this one reads the databases a `[[databases]]` entry lists and emits RFC 0211's
//! live-catalog shape (`ekos_observation_sdk::live_catalog`), so its objects carry `origin: live`
//! and are compiled by the same analyzer as Oracle and PostgreSQL catalogs.
//!
//! Every catalog query is sent with `readonly=1`. The write probe is the one statement sent
//! without it: a zero-row `INSERT … SELECT … WHERE 0` into the first table, so a user that can
//! write is reported while nothing is written.

use ekos_common::redaction::{RedactionConfig, redact};
use ekos_observation_sdk::live_catalog::{self, LiveObject, OwnerCatalog};
use ekos_observation_sdk::{ObservationPackage, ObserveError, Observer, ScanContext};
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// The observer's name and `source_kind`; distinct from RFC 0056's `clickhouse`, whose analyzer
/// reads another shape.
pub const OBSERVER: &str = "clickhouse-live";
/// The target scheme: `clickhouse://<database entry>/<ClickHouse database>/…`.
pub const SCHEME: &str = "clickhouse";

/// One server's connection: the password stays an environment variable's name.
#[derive(Debug, Clone)]
pub struct ChConnection {
    /// `http[s]://host[:port]`, no path.
    pub url: String,
    pub user: String,
    pub password_env: String,
}

impl ChConnection {
    /// `http[s]://host[:port]` (RFC 0235 §2); a trailing `/` is dropped.
    pub fn parse(connect: &str, user: &str, password_env: &str) -> Result<Self, String> {
        let url = connect.trim_end_matches('/');
        let rest = url
            .strip_prefix("http://")
            .or_else(|| url.strip_prefix("https://"))
            .ok_or_else(|| format!("connect is http[s]://host[:port], not {connect:?}"))?;
        if rest.is_empty() || rest.contains(['/', '@', '?', '#']) {
            return Err(format!(
                "connect is http[s]://host[:port] with no path or credentials, not {connect:?}"
            ));
        }
        Ok(Self {
            url: url.to_string(),
            user: user.to_string(),
            password_env: password_env.to_string(),
        })
    }

    pub fn label(&self) -> String {
        let host = self.url.split_once("://").map_or("", |(_, h)| h);
        format!("clickhouse:{}@{host}", self.user)
    }
}

/// A ClickHouse string literal.
fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'"))
}

/// A ClickHouse identifier.
fn ident(value: &str) -> String {
    format!("`{}`", value.replace('\\', "\\\\").replace('`', "\\`"))
}

pub(crate) struct Http {
    conn: ChConnection,
    password: String,
    client: reqwest::Client,
}

impl Http {
    pub(crate) fn new(conn: &ChConnection) -> Result<Self, String> {
        let password = std::env::var(&conn.password_env).map_err(|_| {
            format!(
                "the environment variable {} names this database's password but is not set",
                conn.password_env
            )
        })?;
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self {
            conn: conn.clone(),
            password,
            client,
        })
    }

    pub(crate) async fn send(&self, sql: &str, readonly: bool) -> Result<String, String> {
        let mut req = self
            .client
            .post(&self.conn.url)
            .header("X-ClickHouse-User", &self.conn.user)
            .header("X-ClickHouse-Key", &self.password)
            .body(sql.to_string());
        if readonly {
            req = req.query(&[("readonly", "1")]);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| format!("cannot connect: {e}"))?;
        let status = resp.status();
        let body = resp.text().await.map_err(|e| e.to_string())?;
        if !status.is_success() {
            return Err(body.trim().chars().take(500).collect());
        }
        Ok(body)
    }

    async fn rows(&self, sql: &str) -> Result<Vec<Value>, String> {
        let body = self.send(&format!("{sql} FORMAT JSON"), true).await?;
        let v: Value = serde_json::from_str(&body).map_err(|e| format!("not JSON: {e}"))?;
        Ok(v["data"].as_array().cloned().unwrap_or_default())
    }
}

/// What one read found.
#[derive(Debug, Clone)]
pub struct DatabaseReading {
    pub catalogs: Vec<OwnerCatalog>,
    pub write_check: Option<Value>,
}

/// Read the listed databases' tables and columns, and probe a write.
pub async fn read(
    conn: &ChConnection,
    databases: &[String],
    redaction: &RedactionConfig,
) -> Result<DatabaseReading, String> {
    let http = Http::new(conn)?;
    let list = databases
        .iter()
        .map(|d| quote(d))
        .collect::<Vec<_>>()
        .join(", ");
    let tables = http
        .rows(&format!(
            "SELECT database, name, engine, comment, as_select, sorting_key, primary_key, \
                    partition_key, toString(total_rows) AS total_rows \
             FROM system.tables WHERE database IN ({list}) AND NOT is_temporary \
             ORDER BY database, name"
        ))
        .await?;
    let columns = http
        .rows(&format!(
            "SELECT database, table, name, type, default_kind, default_expression, comment \
             FROM system.columns WHERE database IN ({list}) ORDER BY database, table, position"
        ))
        .await?;
    let catalogs = normalize(databases, &tables, &columns, redaction);
    let probe = catalogs
        .iter()
        .find_map(|c| c.objects.iter().find(|o| o.kind == "table"));
    let write_check = match probe {
        Some(t) => {
            let target = format!("{}.{}", ident(&t.owner), ident(&t.name));
            let sql = format!("INSERT INTO {target} SELECT * FROM {target} WHERE 0");
            Some(live_catalog::write_check(
                http.send(&sql, false).await.map(|_| ()),
            ))
        }
        None => None,
    };
    Ok(DatabaseReading {
        catalogs,
        write_check,
    })
}

fn s(v: &Value, k: &str) -> String {
    match &v[k] {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// `system.tables` + `system.columns` rows as RFC 0211's per-owner objects. Pure. Every listed
/// database gets a catalog, even an empty one.
pub fn normalize(
    databases: &[String],
    tables: &[Value],
    columns: &[Value],
    redaction: &RedactionConfig,
) -> Vec<OwnerCatalog> {
    let r = |t: &str| redact(t, redaction);
    let mut owners: BTreeMap<String, OwnerCatalog> = databases
        .iter()
        .map(|d| {
            (
                d.clone(),
                OwnerCatalog {
                    owner: d.clone(),
                    family: "system".into(),
                    ..Default::default()
                },
            )
        })
        .collect();
    let mut by_table: BTreeMap<(String, String), Vec<Value>> = BTreeMap::new();
    for c in columns {
        let ty = s(c, "type");
        let mut col = json!({
            "name": s(c, "name"),
            "data_type": ty,
            "nullable": ty.starts_with("Nullable("),
        });
        let default = s(c, "default_expression");
        if !default.is_empty() {
            col["default"] = r(&default).into();
            col["default_kind"] = s(c, "default_kind").into();
        }
        let comment = s(c, "comment");
        if !comment.is_empty() {
            col["comment"] = r(&comment).into();
        }
        by_table
            .entry((s(c, "database"), s(c, "table")))
            .or_default()
            .push(col);
    }
    for t in tables {
        let (db, name, engine) = (s(t, "database"), s(t, "name"), s(t, "engine"));
        let Some(c) = owners.get_mut(&db) else {
            continue;
        };
        let census = match engine.as_str() {
            "View" | "MaterializedView" | "LiveView" | "WindowView" => "VIEW",
            "Dictionary" => "DICTIONARY",
            _ => "TABLE",
        };
        c.census.entry(census.into()).or_default().valid += 1;
        let cols = by_table
            .remove(&(db.clone(), name.clone()))
            .unwrap_or_default();
        let comment = s(t, "comment");
        let object = match census {
            "VIEW" => {
                let mut detail = json!({ "text": r(&s(t, "as_select")), "engine": engine });
                if engine == "MaterializedView" {
                    detail["materialized"] = true.into();
                }
                LiveObject {
                    kind: "view".into(),
                    owner: db.clone(),
                    name,
                    status: None,
                    detail,
                }
            }
            "DICTIONARY" => {
                c.columns += cols.len();
                continue;
            }
            _ => {
                let mut detail = json!({ "engine": engine });
                for (k, from) in [
                    ("sorting_key", "sorting_key"),
                    ("engine_primary_key", "primary_key"),
                    ("partition_key", "partition_key"),
                ] {
                    let v = s(t, from);
                    if !v.is_empty() {
                        detail[k] = v.into();
                    }
                }
                if let Ok(n) = s(t, "total_rows").parse::<u64>() {
                    detail["num_rows"] = n.into();
                }
                LiveObject {
                    kind: "table".into(),
                    owner: db.clone(),
                    name,
                    status: None,
                    detail,
                }
            }
        };
        let mut object = object;
        c.columns += cols.len();
        if object.kind == "table" {
            object.detail["columns"] = Value::Array(cols);
        }
        if !comment.is_empty() {
            object.detail["comment"] = r(&comment).into();
        }
        c.objects.push(object);
    }
    owners.into_values().collect()
}

/// RFC 0223 / 0235: `ekos database test`'s report, the same JSON as every other kind.
pub async fn test_connection(
    conn: &ChConnection,
    databases: &[String],
    redaction: &RedactionConfig,
) -> Result<Value, String> {
    let reading = read(conn, databases, redaction).await?;
    Ok(test_report(&conn.label(), &reading))
}

/// The report of one reading. Pure.
pub fn test_report(label: &str, reading: &DatabaseReading) -> Value {
    json!({
        "label": label,
        "family": "system",
        "owners": reading.catalogs.iter().map(|c| json!({
            "owner": c.owner,
            "census": c.census,
            "listed": c.listed(),
            "columns": c.columns,
        })).collect::<Vec<_>>(),
        "write_check": reading.write_check,
        "any_privileges": [],
    })
}

/// One `[[databases]]` entry of kind `clickhouse`.
pub struct ClickHouseCatalogObserver {
    database: String,
    conn: ChConnection,
    databases: Vec<String>,
    redaction: RedactionConfig,
}

impl ClickHouseCatalogObserver {
    pub fn new(
        database: impl Into<String>,
        conn: ChConnection,
        databases: Vec<String>,
        redaction: RedactionConfig,
    ) -> Self {
        Self {
            database: database.into(),
            conn,
            databases,
            redaction,
        }
    }
}

#[async_trait::async_trait]
impl Observer for ClickHouseCatalogObserver {
    fn name(&self) -> &str {
        OBSERVER
    }

    async fn scan(&self, _ctx: &ScanContext) -> Result<ObservationPackage, ObserveError> {
        let reading = read(&self.conn, &self.databases, &self.redaction)
            .await
            .map_err(|e| {
                ObserveError::connector(format!("clickhouse database {}: {e}", self.database))
            })?;
        let mut pkg = ObservationPackage::new(OBSERVER, "database");
        for a in live_catalog::artifacts(
            OBSERVER,
            "ekos-plugin-clickhouse",
            SCHEME,
            &self.database,
            &reading.catalogs,
            reading.write_check.as_ref(),
        ) {
            pkg.push(a);
        }
        Ok(pkg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows() -> (Vec<Value>, Vec<Value>) {
        let tables = vec![
            json!({ "database": "wh", "name": "events", "engine": "MergeTree", "comment": "Raw events",
                    "as_select": "", "sorting_key": "ts, id", "primary_key": "ts", "partition_key": "toYYYYMM(ts)",
                    "total_rows": "42" }),
            json!({ "database": "wh", "name": "daily", "engine": "View", "comment": "",
                    "as_select": "SELECT toDate(ts) AS d, count() AS n FROM wh.events GROUP BY d",
                    "sorting_key": "", "primary_key": "", "partition_key": "", "total_rows": "" }),
            json!({ "database": "wh", "name": "geo", "engine": "Dictionary", "comment": "",
                    "as_select": "", "sorting_key": "", "primary_key": "", "partition_key": "", "total_rows": "" }),
        ];
        let columns = vec![
            json!({ "database": "wh", "table": "events", "name": "id", "type": "UInt64", "default_kind": "", "default_expression": "", "comment": "" }),
            json!({ "database": "wh", "table": "events", "name": "ts", "type": "DateTime", "default_kind": "DEFAULT", "default_expression": "now()", "comment": "" }),
            json!({ "database": "wh", "table": "events", "name": "note", "type": "Nullable(String)", "default_kind": "", "default_expression": "", "comment": "Free text" }),
            json!({ "database": "wh", "table": "daily", "name": "d", "type": "Date", "default_kind": "", "default_expression": "", "comment": "" }),
        ];
        (tables, columns)
    }

    #[test]
    fn tables_carry_columns_and_engine_keys_views_their_query() {
        let (t, c) = rows();
        let cats = normalize(
            &["wh".into(), "empty".into()],
            &t,
            &c,
            &RedactionConfig::default(),
        );
        assert_eq!(
            cats.len(),
            2,
            "an empty listed database still has a catalog"
        );
        let wh = cats.iter().find(|c| c.owner == "wh").unwrap();
        assert_eq!(wh.census["TABLE"].valid, 1);
        assert_eq!(wh.census["VIEW"].valid, 1);
        assert_eq!(wh.census["DICTIONARY"].valid, 1);
        assert_eq!(wh.columns, 4);
        let ev = wh.objects.iter().find(|o| o.name == "events").unwrap();
        assert_eq!(ev.detail["sorting_key"], "ts, id");
        assert_eq!(ev.detail["engine_primary_key"], "ts");
        assert_eq!(ev.detail["num_rows"], 42);
        assert_eq!(ev.detail["comment"], "Raw events");
        let cols = ev.detail["columns"].as_array().unwrap();
        assert_eq!(cols[0]["nullable"], false);
        assert_eq!(cols[1]["default"], "now()");
        assert_eq!(cols[2]["nullable"], true);
        assert_eq!(cols[2]["comment"], "Free text");
        let v = wh.objects.iter().find(|o| o.kind == "view").unwrap();
        assert!(
            v.detail["text"]
                .as_str()
                .unwrap()
                .starts_with("SELECT toDate")
        );
        assert!(
            !wh.objects.iter().any(|o| o.name == "geo"),
            "counted, not emitted"
        );
    }

    #[test]
    fn connect_is_an_http_url_without_path_or_credentials() {
        let c = ChConnection::parse("https://ch.local:8443/", "ro", "PW").unwrap();
        assert_eq!(c.url, "https://ch.local:8443");
        assert_eq!(c.label(), "clickhouse:ro@ch.local:8443");
        for bad in [
            "ch.local:8123",
            "http://",
            "http://u:p@h",
            "http://h/db",
            "ftp://h",
        ] {
            assert!(ChConnection::parse(bad, "ro", "PW").is_err(), "{bad}");
        }
    }

    #[test]
    fn literals_and_identifiers_are_escaped() {
        assert_eq!(quote(r"a'b\c"), r"'a\'b\\c'");
        assert_eq!(ident("we`ird"), r"`we\`ird`");
    }
}
