//! RFC 0211 §3.3 — catalog introspection.
//!
//! The output is the denominator every Oracle coverage number is a fraction of, so this module does
//! not trust its own enumeration: each kind it reads is reconciled against the dictionary's own
//! object list, and an object type it does not know is an error rather than a skipped row.
//!
//! Every read goes through [`CatalogSource`], which the live session implements and tests replace
//! with recorded rows. The SQL is written for one *family* of dictionary views ([`ViewFamily`]):
//! `DBA_*` when the user holds `SELECT_CATALOG_ROLE`, `ALL_*` otherwise. The difference matters for
//! a read-only user: `ALL_SOURCE` and `ALL_TRIGGERS` list only what the user may *execute*, so with
//! `SELECT` alone every routine would be missing from both the list and the count it is checked
//! against.

use crate::OraError;
use ekos_common::redaction::{RedactionConfig, redact};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Which dictionary views a snapshot was read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewFamily {
    Dba,
    All,
}

impl ViewFamily {
    pub const fn prefix(self) -> &'static str {
        match self {
            Self::Dba => "dba",
            Self::All => "all",
        }
    }
}

/// The catalog object kinds this module reads and lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectKind {
    Table,
    View,
    MaterializedView,
    Sequence,
    Index,
    Function,
    Procedure,
    Package,
    PackageBody,
    Trigger,
    Type,
    TypeBody,
    Synonym,
    PrimaryKey,
    ForeignKey,
    UniqueConstraint,
    CheckConstraint,
}

impl ObjectKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Table => "table",
            Self::View => "view",
            Self::MaterializedView => "materialized_view",
            Self::Sequence => "sequence",
            Self::Index => "index",
            Self::Function => "function",
            Self::Procedure => "procedure",
            Self::Package => "package",
            Self::PackageBody => "package_body",
            Self::Trigger => "trigger",
            Self::Type => "type",
            Self::TypeBody => "type_body",
            Self::Synonym => "synonym",
            Self::PrimaryKey => "primary_key",
            Self::ForeignKey => "foreign_key",
            Self::UniqueConstraint => "unique_constraint",
            Self::CheckConstraint => "check_constraint",
        }
    }

    /// The `OBJECT_TYPE` this kind is counted under in `*_OBJECTS`, when it is a schema object.
    pub const fn object_type(self) -> Option<&'static str> {
        match self {
            Self::Table => Some("TABLE"),
            Self::View => Some("VIEW"),
            Self::MaterializedView => Some("MATERIALIZED VIEW"),
            Self::Sequence => Some("SEQUENCE"),
            Self::Index => Some("INDEX"),
            Self::Function => Some("FUNCTION"),
            Self::Procedure => Some("PROCEDURE"),
            Self::Package => Some("PACKAGE"),
            Self::PackageBody => Some("PACKAGE BODY"),
            Self::Trigger => Some("TRIGGER"),
            Self::Type => Some("TYPE"),
            Self::TypeBody => Some("TYPE BODY"),
            Self::Synonym => Some("SYNONYM"),
            Self::PrimaryKey
            | Self::ForeignKey
            | Self::UniqueConstraint
            | Self::CheckConstraint => None,
        }
    }

    fn from_source_type(t: &str) -> Option<Self> {
        Some(match t {
            "FUNCTION" => Self::Function,
            "PROCEDURE" => Self::Procedure,
            "PACKAGE" => Self::Package,
            "PACKAGE BODY" => Self::PackageBody,
            "TRIGGER" => Self::Trigger,
            "TYPE" => Self::Type,
            "TYPE BODY" => Self::TypeBody,
            _ => return None,
        })
    }
}

/// Object types counted in the census but not listed: storage the listed kinds already describe
/// (LOB segments, partitions) or content outside this RFC (Java, jobs, links). Anything neither
/// here nor in [`ObjectKind`] is [`OraError::UnknownObjectType`].
pub const COUNTED_ONLY: &[&str] = &[
    "LOB",
    "LOB PARTITION",
    "LOB SUBPARTITION",
    "TABLE PARTITION",
    "TABLE SUBPARTITION",
    "INDEX PARTITION",
    "INDEX SUBPARTITION",
    "JAVA CLASS",
    "JAVA SOURCE",
    "JAVA RESOURCE",
    "JAVA DATA",
    "JOB",
    "PROGRAM",
    "SCHEDULE",
    "DATABASE LINK",
    "LIBRARY",
    "DIRECTORY",
    "QUEUE",
    "RULE",
    "RULE SET",
    "EVALUATION CONTEXT",
    "CLUSTER",
    "DIMENSION",
    "OPERATOR",
    "INDEXTYPE",
    "XML SCHEMA",
    "CREDENTIAL",
];

/// One catalog object as a fact. `detail` is kind-specific and **redacted** before it gets here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CatalogObject {
    pub kind: ObjectKind,
    pub owner: String,
    pub name: String,
    /// `VALID` / `INVALID` from `*_OBJECTS`; constraints carry their own `ENABLED`/`DISABLED`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    pub detail: serde_json::Value,
}

/// Counts by `OBJECT_TYPE` and status, from `*_OBJECTS`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Census {
    pub valid: usize,
    pub invalid: usize,
}

