//! RFC 0259 D5 — a DuckDB file's catalog as RFC 0211's live-catalog artifacts.
//!
//! One `[[databases]]` entry of kind `duckdb` becomes one [`DuckDbObserver`]. A scan opens the
//! file through the same read-only, locked [`crate::session::Session`] the probe uses (feature
//! `duckdb`), reads the listed schemas' tables, views, columns and constraints from
//! `duckdb_tables()` / `duckdb_views()` / `duckdb_columns()` / `duckdb_constraints()`, reconciles
//! what it read against those functions' own counts (a short catalog is an error, RFC 0157), probes
//! a write, and emits `ekos_observation_sdk::live_catalog`'s artifacts — the same shape as
//! PostgreSQL's, so `recovery::oracle_catalog_analyzer` reads them as `CatalogDialect::DuckDb`.
//!
//! A raw-content entry point (RFC 0043): every free text — a view's query, a comment, a column
//! default — is redacted before it leaves this module. The profile tiers are
//! `ekos_observation_sdk::profile`'s (RFC 0211 §3.4): P0 is `duckdb_tables().estimated_size`, P1
//! and P2 are its aggregate scans, and no tier keeps a text value.
//!
//! The SQL and the normalization are pure and compiled in every build; only [`read`] needs the
//! engine, and without it says so.

use ekos_common::redaction::{RedactionConfig, redact};
use ekos_observation_sdk::live_catalog::{self, LiveObject, OwnerCatalog};
use ekos_observation_sdk::profile::{self, Flavor, ProfileStats, ProfileTier, TableOutcome};
use ekos_observation_sdk::{ObservationPackage, ObserveError, Observer, ScanContext};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// The observer's name, and the `source_kind` its artifacts carry.
pub const OBSERVER: &str = "duckdb";
/// The target scheme: `duckdb://<database>/<schema>/…`.
pub const SCHEME: &str = "duckdb";
/// The catalog family recorded on every owner.
pub const FAMILY: &str = "duckdb_catalog";

/// Rows of text cells, `None` for SQL NULL.
pub type Rows = Vec<Vec<Option<String>>>;

/// What one read found.
#[derive(Debug, Clone)]
pub struct DatabaseReading {
    pub catalogs: Vec<OwnerCatalog>,
    pub write_check: Option<Value>,
}

