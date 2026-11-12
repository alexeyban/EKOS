//! RFC 0252 §8 — the data-seeded signals of RFC 0252 for a benchmark's SQLite database.
//!
//! `ekos relationships discover` reads live `[[databases]]`; a benchmark database is a SQLite file
//! read in process, so this module runs the same steps on it: an RFC 0252 MinHash signature of
//! every comparable column (`ekos_observation_sdk::overlap::signature_sql`, `Flavor::Sqlite`,
//! computed by SQLite with an `md5` function registered on the connection — only digest minimums
//! leave the query), the shared proposal rule (`overlap::data_pairs`: LSH bands, containment,
//! names as a tie-breaker), and the exact inclusion check (`overlap::inclusion_sql`). A pair the
//! data does not bear out is dropped, as `discover` drops it. Declared foreign keys are never
//! proposed.
//!
//! Also the single-column keys of tables that declare none (measured: distinct = rows, no null —
//! RFC 0252 §3.2(d) at width 1).
//!
//! The result holds names and counts only, never a value, and is written by `prepare` to
//! `data/values/<db>.signals.json` (beside the RFC 0241 profile, never a ledger).

use super::context::SchemaInfo;
use super::sqlexec::Db;
use anyhow::{Result, anyhow};
use ekos_observation_sdk::overlap::{self, DataPairRules, SignedColumn};
use ekos_observation_sdk::profile::{self, Family, Flavor};
use md5::{Digest, Md5};
use rusqlite::functions::FunctionFlags;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;

/// Bump when the output changes; `prepare` rebuilds an older file.
pub const SIGNALS_VERSION: u32 = 1;

/// `discover`'s defaults (`[relationships]`).
pub const RULES: DataPairRules = DataPairRules {
    min_containment: 0.9,
    min_parent_distinct: 20,
};

/// The schema owner SQLite calls the main database.
const OWNER: &str = "main";

/// One reference found in the data and verified by the inclusion check.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DataJoin {
    pub child_table: String,
    pub child_column: String,
    pub parent_table: String,
    pub parent_column: String,
    pub containment_estimate: f64,
    pub population: i64,
    pub orphans: i64,
    /// RFC 0158's verdict.
    pub verdict: String,
    pub name_affinity: bool,
}

/// What the data says about one database.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DataSignals {
    pub version: u32,
    pub columns_signed: usize,
    pub pairs_checked: usize,
    pub joins: Vec<DataJoin>,
    /// `(table, column)`: a measured single-column key of a table that declares no key.
    pub keys: Vec<(String, String)>,
}

impl DataSignals {
    /// The pack lines: `child.col = parent.col` per reference, `table(column)` per key.
    pub fn join_lines(&self) -> Vec<String> {
        self.joins
            .iter()
            .map(|j| {
                format!(
                    "{}.{} = {}.{}",
                    j.child_table, j.child_column, j.parent_table, j.parent_column
                )
            })
            .collect()
    }
}

/// The suite's salt (`data/values/salt`): 32 hex digits, created once, readable by the owner only.
/// It keys the signatures and is never written beside them.
pub fn salt(values_dir: &std::path::Path) -> Result<String> {
    use sha2::Digest as _;
    let path = values_dir.join("salt");
    if let Ok(s) = std::fs::read_to_string(&path) {
        let s = s.trim().to_string();
        if !s.is_empty() && s.as_bytes().iter().all(u8::is_ascii_alphanumeric) {
            return Ok(s);
        }
    }
    std::fs::create_dir_all(values_dir)?;
    let seed = format!(
        "{:?}{}{:p}",
        std::time::SystemTime::now(),
        std::process::id(),
        &path
    );
    let s = hex::encode(sha2::Sha256::digest(seed.as_bytes()))[..32].to_string();
    std::fs::write(&path, &s)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(s)
}

/// Registers `md5(text)` (lowercase hex; `NULL` for `NULL`) on the connection.
pub fn register_md5(db: &Db) -> Result<()> {
    db.connection().create_scalar_function(
        "md5",
        1,
        FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
        |ctx| {
            let v: Option<String> = ctx.get(0)?;
            Ok(v.map(|s| hex::encode(Md5::digest(s.as_bytes()))))
        },
    )?;
    Ok(())
}

fn number(db: &Db, sql: &str) -> Result<i64> {
    Ok(db.connection().query_row(sql, [], |r| r.get::<_, i64>(0))?)
}

/// A row of text cells (SQLite returns what it computed; numbers are rendered).
fn text_row(db: &Db, sql: &str) -> Result<Vec<Option<String>>> {
    let mut stmt = db.connection().prepare(sql)?;
    let n = stmt.column_count();
    let row = stmt.query_row([], |r| {
        (0..n)
            .map(|i| {
                Ok(match r.get_ref(i)? {
                    rusqlite::types::ValueRef::Null => None,
                    rusqlite::types::ValueRef::Integer(x) => Some(x.to_string()),
                    rusqlite::types::ValueRef::Real(x) => Some(x.to_string()),
                    rusqlite::types::ValueRef::Text(t) => {
                        Some(String::from_utf8_lossy(t).into_owned())
                    }
                    rusqlite::types::ValueRef::Blob(_) => None,
                })
            })
            .collect::<rusqlite::Result<Vec<_>>>()
    })?;
    Ok(row)
}