/// Everything one owner's catalog contains.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogSnapshot {
    pub owner: String,
    pub family: ViewFamily,
    pub objects: Vec<CatalogObject>,
    /// `*_OBJECTS` by `OBJECT_TYPE`.
    pub census: BTreeMap<String, Census>,
    /// Columns read (table and view columns).
    pub columns: usize,
    /// System-named `"C" IS NOT NULL` checks, folded into column nullability.
    pub folded_not_null: usize,
    /// Constraint types this module does not list (`V` view check option, `O` read-only view, …).
    pub other_constraints: BTreeMap<String, usize>,
}

impl CatalogSnapshot {
    pub fn of_kind(&self, kind: ObjectKind) -> impl Iterator<Item = &CatalogObject> {
        self.objects.iter().filter(move |o| o.kind == kind)
    }

    pub fn counts(&self) -> BTreeMap<ObjectKind, usize> {
        let mut m = BTreeMap::new();
        for o in &self.objects {
            *m.entry(o.kind).or_insert(0) += 1;
        }
        m
    }
}

/// One query result: rows of nullable strings, in the select list's order. Numbers and dates come
/// back as the session renders them, which is why the session pins its NLS settings.
pub type Rows = Vec<Vec<Option<String>>>;

/// Where catalog rows come from: the live session, or recorded rows in tests.
pub trait CatalogSource {
    /// Run `sql` with `owner` bound to `:1`.
    fn rows(&self, query: &Query, owner: &str) -> Result<Rows, OraError>;

    /// Start one read-only transaction, so every query of an introspection sees the same
    /// snapshot: a table created between two queries can then not make a list and a count disagree.
    fn begin_snapshot(&self) -> Result<(), OraError> {
        Ok(())
    }

    /// End it.
    fn end_snapshot(&self) -> Result<(), OraError> {
        Ok(())
    }
}

/// A named catalog query. The name lets a recorded source answer without matching SQL text.
#[derive(Debug, Clone)]
pub struct Query {
    pub name: &'static str,
    pub sql: String,
}

/// The catalog queries, for one view family.
pub fn queries(family: ViewFamily) -> Vec<Query> {
    let f = family.prefix();
    let q = |name, sql: String| Query { name, sql };
    vec![
        q(
            "objects",
            format!(
                "SELECT object_type, object_name, status FROM {f}_objects \
                 WHERE owner = :1 AND object_name NOT LIKE 'BIN$%' AND subobject_name IS NULL"
            ),
        ),
        q(
            "tables",
            format!(
                "SELECT table_name, temporary, iot_type, partitioned, num_rows, last_analyzed \
                 FROM {f}_tables WHERE owner = :1 AND dropped = 'NO' AND nested = 'NO'"
            ),
        ),
        q(
            "columns",
            format!(
                "SELECT table_name, column_name, column_id, data_type, data_length, \
                 data_precision, data_scale, char_length, char_used, nullable, data_default \
                 FROM {f}_tab_columns WHERE owner = :1 AND table_name NOT LIKE 'BIN$%' \
                 ORDER BY table_name, column_id"
            ),
        ),
        q(
            "constraints",
            format!(
                "SELECT constraint_name, constraint_type, table_name, search_condition, \
                 r_owner, r_constraint_name, delete_rule, status, validated, generated \
                 FROM {f}_constraints WHERE owner = :1 AND table_name NOT LIKE 'BIN$%'"
            ),
        ),
        q(
            "constraint_columns",
            format!(
                "SELECT constraint_name, column_name, position FROM {f}_cons_columns \
                 WHERE owner = :1 AND table_name NOT LIKE 'BIN$%' \
                 ORDER BY constraint_name, position"
            ),
        ),
        q(
            "indexes",
            format!(
                "SELECT index_name, table_name, index_type, uniqueness, status, generated \
                 FROM {f}_indexes WHERE owner = :1 AND index_name NOT LIKE 'BIN$%'"
            ),
        ),
        q(
            "index_columns",
            format!(
                "SELECT index_name, column_name, column_position FROM {f}_ind_columns \
                 WHERE index_owner = :1 AND index_name NOT LIKE 'BIN$%' \
                 ORDER BY index_name, column_position"
            ),
        ),
        q(
            "views",
            format!("SELECT view_name, text_length, text FROM {f}_views WHERE owner = :1"),
        ),
        q(
            "mviews",
            format!("SELECT mview_name, refresh_mode FROM {f}_mviews WHERE owner = :1"),
        ),
        q(
            "sequences",
            format!(
                "SELECT sequence_name, increment_by, cache_size FROM {f}_sequences \
                 WHERE sequence_owner = :1"
            ),
        ),
        q(
            "source",
            format!(
                "SELECT name, type, line, text FROM {f}_source WHERE owner = :1 \
                 ORDER BY name, type, line"
            ),
        ),
        q(
            "triggers",
            format!(
                "SELECT trigger_name, trigger_type, triggering_event, table_name, \
                 base_object_type, status FROM {f}_triggers WHERE owner = :1"
            ),
        ),
        q(
            "synonyms",
            format!(
                "SELECT synonym_name, table_owner, table_name, db_link FROM {f}_synonyms \
                 WHERE owner = :1"
            ),
        ),
        q(
            "table_comments",
            format!(
                "SELECT table_name, comments FROM {f}_tab_comments \
                 WHERE owner = :1 AND comments IS NOT NULL"
            ),
        ),
        q(
            "column_comments",
            format!(
                "SELECT table_name, column_name, comments FROM {f}_col_comments \
                 WHERE owner = :1 AND comments IS NOT NULL"
            ),
        ),
        // Profile tier P0 (§3.4): optimizer statistics, no scan. `LOW_VALUE` / `HIGH_VALUE` are
        // never selected — they are raw row values.
        q(
            "column_statistics",
            format!(
                "SELECT table_name, column_name, num_nulls, num_distinct, avg_col_len, \
                 last_analyzed FROM {f}_tab_col_statistics \
                 WHERE owner = :1 AND table_name NOT LIKE 'BIN$%'"
            ),
        ),
    ]
}