/// `'text'`, quotes doubled.
fn literal(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

fn in_list(schemas: &[String]) -> String {
    schemas
        .iter()
        .map(|s| literal(s))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The filter every catalog query shares: the file's own database, the listed schemas, nothing
/// temporary.
fn scope(schemas: &[String]) -> String {
    format!(
        "database_name = current_database() AND schema_name IN ({}) AND NOT temporary",
        in_list(schemas)
    )
}

/// The catalog queries, in the order [`normalize`] takes their rows.
pub struct CatalogSql {
    pub census: String,
    pub tables: String,
    pub views: String,
    pub columns: String,
    pub constraints: String,
}

/// The queries for `schemas`. Each returns text cells only (name lists joined by U+001F).
pub fn catalog_sql(schemas: &[String]) -> CatalogSql {
    let w = scope(schemas);
    CatalogSql {
        census: format!(
            "SELECT schema_name, 'TABLE', count(*) FROM duckdb_tables() WHERE {w} GROUP BY 1 \
             UNION ALL SELECT schema_name, 'VIEW', count(*) FROM duckdb_views() \
             WHERE {w} AND NOT internal GROUP BY 1"
        ),
        tables: format!(
            "SELECT schema_name, table_name, estimated_size, comment FROM duckdb_tables() \
             WHERE {w} ORDER BY 1, 2"
        ),
        views: format!(
            "SELECT schema_name, view_name, sql, comment FROM duckdb_views() \
             WHERE {w} AND NOT internal ORDER BY 1, 2"
        ),
        columns: format!(
            "SELECT schema_name, table_name, column_name, data_type, is_nullable, column_default, \
             comment FROM duckdb_columns() WHERE database_name = current_database() \
             AND schema_name IN ({}) AND NOT internal \
             ORDER BY schema_name, table_name, column_index",
            in_list(schemas)
        ),
        constraints: format!(
            "SELECT schema_name, table_name, constraint_type, constraint_name, \
             array_to_string(constraint_column_names, chr(31)), referenced_table, \
             array_to_string(referenced_column_names, chr(31)), expression \
             FROM duckdb_constraints() WHERE database_name = current_database() \
             AND schema_name IN ({}) \
             AND constraint_type IN ('PRIMARY KEY', 'UNIQUE', 'CHECK', 'FOREIGN KEY') \
             ORDER BY 1, 2, 4",
            in_list(schemas)
        ),
    }
}

fn cell(row: &[Option<String>], i: usize) -> String {
    row.get(i).cloned().flatten().unwrap_or_default()
}

/// A name list the catalog query joined with the unit separator (no `json` extension needed).
fn names(joined: &str) -> Value {
    joined
        .split('\u{1f}')
        .filter(|n| !n.is_empty())
        .map(|n| Value::String(n.to_string()))
        .collect()
}

/// The rows of every catalog query as per-schema objects. Pure. Every listed schema gets a
/// catalog, even an empty one; text is redacted.
pub fn normalize(
    schemas: &[String],
    tables: &Rows,
    views: &Rows,
    columns: &Rows,
    constraints: &Rows,
    redaction: &RedactionConfig,
) -> Vec<OwnerCatalog> {
    let mut owners: BTreeMap<String, OwnerCatalog> = schemas
        .iter()
        .map(|s| {
            let c = OwnerCatalog {
                owner: s.clone(),
                family: FAMILY.into(),
                ..Default::default()
            };
            (s.clone(), c)
        })
        .collect();
    let mut by_table = table_columns(columns, redaction);
    for t in tables {
        let (schema, name) = (cell(t, 0), cell(t, 1));
        let Some(c) = owners.get_mut(&schema) else {
            continue;
        };
        let cols = by_table
            .remove(&(schema.clone(), name.clone()))
            .unwrap_or_default();
        c.columns += cols.len();
        let mut detail = json!({ "columns": cols });
        if let Ok(n) = cell(t, 2).parse::<i64>() {
            // P0: DuckDB's own estimate, exact for a file nothing is writing, but not promised.
            detail["num_rows"] = n.into();
            detail["profile"] = json!({ "tier": "p0", "row_count": n, "row_count_exact": false });
        }
        put_text(&mut detail, "comment", &cell(t, 3), redaction);
        c.objects.push(object("table", &schema, name, detail));
    }
    for v in views {
        let schema = cell(v, 0);
        let Some(c) = owners.get_mut(&schema) else {
            continue;
        };
        let mut detail = json!({ "text": redact(&view_query(&cell(v, 2)), redaction) });
        put_text(&mut detail, "comment", &cell(v, 3), redaction);
        c.objects.push(object("view", &schema, cell(v, 1), detail));
    }
    for k in constraints {
        if let (Some(c), Some(o)) = (owners.get_mut(&cell(k, 0)), constraint(k, redaction)) {
            c.objects.push(o);
        }
    }
    owners.into_values().collect()
}

/// The query of a view: `duckdb_views().sql` is the whole `CREATE VIEW … AS …;`, and the
/// analyzer re-heads the query with the catalog's name, as for every engine. Text that does not
/// parse is kept whole (the analyzer then counts the view as unparsed, never guesses).
fn view_query(sql: &str) -> String {
    use sqlparser::ast::Statement;
    let parsed = sqlparser::parser::Parser::parse_sql(&sqlparser::dialect::DuckDbDialect {}, sql);
    match parsed.as_deref() {
        Ok([Statement::CreateView { query, .. }]) => query.to_string(),
        _ => sql.to_string(),
    }
}

fn object(kind: &str, owner: &str, name: String, detail: Value) -> LiveObject {
    LiveObject {
        kind: kind.into(),
        owner: owner.into(),
        name,
        status: None,
        detail,
    }
}

fn put_text(detail: &mut Value, key: &str, text: &str, redaction: &RedactionConfig) {
    if !text.is_empty() {
        detail[key] = redact(text, redaction).into();
    }
}

/// (schema, table) → its columns, in order.
fn table_columns(
    columns: &Rows,
    redaction: &RedactionConfig,
) -> BTreeMap<(String, String), Vec<Value>> {
    let mut out: BTreeMap<(String, String), Vec<Value>> = BTreeMap::new();
    for r in columns {
        let mut col = json!({
            "name": cell(r, 2),
            "data_type": cell(r, 3),
            "nullable": cell(r, 4) != "false",
        });
        put_text(&mut col, "default", &cell(r, 5), redaction);
        put_text(&mut col, "comment", &cell(r, 6), redaction);
        out.entry((cell(r, 0), cell(r, 1))).or_default().push(col);
    }
    out
}

/// One `duckdb_constraints()` row as a live object; `None` for a type the analyzer does not read.
fn constraint(k: &[Option<String>], redaction: &RedactionConfig) -> Option<LiveObject> {
    let (schema, table, ty) = (cell(k, 0), cell(k, 1), cell(k, 2));
    let mut detail = json!({ "table": table, "columns": names(&cell(k, 4)), "validated": true });
    let kind = match ty.as_str() {
        "PRIMARY KEY" => "primary_key",
        "UNIQUE" => "unique_constraint",
        "CHECK" => {
            detail["condition"] = redact(&cell(k, 7), redaction).into();
            "check_constraint"
        }
        "FOREIGN KEY" => {
            detail["references_owner"] = schema.clone().into();
            detail["references_table"] = cell(k, 5).into();
            detail["references_columns"] = names(&cell(k, 6));
            // DuckDB has no cascading deletes.
            detail["on_delete"] = "no action".into();
            "foreign_key"
        }
        _ => return None,
    };
    let name = format!("{table}.{}", cell(k, 3));
    Some(object(kind, &schema, name, detail))
}

/// The census rows (`schema`, `TABLE`|`VIEW`, count) onto each catalog, then a check that every
/// counted object was read: a short catalog is an error, never a quiet gap.
pub fn reconcile(catalogs: &mut [OwnerCatalog], census: &Rows) -> Result<(), String> {
    for r in census {
        let (schema, ty) = (cell(r, 0), cell(r, 1));
        let n: usize = cell(r, 2).parse().unwrap_or(0);
        if let Some(c) = catalogs.iter_mut().find(|c| c.owner == schema) {
            c.census.entry(ty).or_default().valid += n;
        }
    }
    for c in catalogs.iter() {
        let listed = c.listed();
        for (ty, kind) in [("TABLE", "table"), ("VIEW", "view")] {
            let counted = c.census.get(ty).map_or(0, |x| x.valid);
            let read = listed.get(kind).copied().unwrap_or(0);
            if counted != read {
                return Err(format!(
                    "schema {}: duckdb counts {counted} {kind}s, the catalog read {read}",
                    c.owner
                ));
            }
        }
    }
    Ok(())
}

/// The statement the write probe sends: refused by the read-only session, so nothing is written.
pub fn write_probe_sql(catalogs: &[OwnerCatalog]) -> String {
    let table = catalogs
        .iter()
        .find_map(|c| c.objects.iter().find(|o| o.kind == "table"));
    match table {
        Some(t) => {
            let q = |s: &str| format!("\"{}\"", s.replace('"', "\"\""));
            let target = format!("{}.{}", q(&t.owner), q(&t.name));
            format!("INSERT INTO {target} SELECT * FROM {target} WHERE false")
        }
        None => "CREATE TABLE ekos_write_probe (x INTEGER)".into(),
    }
}

/// RFC 0211 §3.4: P1 / P2 over every table of `catalog`, one aggregate row per query through
/// `run`. Failures are recorded on their table.
pub fn profile_catalog(
    catalog: &mut OwnerCatalog,
    tier: ProfileTier,
    redaction: &RedactionConfig,
    run: &mut dyn FnMut(&str) -> Result<Rows, String>,
) -> ProfileStats {
    let mut stats = ProfileStats::new(tier);
    if tier == ProfileTier::P0 {
        return stats;
    }
    let owner = catalog.owner.clone();
    for o in catalog.objects.iter_mut().filter(|o| o.kind == "table") {
        let mut one = |sql: &str| -> Result<Vec<Option<String>>, String> {
            run(sql)?
                .into_iter()
                .next()
                .ok_or_else(|| "an aggregate query returned no row".to_string())
        };
        let outcome = profile::profile_table(
            Flavor::DuckDb,
            &owner,
            &o.name,
            &mut o.detail,
            tier,
            redaction,
            &mut one,
        );
        stats.record(&outcome.unwrap_or(TableOutcome::Failed));
    }
    stats
}

/// Read, reconcile, profile and probe a write. Blocking.
#[cfg(feature = "duckdb")]
pub fn read(
    path: &std::path::Path,
    schemas: &[String],
    redaction: &RedactionConfig,
    tier: ProfileTier,
) -> Result<DatabaseReading, String> {
    let session = crate::session::Session::open(path, crate::probe::DEFAULT_DEADLINE)?;
    let q = |sql: &str| session.query_text(sql).map(|p| p.rows);
    let sql = catalog_sql(schemas);
    let mut catalogs = normalize(
        schemas,
        &q(&sql.tables)?,
        &q(&sql.views)?,
        &q(&sql.columns)?,
        &q(&sql.constraints)?,
        redaction,
    );
    reconcile(&mut catalogs, &q(&sql.census)?)?;
    for c in &mut catalogs {
        let mut run = |s: &str| q(s);
        c.profile = Some(profile_catalog(c, tier, redaction, &mut run));
    }
    let write_check = live_catalog::write_check(session.execute(&write_probe_sql(&catalogs)));
    Ok(DatabaseReading {
        catalogs,
        write_check: Some(write_check),
    })
}

/// Without the engine nothing is opened.
#[cfg(not(feature = "duckdb"))]
pub fn read(
    _path: &std::path::Path,
    _schemas: &[String],
    _redaction: &RedactionConfig,
    _tier: ProfileTier,
) -> Result<DatabaseReading, String> {
    Err(crate::NOT_BUILT.into())
}

/// RFC 0223 / 0235: what `ekos database test` reports — the same JSON as every other kind.
pub fn test_connection(
    path: &std::path::Path,
    schemas: &[String],
    redaction: &RedactionConfig,
) -> Result<Value, String> {
    let reading = read(path, schemas, redaction, ProfileTier::P0)?;
    Ok(test_report(&format!("duckdb:{}", path.display()), &reading))
}

/// The report of one reading. Pure.
pub fn test_report(label: &str, reading: &DatabaseReading) -> Value {
    json!({
        "label": label,
        "family": FAMILY,
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

/// One `[[databases]]` entry of kind `duckdb`.
pub struct DuckDbObserver {
    database: String,
    path: PathBuf,
    schemas: Vec<String>,
    redaction: RedactionConfig,
    profile: ProfileTier,
}

impl DuckDbObserver {
    pub fn new(
        database: impl Into<String>,
        path: PathBuf,
        schemas: Vec<String>,
        redaction: RedactionConfig,
        profile: ProfileTier,
    ) -> Self {
        Self {
            database: database.into(),
            path,
            schemas,
            redaction,
            profile,
        }
    }
}

#[async_trait::async_trait]
impl Observer for DuckDbObserver {
    fn name(&self) -> &str {
        OBSERVER
    }

    async fn scan(&self, _ctx: &ScanContext) -> Result<ObservationPackage, ObserveError> {
        let (path, schemas) = (self.path.clone(), self.schemas.clone());
        let (redaction, tier) = (self.redaction.clone(), self.profile);
        let reading = tokio::task::spawn_blocking(move || read(&path, &schemas, &redaction, tier))
            .await
            .map_err(|e| ObserveError::connector(format!("duckdb reader stopped: {e}")))?
            .map_err(|e| {
                ObserveError::connector(format!("duckdb database {}: {e}", self.database))
            })?;
        let mut pkg = ObservationPackage::new(OBSERVER, "database");
        for a in live_catalog::artifacts(
            OBSERVER,
            "ekos-plugin-duckdb",
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

    fn rows(v: &[&[&str]]) -> Rows {
        v.iter()
            .map(|r| {
                r.iter()
                    .map(|c| (!c.is_empty()).then(|| c.to_string()))
                    .collect()
            })
            .collect()
    }

    fn reading() -> Vec<OwnerCatalog> {
        let schemas = vec!["gold".to_string(), "empty".to_string()];
        normalize(
            &schemas,
            &rows(&[&["gold", "fct_invoice", "3333", "Invoices by status"]]),
            &rows(&[&["gold", "v_open", "CREATE VIEW v_open AS SELECT 1 AS x;", ""]]),
            &rows(&[
                &["gold", "fct_invoice", "id", "BIGINT", "false", "", ""],
                &[
                    "gold",
                    "fct_invoice",
                    "total",
                    "DECIMAL(18,2)",
                    "true",
                    "0",
                    "key AKIAIOSFODNN7EXAMPLE",
                ],
            ]),
            &rows(&[
                &[
                    "gold",
                    "fct_invoice",
                    "PRIMARY KEY",
                    "fct_invoice_id_pkey",
                    "id",
                    "",
                    "",
                    "",
                ],
                &[
                    "gold",
                    "fct_invoice",
                    "CHECK",
                    "c1",
                    "total",
                    "",
                    "",
                    "(total >= 0)",
                ],
                &[
                    "gold",
                    "fct_line",
                    "FOREIGN KEY",
                    "fk1",
                    "invoice_id",
                    "fct_invoice",
                    "id",
                    "",
                ],
                &["gold", "fct_invoice", "NOT NULL", "nn", "id", "", "", ""],
            ]),
            &RedactionConfig::default(),
        )
    }

    #[test]
    fn rows_become_the_live_catalog_shape() {
        let c = reading();
        assert_eq!(c.len(), 2, "every listed schema gets a catalog");
        let gold = c.iter().find(|c| c.owner == "gold").unwrap();
        assert_eq!(gold.columns, 2);
        let t = &gold.objects[0];
        assert_eq!((t.kind.as_str(), t.name.as_str()), ("table", "fct_invoice"));
        assert_eq!(t.detail["num_rows"], 3333);
        assert_eq!(t.detail["columns"][0]["nullable"], false);
        assert_eq!(t.detail["columns"][1]["data_type"], "DECIMAL(18,2)");
        assert!(
            !t.detail.to_string().contains("AKIAIOSFODNN7EXAMPLE"),
            "comments are redacted: {}",
            t.detail
        );
        let kinds: Vec<&str> = gold.objects.iter().map(|o| o.kind.as_str()).collect();
        assert_eq!(
            kinds,
            [
                "table",
                "view",
                "primary_key",
                "check_constraint",
                "foreign_key"
            ]
        );
        let fk = &gold.objects[4];
        assert_eq!(fk.name, "fct_line.fk1");
        assert_eq!(fk.detail["references_columns"], json!(["id"]));
        assert_eq!(gold.objects[3].detail["condition"], "(total >= 0)");
        assert_eq!(gold.objects[1].detail["text"], "SELECT 1 AS x");
    }

    #[test]
    fn a_short_catalog_is_an_error() {
        let mut c = reading();
        let census = rows(&[&["gold", "TABLE", "1"], &["gold", "VIEW", "1"]]);
        reconcile(&mut c, &census).unwrap();
        assert_eq!(c[1].census["TABLE"].valid, 1);
        let mut c = reading();
        let more = rows(&[&["gold", "TABLE", "2"], &["gold", "VIEW", "1"]]);
        let e = reconcile(&mut c, &more).unwrap_err();
        assert!(e.contains("counts 2 tables, the catalog read 1"), "{e}");
    }

    #[test]
    fn the_queries_stay_inside_the_listed_schemas_of_this_file() {
        let sql = catalog_sql(&["gold".into(), "o'neil".into()]);
        for q in [
            &sql.census,
            &sql.tables,
            &sql.views,
            &sql.columns,
            &sql.constraints,
        ] {
            assert!(q.contains("database_name = current_database()"), "{q}");
            assert!(q.contains("IN ('gold', 'o''neil')"), "{q}");
        }
        let c = reading();
        assert_eq!(
            write_probe_sql(&c),
            "INSERT INTO \"gold\".\"fct_invoice\" SELECT * FROM \"gold\".\"fct_invoice\" WHERE false"
        );
    }

    /// RFC 0252's rule: a profile query returns counts and bounds of numbers and dates, never a
    /// text value.
    #[test]
    fn a_p2_profile_never_selects_a_text_value() {
        let mut gold = reading().into_iter().find(|c| c.owner == "gold").unwrap();
        gold.objects[0].detail["columns"]
            .as_array_mut()
            .unwrap()
            .push(json!({ "name": "docstatus", "data_type": "VARCHAR", "nullable": true }));
        let mut seen = Vec::new();
        let mut run = |sql: &str| -> Result<Rows, String> {
            seen.push(sql.to_string());
            Ok(vec![vec![Some("0".into()); 64]])
        };
        let stats = profile_catalog(
            &mut gold,
            ProfileTier::P2,
            &RedactionConfig::default(),
            &mut run,
        );
        assert_eq!(stats.tables_profiled, 1);
        let all = seen.join("\n");
        assert!(!all.contains("min(\"docstatus\")"), "{all}");
        assert!(!all.contains("max(\"docstatus\")"), "{all}");
        assert!(all.contains("\"gold\".\"fct_invoice\""), "{all}");
    }

    #[cfg(not(feature = "duckdb"))]
    #[test]
    fn without_the_engine_a_read_says_so() {
        let e = read(
            std::path::Path::new("/nonexistent.duckdb"),
            &["main".into()],
            &RedactionConfig::default(),
            ProfileTier::P0,
        )
        .unwrap_err();
        assert!(e.contains("duckdb feature"), "{e}");
    }
}