/// Declared foreign keys as (child table, child column, parent table, parent column), lowercase.
fn declared(db: &Db, schema: &SchemaInfo) -> Result<BTreeSet<(String, String, String, String)>> {
    let mut out = BTreeSet::new();
    for t in &schema.table_names {
        let mut stmt = db.connection().prepare(&format!(
            "PRAGMA foreign_key_list({})",
            profile::ident(Flavor::Sqlite, t).map_err(|e| anyhow!(e))?
        ))?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, Option<String>>(4)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        for (to, from, to_col) in rows {
            let to_col = to_col.unwrap_or_else(|| {
                schema
                    .tables
                    .get(&to.to_lowercase())
                    .and_then(|cs| cs.iter().find(|c| c.2 > 0).map(|c| c.0.clone()))
                    .unwrap_or_default()
            });
            out.insert((
                t.to_lowercase(),
                from.to_lowercase(),
                to.to_lowercase(),
                to_col.to_lowercase(),
            ));
        }
    }
    Ok(out)
}

/// One table's signed columns, and its measured single-column key when it declares none.
fn sign_table(
    db: &Db,
    schema: &SchemaInfo,
    table: &str,
    salt: &str,
) -> Result<(Vec<SignedColumn>, Option<String>)> {
    let cols = &schema.tables[&table.to_lowercase()];
    let f = Flavor::Sqlite;
    let tbl = profile::table_ref(f, OWNER, table).map_err(|e| anyhow!(e))?;
    let rows = number(db, &format!("SELECT COUNT(*) FROM {tbl}"))?;
    let declared_pk: Vec<&str> = cols
        .iter()
        .filter(|c| c.2 > 0)
        .map(|c| c.0.as_str())
        .collect();
    // `signature_columns` leaves out text longer than 64 characters, read from a profile.
    let mut described = Vec::new();
    for (name, ty, _) in cols {
        let mut c = json!({"name": name, "data_type": ty});
        if profile::family(f, ty) == Family::Text {
            let col = profile::ident(f, name).map_err(|e| anyhow!(e))?;
            let len = number(
                db,
                &format!("SELECT COALESCE(MAX(LENGTH({col})), 0) FROM {tbl}"),
            )?;
            c["profile"] = json!({"max_length": len});
        }
        described.push(c);
    }
    let mut out = Vec::new();
    let mut key = None;
    for name in overlap::signature_columns(f, &described) {
        let ty = cols
            .iter()
            .find(|c| c.0 == name)
            .map(|c| c.1.as_str())
            .unwrap_or("");
        let numeric = profile::family(f, ty) == Family::Number;
        let sql = overlap::signature_sql(f, OWNER, table, &name, salt, numeric)
            .map_err(|e| anyhow!(e))?;
        let Some(sig) = overlap::parse_signature(&text_row(db, &sql)?) else {
            continue;
        };
        if sig.distinct == 0 {
            continue;
        }
        let col = profile::ident(f, &name).map_err(|e| anyhow!(e))?;
        let measured = rows > 0
            && sig.distinct == rows
            && number(
                db,
                &format!("SELECT COUNT(*) FROM {tbl} WHERE {col} IS NULL"),
            )? == 0;
        if declared_pk.is_empty() && measured && key.is_none() {
            key = Some(name.clone());
        }
        out.push(SignedColumn {
            table_key: table.to_lowercase(),
            table: table.to_string(),
            unique: (declared_pk.len() == 1 && declared_pk[0] == name) || measured,
            column: name,
            numeric,
            sig,
        });
    }
    Ok((out, key))
}