fn cell(row: &[Option<String>], i: usize) -> Option<&str> {
    row.get(i).and_then(|c| c.as_deref())
}

fn text(row: &[Option<String>], i: usize) -> String {
    cell(row, i).unwrap_or_default().to_string()
}

fn number(row: &[Option<String>], i: usize) -> Option<i64> {
    cell(row, i).and_then(|v| v.trim().parse().ok())
}

/// Read one owner's catalog, then reconcile it.
pub fn introspect(
    source: &dyn CatalogSource,
    owner: &str,
    family: ViewFamily,
    redaction: &RedactionConfig,
) -> Result<CatalogSnapshot, OraError> {
    let mut results: BTreeMap<&'static str, Rows> = BTreeMap::new();
    source.begin_snapshot()?;
    let read = queries(family).into_iter().try_for_each(|q| {
        let started = std::time::Instant::now();
        let rows = source.rows(&q, owner)?;
        ekos_common::run_log::emit(
            "oracle.catalog",
            &format!(
                "read {}",
                source_view(&q.sql).unwrap_or_else(|| q.name.to_string())
            ),
            &serde_json::json!({
                "owner": owner,
                "query": q.name,
                "rows": rows.len(),
                "ms": started.elapsed().as_millis() as u64,
            }),
        );
        results.insert(q.name, rows);
        Ok::<_, OraError>(())
    });
    let ended = source.end_snapshot();
    read?;
    ended?;
    let get = |name: &str| results.get(name).map(Vec::as_slice).unwrap_or_default();
    let r = |s: &str| redact(s, redaction);

    let (census, status) = census_of(get("objects"))?;
    let mut snapshot = CatalogSnapshot {
        owner: owner.to_string(),
        family,
        objects: Vec::new(),
        census,
        columns: 0,
        folded_not_null: 0,
        other_constraints: BTreeMap::new(),
    };
    let status_of = |t: &str, name: &str| status.get(&(t.to_string(), name.to_string())).cloned();
    let comments = table_comments(get("table_comments"), &r);
    let column_comments = column_comments(get("column_comments"), &r);
    let statistics = column_statistics(get("column_statistics"));
    let mut columns = columns_by_table(get("columns"), &column_comments, &statistics, &r);
    snapshot.columns = columns.values().map(Vec::len).sum();

    for row in get("tables") {
        let name = text(row, 0);
        let mut detail = serde_json::json!({
            "temporary": cell(row, 1) == Some("Y"),
            "partitioned": cell(row, 3) == Some("YES"),
            "columns": columns.remove(&name).unwrap_or_default(),
        });
        if let Some(iot) = cell(row, 2) {
            detail["iot_type"] = iot.into();
        }
        if let Some(n) = number(row, 4) {
            detail["num_rows"] = n.into();
            detail["profile"] = crate::profile::TableProfile::from_statistics(n).into_json();
        }
        if let Some(t) = cell(row, 5) {
            detail["last_analyzed"] = t.into();
        }
        if let Some(c) = comments.get(&name) {
            detail["comment"] = c.as_str().into();
        }
        snapshot.objects.push(CatalogObject {
            kind: ObjectKind::Table,
            owner: owner.to_string(),
            status: status_of("TABLE", &name),
            name,
            detail,
        });
    }

    for row in get("views") {
        let name = text(row, 0);
        let length = number(row, 1).unwrap_or(0);
        let body = text(row, 2);
        let mut detail = serde_json::json!({
            "text": r(&body),
            "columns": columns.remove(&name).unwrap_or_default(),
        });
        // The LONG is read in full; a shorter read means the driver cut it, and a fragment must not
        // be parsed as the definition.
        if length > body.chars().count() as i64 {
            detail["text_truncated"] = true.into();
            detail["text_length"] = length.into();
        }
        if let Some(c) = comments.get(&name) {
            detail["comment"] = c.as_str().into();
        }
        snapshot.objects.push(CatalogObject {
            kind: ObjectKind::View,
            owner: owner.to_string(),
            status: status_of("VIEW", &name),
            name,
            detail,
        });
    }

    for row in get("mviews") {
        let name = text(row, 0);
        snapshot.objects.push(CatalogObject {
            kind: ObjectKind::MaterializedView,
            owner: owner.to_string(),
            status: status_of("MATERIALIZED VIEW", &name),
            detail: serde_json::json!({ "refresh_mode": text(row, 1) }),
            name,
        });
    }

    for row in get("sequences") {
        let name = text(row, 0);
        snapshot.objects.push(CatalogObject {
            kind: ObjectKind::Sequence,
            owner: owner.to_string(),
            status: status_of("SEQUENCE", &name),
            detail: serde_json::json!({
                "increment_by": number(row, 1),
                "cache_size": number(row, 2),
            }),
            name,
        });
    }

    add_indexes(
        &mut snapshot,
        get("indexes"),
        get("index_columns"),
        &status_of,
    );
    add_constraints(
        &mut snapshot,
        get("constraints"),
        get("constraint_columns"),
        &r,
    );
    add_source(
        &mut snapshot,
        get("source"),
        get("triggers"),
        &status_of,
        &r,
    );

    for row in get("synonyms") {
        let name = text(row, 0);
        let mut detail = serde_json::json!({
            "target_owner": cell(row, 1),
            "target": text(row, 2),
        });
        if let Some(link) = cell(row, 3) {
            detail["db_link"] = link.into();
        }
        snapshot.objects.push(CatalogObject {
            kind: ObjectKind::Synonym,
            owner: owner.to_string(),
            status: status_of("SYNONYM", &name),
            detail,
            name,
        });
    }

    reconcile(&snapshot)?;
    ekos_common::run_log::emit(
        "oracle.catalog",
        &format!(
            "reconciled against {}_OBJECTS",
            family.prefix().to_uppercase()
        ),
        &serde_json::json!({
            "owner": owner,
            "kinds": all_kinds().filter(|k| k.object_type().is_some()).count(),
            "objects": snapshot.objects.len(),
            "mismatches": 0,
        }),
    );
    Ok(snapshot)
}

/// The view a catalog query reads (`DBA_TAB_COLUMNS`), for the run log.
fn source_view(sql: &str) -> Option<String> {
    let upper = sql.to_uppercase();
    let rest = &upper[upper.find(" FROM ")? + 6..];
    let view: String = rest
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '$')
        .collect();
    (!view.is_empty()).then_some(view)
}

type StatusMap = BTreeMap<(String, String), String>;

fn census_of(
    rows: &[Vec<Option<String>>],
) -> Result<(BTreeMap<String, Census>, StatusMap), OraError> {
    let mut census: BTreeMap<String, Census> = BTreeMap::new();
    let mut status = StatusMap::new();
    for row in rows {
        let t = text(row, 0);
        let known = all_kinds().any(|k| k.object_type() == Some(t.as_str()));
        if !known && !COUNTED_ONLY.contains(&t.as_str()) {
            return Err(OraError::UnknownObjectType(t));
        }
        let s = text(row, 2);
        let entry = census.entry(t.clone()).or_default();
        if s == "VALID" {
            entry.valid += 1;
        } else {
            entry.invalid += 1;
        }
        status.insert((t, text(row, 1)), s);
    }
    Ok((census, status))
}

fn all_kinds() -> impl Iterator<Item = ObjectKind> {
    use ObjectKind::*;
    [
        Table,
        View,
        MaterializedView,
        Sequence,
        Index,
        Function,
        Procedure,
        Package,
        PackageBody,
        Trigger,
        Type,
        TypeBody,
        Synonym,
        PrimaryKey,
        ForeignKey,
        UniqueConstraint,
        CheckConstraint,
    ]
    .into_iter()
}

fn table_comments(
    rows: &[Vec<Option<String>>],
    r: &dyn Fn(&str) -> String,
) -> BTreeMap<String, String> {
    rows.iter()
        .map(|row| (text(row, 0), r(&text(row, 1))))
        .collect()
}

fn column_comments(
    rows: &[Vec<Option<String>>],
    r: &dyn Fn(&str) -> String,
) -> BTreeMap<(String, String), String> {
    rows.iter()
        .map(|row| ((text(row, 0), text(row, 1)), r(&text(row, 2))))
        .collect()
}

/// An Oracle column type as written in DDL: `VARCHAR2(60 CHAR)`, `NUMBER(10,0)`, `DATE`.
pub fn render_type(
    data_type: &str,
    length: Option<i64>,
    precision: Option<i64>,
    scale: Option<i64>,
    char_length: Option<i64>,
    char_used: Option<&str>,
) -> String {
    match data_type {
        "VARCHAR2" | "NVARCHAR2" | "CHAR" | "NCHAR" => {
            let n = char_length.filter(|n| *n > 0).or(length).unwrap_or(0);
            match (data_type, char_used) {
                ("VARCHAR2" | "CHAR", Some("C")) => format!("{data_type}({n} CHAR)"),
                _ => format!("{data_type}({n})"),
            }
        }
        "RAW" => format!("RAW({})", length.unwrap_or(0)),
        "NUMBER" => match (precision, scale) {
            (Some(p), Some(s)) => format!("NUMBER({p},{s})"),
            (None, Some(0)) => "INTEGER".to_string(),
            _ => "NUMBER".to_string(),
        },
        "FLOAT" => match precision {
            Some(p) => format!("FLOAT({p})"),
            None => "FLOAT".to_string(),
        },
        other => other.to_string(),
    }
}

/// P0 column profiles by (table, column): counts from the optimizer's statistics, never exact.
fn column_statistics(
    rows: &[Vec<Option<String>>],
) -> BTreeMap<(String, String), serde_json::Value> {
    rows.iter()
        .map(|row| {
            let mut p = serde_json::json!({ "tier": "p0", "nulls_exact": false });
            for (i, key) in [(2, "nulls"), (3, "distinct"), (4, "avg_length")] {
                if let Some(n) = number(row, i) {
                    p[key] = n.into();
                }
            }
            if let Some(t) = cell(row, 5) {
                p["analyzed"] = t.into();
            }
            ((text(row, 0), text(row, 1)), p)
        })
        .collect()
}