/// RFC 0252's data-seeded discovery and key search on one SQLite database.
pub fn discover(db: &Db, schema: &SchemaInfo, salt: &str) -> Result<DataSignals> {
    register_md5(db)?;
    let declared = declared(db, schema)?;
    let mut cols = Vec::new();
    let mut keys = Vec::new();
    for t in &schema.table_names {
        let (signed, key) = sign_table(db, schema, t, salt)?;
        cols.extend(signed);
        if let Some(k) = key {
            keys.push((t.clone(), k));
        }
    }
    let is_declared = |c: &SignedColumn, p: &SignedColumn| {
        declared.contains(&(
            c.table.to_lowercase(),
            c.column.to_lowercase(),
            p.table.to_lowercase(),
            p.column.to_lowercase(),
        ))
    };
    let pairs = overlap::data_pairs(&cols, RULES, &is_declared);
    let mut joins = Vec::new();
    for &(c, p, containment) in &pairs {
        let (c, p) = (&cols[c], &cols[p]);
        let sql = overlap::inclusion_sql(
            Flavor::Sqlite,
            (OWNER, &c.table, &c.column),
            (OWNER, &p.table, &p.column),
        )
        .map_err(|e| anyhow!(e))?;
        let row = text_row(db, &sql)?;
        let n = |i: usize| -> i64 {
            row.get(i)
                .and_then(Option::as_deref)
                .and_then(|v| v.parse().ok())
                .unwrap_or(0)
        };
        let r = ekos_migrate_dq::infer::InclusionResult {
            population: n(0),
            orphans: n(1),
        };
        let verdict = r.verdict();
        if !(verdict.is_relationship() || verdict.as_str() == "unclear") {
            continue;
        }
        joins.push(DataJoin {
            child_table: c.table.clone(),
            child_column: c.column.clone(),
            parent_table: p.table.clone(),
            parent_column: p.column.clone(),
            containment_estimate: (containment * 1000.0).round() / 1000.0,
            population: r.population,
            orphans: r.orphans,
            verdict: verdict.as_str().to_string(),
            name_affinity: overlap::name_affinity(&c.column, &p.table, &p.column),
        });
    }
    joins.sort_by(|a, b| {
        (
            &a.child_table,
            &a.child_column,
            &a.parent_table,
            &a.parent_column,
        )
            .cmp(&(
                &b.child_table,
                &b.child_column,
                &b.parent_table,
                &b.parent_column,
            ))
    });
    Ok(DataSignals {
        version: SIGNALS_VERSION,
        columns_signed: cols.len(),
        pairs_checked: pairs.len(),
        joins,
        keys,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (tempfile::TempDir, Db) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f.sqlite");
        let c = rusqlite::Connection::open(&path).unwrap();
        c.execute_batch(
            "CREATE TABLE customer (id INTEGER PRIMARY KEY, code TEXT);
             CREATE TABLE invoice (id INTEGER PRIMARY KEY, cust_ref TEXT, amount INTEGER);
             CREATE TABLE note (body TEXT);",
        )
        .unwrap();
        for i in 0..60 {
            c.execute(
                "INSERT INTO customer VALUES (?1, ?2)",
                rusqlite::params![i, format!("C{:04}", i * 7)],
            )
            .unwrap();
        }
        for i in 0..200 {
            c.execute(
                "INSERT INTO invoice VALUES (?1, ?2, ?3)",
                rusqlite::params![i, format!("C{:04}", (i % 50) * 7), i * 3],
            )
            .unwrap();
            c.execute(
                "INSERT INTO note VALUES (?1)",
                rusqlite::params![format!("n{i}")],
            )
            .unwrap();
        }
        drop(c);
        let db = Db::open(&path).unwrap();
        (dir, db)
    }

    #[test]
    fn an_undeclared_reference_is_found_and_verified() {
        let (_d, db) = fixture();
        let schema = SchemaInfo::read(&db).unwrap();
        let s = discover(&db, &schema, "salt1").unwrap();
        let j = s
            .joins
            .iter()
            .find(|j| j.child_column == "cust_ref")
            .expect("invoice.cust_ref → customer.code");
        assert_eq!(
            (j.parent_table.as_str(), j.parent_column.as_str()),
            ("customer", "code")
        );
        assert_eq!((j.population, j.orphans), (200, 0));
        assert_eq!(j.verdict, "clean_relationship");
        // A key column with no name pointing anywhere is never a child of another key.
        assert!(!s.joins.iter().any(|j| j.child_column == "id"));
        // `note` declares no key; its one column is measured unique.
        assert_eq!(s.keys, vec![("note".to_string(), "body".to_string())]);
    }

    #[test]
    fn the_result_carries_no_value() {
        let (_d, db) = fixture();
        let schema = SchemaInfo::read(&db).unwrap();
        let text = serde_json::to_string(&discover(&db, &schema, "salt1").unwrap()).unwrap();
        assert!(!text.contains("C0007"), "{text}");
        assert!(!text.contains("n17"), "{text}");
    }

    #[test]
    fn the_pack_lists_data_joins_after_the_declared_ones() {
        let (_d, db) = fixture();
        let schema = SchemaInfo::read(&db).unwrap();
        let s = discover(&db, &schema, "salt1").unwrap();
        let pack = super::super::context::DbPack {
            joins: vec!["a.x = b.y".into()],
            data_joins: s.join_lines(),
            data_keys: s.keys.clone(),
            ..Default::default()
        };
        let text = pack.render();
        let declared = text.find("-- Join paths (foreign keys):").unwrap();
        let found = text.find("-- Join paths found in the data").unwrap();
        assert!(declared < found, "{text}");
        assert!(
            text.contains("--   invoice.cust_ref = customer.code"),
            "{text}"
        );
        assert!(text.contains("--   note(body)"), "{text}");
    }

    #[test]
    fn md5_matches_the_other_engines() {
        let (_d, db) = fixture();
        register_md5(&db).unwrap();
        let h: String = db
            .connection()
            .query_row("SELECT md5('abc')", [], |r| r.get(0))
            .unwrap();
        assert_eq!(h, "900150983cd24fb0d6963f7d28e17f72");
    }
}