fn columns_by_table(
    rows: &[Vec<Option<String>>],
    comments: &BTreeMap<(String, String), String>,
    statistics: &BTreeMap<(String, String), serde_json::Value>,
    r: &dyn Fn(&str) -> String,
) -> BTreeMap<String, Vec<serde_json::Value>> {
    let mut out: BTreeMap<String, Vec<serde_json::Value>> = BTreeMap::new();
    for row in rows {
        let table = text(row, 0);
        let name = text(row, 1);
        let data_type = render_type(
            cell(row, 3).unwrap_or_default(),
            number(row, 4),
            number(row, 5),
            number(row, 6),
            number(row, 7),
            cell(row, 8),
        );
        let mut c = serde_json::json!({
            "name": name,
            "data_type": data_type,
            "nullable": cell(row, 9) != Some("N"),
        });
        if let Some(d) = cell(row, 10).map(str::trim).filter(|d| !d.is_empty()) {
            c["default"] = r(d).into();
        }
        let key = (table.clone(), name);
        if let Some(comment) = comments.get(&key) {
            c["comment"] = comment.as_str().into();
        }
        if let Some(p) = statistics.get(&key).filter(|p| p.get("nulls").is_some()) {
            c["profile"] = p.clone();
        }
        out.entry(table).or_default().push(c);
    }
    out
}

fn add_indexes(
    snapshot: &mut CatalogSnapshot,
    rows: &[Vec<Option<String>>],
    column_rows: &[Vec<Option<String>>],
    status_of: &dyn Fn(&str, &str) -> Option<String>,
) {
    let mut cols: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for row in column_rows {
        cols.entry(text(row, 0)).or_default().push(text(row, 1));
    }
    for row in rows {
        let name = text(row, 0);
        snapshot.objects.push(CatalogObject {
            kind: ObjectKind::Index,
            owner: snapshot.owner.clone(),
            status: status_of("INDEX", &name),
            detail: serde_json::json!({
                "table": text(row, 1),
                "index_type": text(row, 2),
                "unique": cell(row, 3) == Some("UNIQUE"),
                "generated": cell(row, 5) == Some("Y"),
                "columns": cols.remove(&name).unwrap_or_default(),
            }),
            name,
        });
    }
}

/// `"C" IS NOT NULL`, exactly as Oracle writes a column's `NOT NULL` into its check condition.
fn is_not_null_check(condition: &str, columns: &[String]) -> bool {
    match columns {
        [c] => condition.trim() == format!("\"{c}\" IS NOT NULL"),
        _ => false,
    }
}

fn add_constraints(
    snapshot: &mut CatalogSnapshot,
    rows: &[Vec<Option<String>>],
    column_rows: &[Vec<Option<String>>],
    r: &dyn Fn(&str) -> String,
) {
    let mut cols: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for row in column_rows {
        cols.entry(text(row, 0)).or_default().push(text(row, 1));
    }
    // A foreign key names the constraint it references; resolve that to table and columns.
    let mut keys: BTreeMap<String, (String, Vec<String>)> = BTreeMap::new();
    for row in rows {
        if matches!(cell(row, 1), Some("P" | "U")) {
            let name = text(row, 0);
            let c = cols.get(&name).cloned().unwrap_or_default();
            keys.insert(name, (text(row, 2), c));
        }
    }
    for row in rows {
        let name = text(row, 0);
        let columns = cols.get(&name).cloned().unwrap_or_default();
        let generated = cell(row, 9) == Some("GENERATED NAME");
        let kind = match cell(row, 1) {
            Some("P") => ObjectKind::PrimaryKey,
            Some("R") => ObjectKind::ForeignKey,
            Some("U") => ObjectKind::UniqueConstraint,
            Some("C") => {
                let condition = text(row, 3);
                if generated && is_not_null_check(&condition, &columns) {
                    snapshot.folded_not_null += 1;
                    continue;
                }
                ObjectKind::CheckConstraint
            }
            other => {
                *snapshot
                    .other_constraints
                    .entry(other.unwrap_or_default().to_string())
                    .or_insert(0) += 1;
                continue;
            }
        };
        let mut detail = serde_json::json!({
            "table": text(row, 2),
            "columns": columns,
            "validated": cell(row, 8) == Some("VALIDATED"),
            "generated_name": generated,
        });
        if kind == ObjectKind::CheckConstraint {
            detail["condition"] = r(&text(row, 3)).into();
        }
        if kind == ObjectKind::ForeignKey {
            let referenced = text(row, 5);
            detail["references_owner"] = cell(row, 4).into();
            detail["references_constraint"] = referenced.as_str().into();
            // Only resolvable inside this owner; a cross-owner key keeps the constraint name.
            if cell(row, 4) == Some(snapshot.owner.as_str())
                && let Some((t, c)) = keys.get(&referenced)
            {
                detail["references_table"] = t.as_str().into();
                detail["references_columns"] = serde_json::json!(c);
            }
            detail["on_delete"] = cell(row, 6).into();
        }
        snapshot.objects.push(CatalogObject {
            kind,
            owner: snapshot.owner.clone(),
            status: cell(row, 7).map(str::to_string),
            detail,
            name,
        });
    }
}

fn add_source(
    snapshot: &mut CatalogSnapshot,
    rows: &[Vec<Option<String>>],
    trigger_rows: &[Vec<Option<String>>],
    status_of: &dyn Fn(&str, &str) -> Option<String>,
    r: &dyn Fn(&str) -> String,
) {
    let mut bodies: BTreeMap<(String, String), String> = BTreeMap::new();
    for row in rows {
        let key = (text(row, 0), text(row, 1));
        bodies
            .entry(key)
            .or_default()
            .push_str(cell(row, 3).unwrap_or_default());
    }
    let mut triggers: BTreeMap<String, serde_json::Value> = BTreeMap::new();
    for row in trigger_rows {
        triggers.insert(
            text(row, 0),
            serde_json::json!({
                "trigger_type": text(row, 1),
                "event": text(row, 2),
                "table": cell(row, 3),
                "base_object_type": text(row, 4),
                "enabled": cell(row, 5) == Some("ENABLED"),
            }),
        );
    }
    for ((name, source_type), body) in bodies {
        let Some(kind) = ObjectKind::from_source_type(&source_type) else {
            // `*_SOURCE` also holds JAVA SOURCE and LIBRARY rows; they are counted, not listed.
            continue;
        };
        let mut detail = serde_json::json!({ "text": r(&body) });
        if kind == ObjectKind::Trigger
            && let Some(t) = triggers.remove(&name)
        {
            detail["binding"] = t;
        }
        snapshot.objects.push(CatalogObject {
            kind,
            owner: snapshot.owner.clone(),
            status: status_of(&source_type, &name),
            detail,
            name,
        });
    }
}

/// Compare every listed kind with the dictionary's own count. A shortfall is an error: a catalog
/// that silently lists less shrinks every coverage number computed from it.
pub fn reconcile(snapshot: &CatalogSnapshot) -> Result<(), OraError> {
    let counts = snapshot.counts();
    for kind in all_kinds() {
        let Some(t) = kind.object_type() else {
            continue;
        };
        let expected = snapshot
            .census
            .get(t)
            .map(|c| c.valid + c.invalid)
            .unwrap_or(0);
        let got = counts.get(&kind).copied().unwrap_or(0);
        if got != expected {
            return Err(OraError::CatalogIncomplete {
                kind: kind.as_str(),
                expected,
                got,
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Recorded rows, answered by query name.
    #[derive(Default)]
    struct Recorded(BTreeMap<&'static str, Rows>);

    impl Recorded {
        fn with(mut self, name: &'static str, rows: &[&[Option<&str>]]) -> Self {
            self.0.insert(
                name,
                rows.iter()
                    .map(|r| r.iter().map(|c| c.map(str::to_string)).collect())
                    .collect(),
            );
            self
        }
    }

    impl CatalogSource for Recorded {
        fn rows(&self, query: &Query, _owner: &str) -> Result<Rows, OraError> {
            Ok(self.0.get(query.name).cloned().unwrap_or_default())
        }
    }

    fn invoice() -> Recorded {
        Recorded::default()
            .with(
                "objects",
                &[
                    &[Some("TABLE"), Some("C_INVOICE"), Some("VALID")],
                    &[Some("TABLE"), Some("C_BPARTNER"), Some("VALID")],
                    &[Some("INDEX"), Some("C_INVOICE_KEY"), Some("VALID")],
                    &[Some("FUNCTION"), Some("INVOICEOPEN"), Some("INVALID")],
                    &[
                        Some("LOB"),
                        Some("SYS_LOB0000012345C00004$$"),
                        Some("VALID"),
                    ],
                ],
            )
            .with(
                "tables",
                &[
                    &[
                        Some("C_INVOICE"),
                        Some("N"),
                        None,
                        Some("NO"),
                        Some("120"),
                        None,
                    ],
                    &[Some("C_BPARTNER"), Some("N"), None, Some("NO"), None, None],
                ],
            )
            .with(
                "column_statistics",
                &[&[
                    Some("C_INVOICE"),
                    Some("C_BPARTNER_ID"),
                    Some("3"),
                    Some("40"),
                    Some("5"),
                    Some("2024-12-01T10:00:00"),
                ]],
            )
            .with(
                "columns",
                &[
                    &[
                        Some("C_INVOICE"),
                        Some("C_INVOICE_ID"),
                        Some("1"),
                        Some("NUMBER"),
                        Some("22"),
                        Some("10"),
                        Some("0"),
                        Some("0"),
                        None,
                        Some("N"),
                        None,
                    ],
                    &[
                        Some("C_INVOICE"),
                        Some("DOCUMENTNO"),
                        Some("2"),
                        Some("VARCHAR2"),
                        Some("120"),
                        None,
                        None,
                        Some("30"),
                        Some("C"),
                        Some("N"),
                        None,
                    ],
                    &[
                        Some("C_INVOICE"),
                        Some("ISACTIVE"),
                        Some("3"),
                        Some("CHAR"),
                        Some("1"),
                        None,
                        None,
                        Some("1"),
                        Some("B"),
                        Some("N"),
                        Some("'Y' "),
                    ],
                    &[
                        Some("C_INVOICE"),
                        Some("C_BPARTNER_ID"),
                        Some("4"),
                        Some("NUMBER"),
                        Some("22"),
                        Some("10"),
                        Some("0"),
                        Some("0"),
                        None,
                        Some("Y"),
                        None,
                    ],
                ],
            )
            .with(
                "constraints",
                &[
                    &[
                        Some("C_INVOICE_KEY"),
                        Some("P"),
                        Some("C_INVOICE"),
                        None,
                        None,
                        None,
                        None,
                        Some("ENABLED"),
                        Some("VALIDATED"),
                        Some("USER NAME"),
                    ],
                    &[
                        Some("C_BPARTNER_KEY"),
                        Some("P"),
                        Some("C_BPARTNER"),
                        None,
                        None,
                        None,
                        None,
                        Some("ENABLED"),
                        Some("VALIDATED"),
                        Some("USER NAME"),
                    ],
                    &[
                        Some("CBPARTNER_CINVOICE"),
                        Some("R"),
                        Some("C_INVOICE"),
                        None,
                        Some("ADEMPIERE"),
                        Some("C_BPARTNER_KEY"),
                        Some("NO ACTION"),
                        Some("ENABLED"),
                        Some("NOT VALIDATED"),
                        Some("USER NAME"),
                    ],
                    &[
                        Some("SYS_C0012345"),
                        Some("C"),
                        Some("C_INVOICE"),
                        Some("\"DOCUMENTNO\" IS NOT NULL"),
                        None,
                        None,
                        None,
                        Some("ENABLED"),
                        Some("VALIDATED"),
                        Some("GENERATED NAME"),
                    ],
                    &[
                        Some("C_INVOICE_ISACTIVE_CHECK"),
                        Some("C"),
                        Some("C_INVOICE"),
                        Some("IsActive IN ('Y','N')"),
                        None,
                        None,
                        None,
                        Some("ENABLED"),
                        Some("VALIDATED"),
                        Some("USER NAME"),
                    ],
                ],
            )
            .with(
                "constraint_columns",
                &[
                    &[Some("C_INVOICE_KEY"), Some("C_INVOICE_ID"), Some("1")],
                    &[Some("C_BPARTNER_KEY"), Some("C_BPARTNER_ID"), Some("1")],
                    &[Some("CBPARTNER_CINVOICE"), Some("C_BPARTNER_ID"), Some("1")],
                    &[Some("SYS_C0012345"), Some("DOCUMENTNO"), Some("1")],
                    &[
                        Some("C_INVOICE_ISACTIVE_CHECK"),
                        Some("ISACTIVE"),
                        Some("1"),
                    ],
                ],
            )
            .with(
                "indexes",
                &[&[
                    Some("C_INVOICE_KEY"),
                    Some("C_INVOICE"),
                    Some("NORMAL"),
                    Some("UNIQUE"),
                    Some("VALID"),
                    Some("N"),
                ]],
            )
            .with(
                "index_columns",
                &[&[Some("C_INVOICE_KEY"), Some("C_INVOICE_ID"), Some("1")]],
            )
            .with(
                "source",
                &[
                    &[
                        Some("INVOICEOPEN"),
                        Some("FUNCTION"),
                        Some("1"),
                        Some("FUNCTION invoiceOpen (p_id IN NUMBER)\n"),
                    ],
                    &[
                        Some("INVOICEOPEN"),
                        Some("FUNCTION"),
                        Some("2"),
                        Some("RETURN NUMBER IS BEGIN RETURN 0; END;\n"),
                    ],
                ],
            )
            .with(
                "column_comments",
                &[&[
                    Some("C_INVOICE"),
                    Some("DOCUMENTNO"),
                    Some("Document sequence number"),
                ]],
            )
    }

    fn snapshot() -> CatalogSnapshot {
        introspect(
            &invoice(),
            "ADEMPIERE",
            ViewFamily::Dba,
            &RedactionConfig::default(),
        )
        .unwrap()
    }

    #[test]
    fn tables_carry_their_columns_in_ddl_form() {
        let s = snapshot();
        let t = s
            .of_kind(ObjectKind::Table)
            .find(|t| t.name == "C_INVOICE")
            .unwrap();
        let cols = t.detail["columns"].as_array().unwrap();
        let types: Vec<&str> = cols
            .iter()
            .map(|c| c["data_type"].as_str().unwrap())
            .collect();
        assert_eq!(
            types,
            [
                "NUMBER(10,0)",
                "VARCHAR2(30 CHAR)",
                "CHAR(1)",
                "NUMBER(10,0)"
            ]
        );
        assert_eq!(cols[2]["default"], "'Y'");
        assert_eq!(cols[1]["comment"], "Document sequence number");
        assert_eq!(cols[3]["nullable"], true);
        assert_eq!(t.detail["num_rows"], 120);
        assert_eq!(s.columns, 4);
        // P0: statistics, marked inexact; a column without statistics has no profile at all.
        assert_eq!(t.detail["profile"]["row_count_exact"], false);
        assert_eq!(cols[3]["profile"]["nulls"], 3);
        assert_eq!(cols[3]["profile"]["nulls_exact"], false);
        assert_eq!(cols[3]["profile"]["distinct"], 40);
        assert!(cols[0].get("profile").is_none());
    }

    #[test]
    fn a_system_named_not_null_check_is_folded_and_a_written_check_is_kept() {
        let s = snapshot();
        assert_eq!(s.folded_not_null, 1);
        let checks: Vec<&str> = s
            .of_kind(ObjectKind::CheckConstraint)
            .map(|c| c.name.as_str())
            .collect();
        assert_eq!(checks, ["C_INVOICE_ISACTIVE_CHECK"]);
    }

    #[test]
    fn a_foreign_key_resolves_the_referenced_table_and_columns() {
        let s = snapshot();
        let fk = s.of_kind(ObjectKind::ForeignKey).next().unwrap();
        assert_eq!(fk.detail["references_table"], "C_BPARTNER");
        assert_eq!(
            fk.detail["references_columns"],
            serde_json::json!(["C_BPARTNER_ID"])
        );
        assert_eq!(fk.detail["validated"], false);
    }

    #[test]
    fn source_lines_join_into_one_routine_and_an_invalid_status_is_kept() {
        let s = snapshot();
        let f = s.of_kind(ObjectKind::Function).next().unwrap();
        assert!(
            f.detail["text"]
                .as_str()
                .unwrap()
                .starts_with("FUNCTION invoiceOpen")
        );
        assert!(f.detail["text"].as_str().unwrap().contains("RETURN 0;"));
        assert_eq!(f.status.as_deref(), Some("INVALID"));
        assert_eq!(s.census["FUNCTION"].invalid, 1);
        assert_eq!(
            s.census["LOB"].valid, 1,
            "counted-only types are in the census"
        );
    }

    #[test]
    fn an_object_type_nobody_listed_is_an_error_not_a_skipped_row() {
        let src = invoice().with("objects", &[&[Some("WIDGET"), Some("X"), Some("VALID")]]);
        let err = introspect(
            &src,
            "ADEMPIERE",
            ViewFamily::Dba,
            &RedactionConfig::default(),
        )
        .unwrap_err();
        assert!(matches!(err, OraError::UnknownObjectType(t) if t == "WIDGET"));
    }

    #[test]
    fn the_run_log_names_the_view_each_query_reads() {
        let views: Vec<_> = queries(ViewFamily::Dba)
            .iter()
            .map(|q| source_view(&q.sql))
            .collect();
        assert!(views.iter().all(Option::is_some), "{views:?}");
        assert!(views.contains(&Some("DBA_TAB_COLUMNS".into())), "{views:?}");
        assert_eq!(source_view("SELECT 1"), None);
    }

    #[test]
    fn a_routine_missing_from_the_source_view_fails_reconciliation() {
        // What an `ALL_*` read by a user without EXECUTE looks like: the object is listed, its
        // source is not.
        let src = invoice().with("source", &[]);
        let err = introspect(
            &src,
            "ADEMPIERE",
            ViewFamily::All,
            &RedactionConfig::default(),
        )
        .unwrap_err();
        assert!(matches!(
            err,
            OraError::CatalogIncomplete {
                kind: "function",
                expected: 1,
                got: 0
            }
        ));
    }

    #[test]
    fn view_text_longer_than_text_vc_is_marked_truncated() {
        let src = invoice()
            .with(
                "objects",
                &[
                    &[Some("TABLE"), Some("C_INVOICE"), Some("VALID")],
                    &[Some("TABLE"), Some("C_BPARTNER"), Some("VALID")],
                    &[Some("INDEX"), Some("C_INVOICE_KEY"), Some("VALID")],
                    &[Some("FUNCTION"), Some("INVOICEOPEN"), Some("VALID")],
                    &[Some("VIEW"), Some("RV_X"), Some("VALID")],
                ],
            )
            .with(
                "views",
                &[&[Some("RV_X"), Some("5000"), Some("SELECT 1 FROM dual")]],
            );
        let s = introspect(
            &src,
            "ADEMPIERE",
            ViewFamily::Dba,
            &RedactionConfig::default(),
        )
        .unwrap();
        let v = s.of_kind(ObjectKind::View).next().unwrap();
        assert_eq!(v.detail["text_truncated"], true);
        assert_eq!(v.detail["text_length"], 5000);
    }

    #[test]
    fn secrets_in_routine_text_are_redacted_before_they_leave_the_crate() {
        let src = invoice().with(
            "source",
            &[&[
                Some("INVOICEOPEN"),
                Some("FUNCTION"),
                Some("1"),
                Some("-- password = 'hunter2hunter2'\nFUNCTION f RETURN NUMBER IS BEGIN RETURN 0; END;"),
            ]],
        );
        let s = introspect(
            &src,
            "ADEMPIERE",
            ViewFamily::Dba,
            &RedactionConfig::default(),
        )
        .unwrap();
        let f = s.of_kind(ObjectKind::Function).next().unwrap();
        assert!(
            !f.detail["text"]
                .as_str()
                .unwrap()
                .contains("hunter2hunter2")
        );
    }

    #[test]
    fn types_render_as_ddl() {
        assert_eq!(
            render_type("NUMBER", Some(22), None, None, None, None),
            "NUMBER"
        );
        assert_eq!(
            render_type("NUMBER", Some(22), None, Some(0), None, None),
            "INTEGER"
        );
        assert_eq!(
            render_type("VARCHAR2", Some(60), None, None, Some(60), Some("B")),
            "VARCHAR2(60)"
        );
        assert_eq!(
            render_type("NVARCHAR2", Some(120), None, None, Some(60), Some("C")),
            "NVARCHAR2(60)"
        );
        assert_eq!(
            render_type("TIMESTAMP(6)", Some(11), None, Some(6), None, None),
            "TIMESTAMP(6)"
        );
    }
}
