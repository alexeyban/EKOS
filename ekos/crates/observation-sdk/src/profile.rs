//! RFC 0211 §3.4 / RFC 0252 §3.1 — one profile shape for every live database.
//!
//! A `[[databases]]` profile measures a table in three tiers, whatever the engine:
//!
//! | Tier | Measures | Cost |
//! |---|---|---|
//! | P0 | the optimizer's statistics, read with the catalog | none |
//! | P1 | an exact row count per table | one scan per table |
//! | P2 | one aggregate scan per table: exact null counts, number and date bounds, longest text | one scan per table |
//!
//! This module is pure: it **plans** the aggregate queries for one table in the engine's dialect
//! ([`plan_table`]) and **applies** their answers to the table's catalog detail ([`TablePlan::apply`]).
//! Each live crate only runs the planned text through its own read-only session, so the rules that
//! keep row values out of the ledger live here, once, and are tested on the SQL itself:
//!
//! - a profiler never runs free SQL: every query is built from a closed set of aggregates
//!   ([`Aggregate`]) over quoted identifiers;
//! - a text column is never the argument of `MIN` or `MAX` — those *are* values. It contributes a
//!   count and a length;
//! - a number or date column whose name classifies it as personal data ([`ekos_common::pii`]) gets
//!   a count and nothing else;
//! - bounds that are recorded still pass redaction (a number can match a secret pattern).

use ekos_common::pii;
use ekos_common::redaction::{RedactionConfig, redact};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// How much work a profile may do. Ordered: a higher tier includes everything below it.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ProfileTier {
    /// Optimizer statistics only. No scan of user data.
    #[default]
    P0,
    /// An exact row count per table.
    P1,
    /// One aggregate scan per table.
    P2,
}

impl ProfileTier {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::P0 => "p0",
            Self::P1 => "p1",
            Self::P2 => "p2",
        }
    }
}

/// The SQL dialect a plan is rendered in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Flavor {
    Oracle,
    Postgres,
    ClickHouse,
    /// RFC 0252 §3.5: T-SQL.
    SqlServer,
    /// RFC 0252 §3.5: Informix SQL. Identifiers are quoted with `"…"`, which the session makes
    /// delimited identifiers (`DELIMIDENT=y`; always so over DRDA).
    Informix,
    /// RFC 0252 §8: SQLite, for benchmark databases (BIRD) read in process. The owner is the
    /// attached schema (`main`). SQLite has no MD5: a caller that computes overlap signatures
    /// registers an `md5(text)` function returning lowercase hex on its connection.
    Sqlite,
    /// RFC 0259 D5: DuckDB, a mart file read in process through a read-only, locked session. The
    /// owner is a schema of the file (`main`, `gold`); `md5` is built in.
    DuckDb,
}

/// What a profile run did, for the summary line and the catalog object.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileStats {
    pub tier: ProfileTier,
    pub tables_profiled: usize,
    /// Tables whose scan failed (timeout, privilege): recorded on the table, not fatal.
    pub tables_failed: usize,
    /// Tables not scanned because the session was lost after an earlier failure.
    pub tables_skipped: usize,
    /// Columns with an exact null count.
    pub columns_measured: usize,
    /// Columns whose bounds were withheld because their name classifies them as personal data.
    pub values_suppressed: usize,
}

impl ProfileStats {
    pub fn new(tier: ProfileTier) -> Self {
        Self {
            tier,
            ..Default::default()
        }
    }

    /// Fold one table's outcome in.
    pub fn record(&mut self, outcome: &TableOutcome) {
        match outcome {
            TableOutcome::Profiled {
                measured,
                suppressed,
            } => {
                self.tables_profiled += 1;
                self.columns_measured += measured;
                self.values_suppressed += suppressed;
            }
            TableOutcome::Failed => self.tables_failed += 1,
            TableOutcome::Skipped => self.tables_skipped += 1,
        }
    }
}

/// How one table's profile ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableOutcome {
    Profiled { measured: usize, suppressed: usize },
    Failed,
    Skipped,
}

/// A column's type, as far as profiling cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    Number,
    Date,
    Text,
    /// Counted (nulls) only: LOBs, binary, JSON, arrays, booleans, objects.
    Other,
    /// Not even countable (Oracle `LONG` / `LONG RAW`).
    Uncountable,
}

/// The [`Family`] of a catalog `data_type`, in the engine's own spelling.
pub fn family(flavor: Flavor, data_type: &str) -> Family {
    match flavor {
        Flavor::Oracle => oracle_family(data_type),
        Flavor::Postgres => postgres_family(data_type),
        Flavor::ClickHouse => clickhouse_family(data_type),
        Flavor::SqlServer => sqlserver_family(data_type),
        Flavor::Informix => informix_family(data_type),
        Flavor::Sqlite => sqlite_family(data_type),
        Flavor::DuckDb => duckdb_family(data_type),
    }
}

/// DuckDB's `data_type` as `duckdb_columns()` spells it (`DECIMAL(18,2)`, `TIMESTAMP WITH TIME
/// ZONE`, `VARCHAR`, `INTEGER[]`, `STRUCT(…)`).
fn duckdb_family(data_type: &str) -> Family {
    if data_type.trim_end().ends_with(']') {
        return Family::Other;
    }
    let base = base_type(data_type);
    match base.as_str() {
        "tinyint" | "smallint" | "integer" | "bigint" | "hugeint" | "utinyint" | "usmallint"
        | "uinteger" | "ubigint" | "uhugeint" | "decimal" | "numeric" | "float" | "double"
        | "real" => Family::Number,
        "date" => Family::Date,
        "varchar" | "text" | "string" | "uuid" => Family::Text,
        _ if base.starts_with("timestamp") || base.starts_with("time") => Family::Date,
        _ => Family::Other,
    }
}

fn base_type(data_type: &str) -> String {
    data_type
        .split('(')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
}

fn oracle_family(data_type: &str) -> Family {
    let base = base_type(data_type);
    match base.as_str() {
        "long" | "long raw" => Family::Uncountable,
        "number" | "float" | "binary_float" | "binary_double" | "integer" => Family::Number,
        "date" => Family::Date,
        "varchar2" | "nvarchar2" | "char" | "nchar" => Family::Text,
        _ if base.starts_with("timestamp") => Family::Date,
        _ => Family::Other,
    }
}

fn postgres_family(data_type: &str) -> Family {
    if data_type.trim_end().ends_with("[]") {
        return Family::Other;
    }
    let base = base_type(data_type);
    match base.as_str() {
        "smallint" | "integer" | "bigint" | "int" | "int2" | "int4" | "int8" | "numeric"
        | "decimal" | "real" | "double precision" | "float4" | "float8" | "money" => Family::Number,
        "date" => Family::Date,
        "character varying" | "varchar" | "character" | "char" | "bpchar" | "text" | "citext"
        | "name" | "uuid" => Family::Text,
        _ if base.starts_with("timestamp") || base.starts_with("time") => Family::Date,
        _ => Family::Other,
    }
}

/// `Nullable(LowCardinality(String))` → `String`.
fn clickhouse_inner(data_type: &str) -> &str {
    let mut t = data_type.trim();
    for wrapper in ["Nullable(", "LowCardinality("] {
        while let Some(inner) = t.strip_prefix(wrapper).and_then(|s| s.strip_suffix(')')) {
            t = inner.trim();
        }
    }
    t
}

fn clickhouse_family(data_type: &str) -> Family {
    let t = clickhouse_inner(data_type);
    let base = t.split('(').next().unwrap_or_default();
    match base {
        b if b.starts_with("Int") || b.starts_with("UInt") || b.starts_with("Float") => {
            Family::Number
        }
        b if b.starts_with("Decimal") => Family::Number,
        "Date" | "Date32" | "DateTime" | "DateTime64" => Family::Date,
        "String" | "FixedString" | "UUID" | "Enum8" | "Enum16" => Family::Text,
        _ => Family::Other,
    }
}

/// SQL Server type names as `sys.types` spells them. `text`, `ntext` and `image` are counted
/// only: `LEN` refuses them and `MIN`/`MAX` cannot compare them. `uniqueidentifier`, `bit`,
/// binary, `xml`, `sql_variant`, spatial and `hierarchyid` are counted only, as elsewhere.
fn sqlserver_family(data_type: &str) -> Family {
    let base = base_type(data_type);
    match base.as_str() {
        "tinyint" | "smallint" | "int" | "bigint" | "decimal" | "numeric" | "float" | "real"
        | "money" | "smallmoney" => Family::Number,
        "date" | "datetime" | "datetime2" | "smalldatetime" | "datetimeoffset" | "time" => {
            Family::Date
        }
        "char" | "varchar" | "nchar" | "nvarchar" => Family::Text,
        _ => Family::Other,
    }
}

/// SQLite's declared types, read by its type-affinity rules (a declared type is free text):
/// `INT` anywhere → whole number; `CHAR`, `CLOB`, `TEXT` → text; `REAL`, `FLOA`, `DOUB`,
/// `NUMERIC`, `DECIMAL` → number; `DATE`, `TIME` → date (stored as text); the rest counted only.
fn sqlite_family(data_type: &str) -> Family {
    let t = data_type.trim().to_ascii_lowercase();
    if t.contains("int") {
        Family::Number
    } else if t.contains("char") || t.contains("clob") || t.contains("text") {
        Family::Text
    } else if ["real", "floa", "doub", "numeric", "decimal"]
        .iter()
        .any(|k| t.contains(k))
    {
        Family::Number
    } else if t.contains("date") || t.contains("time") {
        Family::Date
    } else {
        Family::Other
    }
}

/// Informix type names as the plugin decodes `syscolumns.coltype` (`decimal(12,2)`,
/// `datetime year to second`, `lvarchar(2048)`). Only a `datetime` that starts at `year` is a date:
/// `datetime hour to minute` has no day to compare. `text`, `byte`, `blob`, `clob`, `boolean`,
/// `interval`, collections and opaque types are counted only.
fn informix_family(data_type: &str) -> Family {
    let t = data_type.trim().to_ascii_lowercase();
    let base = base_type(&t);
    match base.as_str() {
        "smallint" | "integer" | "int" | "int8" | "bigint" | "serial" | "serial8" | "bigserial"
        | "decimal" | "dec" | "numeric" | "money" | "float" | "smallfloat" | "real"
        | "double precision" => Family::Number,
        "date" => Family::Date,
        _ if t.starts_with("datetime year") => Family::Date,
        "char" | "character" | "varchar" | "nchar" | "nvarchar" | "lvarchar" => Family::Text,
        _ => Family::Other,
    }
}

/// Oracle refuses more expressions than this in one select list, PostgreSQL more than 1,664
/// target entries; one bound for every engine, leaving room for the row count.
const EXPRESSIONS_PER_QUERY: usize = 900;

/// A quoted identifier in `flavor`, or `Err` for a name that cannot be quoted safely.
///
/// Oracle names are checked, not escaped: a quoted Oracle identifier cannot contain `"` at all, and
/// the catalog read already refuses anything outside `[A-Za-z0-9_$#]` (`ora-live`'s guard).
pub fn ident(flavor: Flavor, name: &str) -> Result<String, String> {
    if name.is_empty() || name.len() > 128 || name.contains('\0') {
        return Err(format!("unsafe identifier {name:?}"));
    }
    match flavor {
        Flavor::Oracle => {
            let ok = name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'$' | b'#'));
            if ok {
                Ok(format!("\"{name}\""))
            } else {
                Err(format!("unsafe identifier {name:?}"))
            }
        }
        Flavor::Postgres => Ok(format!("\"{}\"", name.replace('"', "\"\""))),
        Flavor::ClickHouse => Ok(format!(
            "`{}`",
            name.replace('\\', "\\\\").replace('`', "\\`")
        )),
        // `QUOTENAME`'s rule: a `]` inside the name is doubled.
        Flavor::SqlServer => Ok(format!("[{}]", name.replace(']', "]]"))),
        // A delimited identifier (the session sets `DELIMIDENT`); `"` inside is doubled.
        Flavor::Informix | Flavor::Sqlite | Flavor::DuckDb => {
            Ok(format!("\"{}\"", name.replace('"', "\"\"")))
        }
    }
}

/// `owner.table`, both quoted.
pub fn table_ref(flavor: Flavor, owner: &str, table: &str) -> Result<String, String> {
    Ok(format!(
        "{}.{}",
        ident(flavor, owner)?,
        ident(flavor, table)?
    ))
}

/// The closed set of aggregates a build-time profile computes over one column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Aggregate {
    NonNull,
    Min,
    Max,
    MaxLength,
}

impl Aggregate {
    fn key(self) -> &'static str {
        match self {
            Self::NonNull => "non_null",
            Self::Min => "min",
            Self::Max => "max",
            Self::MaxLength => "max_length",
        }
    }

    fn render(self, flavor: Flavor, col: &str, fam: Family) -> String {
        match (flavor, self) {
            // `COUNT(lob)` is ORA-00932: what is not plainly countable is counted through
            // `IS NOT NULL`, which every type accepts.
            (Flavor::Oracle, Self::NonNull) if fam == Family::Other => {
                format!("COUNT(CASE WHEN {col} IS NOT NULL THEN 1 END)")
            }
            (Flavor::Oracle, Self::NonNull) => format!("COUNT({col})"),
            (Flavor::Oracle, Self::Min) => format!("MIN({col})"),
            (Flavor::Oracle, Self::Max) => format!("MAX({col})"),
            (Flavor::Oracle, Self::MaxLength) => format!("MAX(LENGTH({col}))"),
            (Flavor::Postgres, Self::NonNull) => format!("count({col})"),
            (Flavor::Postgres, Self::Min) => format!("min({col})::text"),
            (Flavor::Postgres, Self::Max) => format!("max({col})::text"),
            (Flavor::Postgres, Self::MaxLength) => format!("max(length({col}::text))"),
            (Flavor::ClickHouse, Self::NonNull) => format!("count({col})"),
            (Flavor::ClickHouse, Self::Min) => format!("toString(min({col}))"),
            (Flavor::ClickHouse, Self::Max) => format!("toString(max({col}))"),
            (Flavor::ClickHouse, Self::MaxLength) => format!("max(length(toString({col})))"),
            // `COUNT` overflows `int` past 2^31 rows; `COUNT_BIG` is `bigint`. What is not plainly
            // countable is counted through `IS NOT NULL`, as for Oracle.
            (Flavor::SqlServer, Self::NonNull) if fam == Family::Other => {
                format!("COUNT_BIG(CASE WHEN {col} IS NOT NULL THEN 1 END)")
            }
            (Flavor::SqlServer, Self::NonNull) => format!("COUNT_BIG({col})"),
            // Style 126 is ISO 8601 for dates and full precision for `float`; ignored for exact
            // numbers.
            (Flavor::SqlServer, Self::Min) => format!("CONVERT(nvarchar(100), MIN({col}), 126)"),
            (Flavor::SqlServer, Self::Max) => format!("CONVERT(nvarchar(100), MAX({col}), 126)"),
            // `LEN` counts characters (trailing spaces excluded), `DATALENGTH` bytes; characters
            // are what a target type needs.
            (Flavor::SqlServer, Self::MaxLength) => format!("MAX(LEN({col}))"),
            // `COUNT(text_or_byte)` is refused; `IS NOT NULL` is accepted by every type.
            (Flavor::Informix, Self::NonNull) if fam == Family::Other => {
                format!("COUNT(CASE WHEN {col} IS NOT NULL THEN 1 END)")
            }
            (Flavor::Informix, Self::NonNull) => format!("COUNT({col})"),
            // A `DATE` renders in the client's `DBDATE` order (`01/31/2024` by default); `EXTEND`
            // makes it a `DATETIME YEAR TO FRACTION`, which renders as ISO 8601 whatever the
            // client's settings.
            (Flavor::Informix, a @ (Self::Min | Self::Max)) if fam == Family::Date => {
                let f = if a == Self::Min { "MIN" } else { "MAX" };
                format!("CAST(EXTEND({f}({col}), YEAR TO FRACTION(5)) AS LVARCHAR(100))")
            }
            (Flavor::Informix, Self::Min) => format!("CAST(MIN({col}) AS LVARCHAR(100))"),
            (Flavor::Informix, Self::Max) => format!("CAST(MAX({col}) AS LVARCHAR(100))"),
            // `LENGTH` ignores a `CHAR`'s trailing blanks.
            (Flavor::Informix, Self::MaxLength) => format!("MAX(LENGTH({col}))"),
            (Flavor::Sqlite, Self::NonNull) => format!("COUNT({col})"),
            (Flavor::Sqlite, Self::Min) => format!("CAST(MIN({col}) AS TEXT)"),
            (Flavor::Sqlite, Self::Max) => format!("CAST(MAX({col}) AS TEXT)"),
            (Flavor::Sqlite, Self::MaxLength) => format!("MAX(LENGTH({col}))"),
            (Flavor::DuckDb, Self::NonNull) => format!("count({col})"),
            (Flavor::DuckDb, Self::Min) => format!("CAST(min({col}) AS VARCHAR)"),
            (Flavor::DuckDb, Self::Max) => format!("CAST(max({col}) AS VARCHAR)"),
            (Flavor::DuckDb, Self::MaxLength) => format!("max(length(CAST({col} AS VARCHAR)))"),
        }
    }
}

fn row_count(flavor: Flavor) -> &'static str {
    match flavor {
        Flavor::Oracle => "COUNT(*)",
        Flavor::Postgres => "count(*)",
        Flavor::ClickHouse => "count()",
        Flavor::SqlServer => "COUNT_BIG(*)",
        Flavor::Informix | Flavor::Sqlite | Flavor::DuckDb => "COUNT(*)",
    }
}

/// One answer slot of a planned query.
#[derive(Debug, Clone)]
struct Slot {
    column: usize,
    aggregate: Aggregate,
}

/// The aggregates for one table's columns: (planned, columns whose bounds were withheld). Pure.
fn measures(flavor: Flavor, columns: &[Value]) -> (Vec<(Slot, String)>, Vec<usize>) {
    let mut out = Vec::new();
    let mut suppressed = Vec::new();
    for (i, c) in columns.iter().enumerate() {
        let Some(name) = c["name"].as_str() else {
            continue;
        };
        let Ok(col) = ident(flavor, name) else {
            continue;
        };
        let fam = family(flavor, c["data_type"].as_str().unwrap_or_default());
        if fam == Family::Uncountable {
            continue;
        }
        let mut add = |aggregate: Aggregate| {
            out.push((
                Slot {
                    column: i,
                    aggregate,
                },
                aggregate.render(flavor, &col, fam),
            ))
        };
        add(Aggregate::NonNull);
        match fam {
            Family::Number | Family::Date => {
                if pii::classify_name(name).is_some() {
                    suppressed.push(i);
                } else {
                    add(Aggregate::Min);
                    add(Aggregate::Max);
                }
            }
            Family::Text => add(Aggregate::MaxLength),
            Family::Other | Family::Uncountable => {}
        }
    }
    (out, suppressed)
}

/// The queries one table's profile runs, and how their answers map back.
#[derive(Debug, Clone)]
pub struct TablePlan {
    tier: ProfileTier,
    /// Every query is one row: the row count, then one value per slot.
    pub queries: Vec<String>,
    slots: Vec<Vec<Slot>>,
    suppressed: Vec<usize>,
}

/// Plan the profile of `owner.table` at `tier`. `columns` are the catalog's column objects
/// (`name`, `data_type`). P0 plans nothing: its statistics come with the catalog.
pub fn plan_table(
    flavor: Flavor,
    owner: &str,
    table: &str,
    columns: &[Value],
    tier: ProfileTier,
) -> Result<TablePlan, String> {
    let from = table_ref(flavor, owner, table)?;
    let count = row_count(flavor);
    let mut plan = TablePlan {
        tier,
        queries: Vec::new(),
        slots: Vec::new(),
        suppressed: Vec::new(),
    };
    match tier {
        ProfileTier::P0 => {}
        ProfileTier::P1 => {
            plan.queries.push(format!("SELECT {count} FROM {from}"));
            plan.slots.push(Vec::new());
        }
        ProfileTier::P2 => {
            let (measures, suppressed) = measures(flavor, columns);
            plan.suppressed = suppressed;
            if measures.is_empty() {
                plan.queries.push(format!("SELECT {count} FROM {from}"));
                plan.slots.push(Vec::new());
            }
            for chunk in measures.chunks(EXPRESSIONS_PER_QUERY) {
                let list: Vec<&str> = std::iter::once(count)
                    .chain(chunk.iter().map(|(_, sql)| sql.as_str()))
                    .collect();
                plan.queries
                    .push(format!("SELECT {} FROM {from}", list.join(", ")));
                plan.slots
                    .push(chunk.iter().map(|(slot, _)| slot.clone()).collect());
            }
        }
    }
    Ok(plan)
}

fn number(v: Option<&String>) -> Option<i64> {
    let v = v?.trim();
    // ClickHouse and some drivers render integral aggregates as `12` or `12.0`.
    v.parse::<i64>()
        .ok()
        .or_else(|| v.parse::<f64>().ok().map(|f| f as i64))
}

fn profile_of(column: &mut Value) -> &mut Value {
    if !column["profile"].is_object() {
        column["profile"] = json!({});
    }
    &mut column["profile"]
}

impl TablePlan {
    /// Write the answers — one row per query, in order, `None` for SQL NULL — into the table's
    /// catalog `detail` (`detail.profile` and each column's `profile`). Returns how it went.
    pub fn apply(
        &self,
        answers: &[Vec<Option<String>>],
        detail: &mut Value,
        redaction: &RedactionConfig,
    ) -> Result<TableOutcome, String> {
        if answers.len() != self.queries.len() {
            return Err(format!(
                "{} queries planned, {} answered",
                self.queries.len(),
                answers.len()
            ));
        }
        let mut rows = None;
        for row in answers {
            rows = number(row.first().and_then(Option::as_ref));
        }
        let Some(rows) = rows else {
            return Err("the row count came back empty".into());
        };
        detail["profile"] = json!({
            "tier": self.tier.as_str(),
            "row_count": rows,
            "row_count_exact": true,
        });
        let mut measured = 0;
        for (slots, row) in self.slots.iter().zip(answers) {
            for (slot, v) in slots.iter().zip(row.iter().skip(1)) {
                let Some(column) = detail["columns"].get_mut(slot.column) else {
                    continue;
                };
                let p = profile_of(column);
                p["tier"] = self.tier.as_str().into();
                match slot.aggregate {
                    Aggregate::NonNull => {
                        if let Some(n) = number(v.as_ref()) {
                            p["nulls"] = (rows - n).into();
                            p["nulls_exact"] = true.into();
                            measured += 1;
                        }
                    }
                    Aggregate::MaxLength => {
                        if let Some(n) = number(v.as_ref()) {
                            p["max_length"] = n.into();
                        }
                    }
                    a @ (Aggregate::Min | Aggregate::Max) => {
                        if let Some(v) = v {
                            p[a.key()] = redact(v, redaction).into();
                        }
                    }
                }
            }
        }
        for &i in &self.suppressed {
            let Some(column) = detail["columns"].get_mut(i) else {
                continue;
            };
            let class = column["name"].as_str().and_then(pii::classify_name);
            let p = profile_of(column);
            p["values_suppressed"] = true.into();
            if let Some(c) = class {
                p["pii"] = serde_json::to_value(c).unwrap_or(Value::Null);
            }
        }
        Ok(TableOutcome::Profiled {
            measured,
            suppressed: self.suppressed.len(),
        })
    }
}

/// Record a failed table scan on its detail, redacted (an error can quote a value).
pub fn record_failure(detail: &mut Value, error: &str, redaction: &RedactionConfig) {
    detail["profile"]["error"] = redact(error, redaction).into();
}

/// Record a table not scanned because the session was lost earlier.
pub fn record_skipped(detail: &mut Value) {
    detail["profile"]["skipped"] = "session lost after an earlier failure".into();
}

/// Runs one aggregate query and returns its single row, `None` for SQL NULL.
pub type RunAggregate<'a> = dyn FnMut(&str) -> Result<Vec<Option<String>>, String> + 'a;

/// Profile one table synchronously through `run` (one row of one aggregate query, `None` for SQL
/// NULL), writing into `detail`. A failure is recorded on the table and returned as `Err` with the
/// message, so the caller can decide whether its session survived.
pub fn profile_table(
    flavor: Flavor,
    owner: &str,
    table: &str,
    detail: &mut Value,
    tier: ProfileTier,
    redaction: &RedactionConfig,
    run: &mut RunAggregate<'_>,
) -> Result<TableOutcome, String> {
    let columns = detail["columns"].as_array().cloned().unwrap_or_default();
    let result = plan_table(flavor, owner, table, &columns, tier).and_then(|plan| {
        let answers = plan
            .queries
            .iter()
            .map(|q| run(q))
            .collect::<Result<Vec<_>, _>>()?;
        plan.apply(&answers, detail, redaction)
    });
    if let Err(e) = &result {
        record_failure(detail, e, redaction);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn invoice() -> Vec<Value> {
        serde_json::from_value(json!([
            { "name": "C_INVOICE_ID", "data_type": "NUMBER(10,0)" },
            { "name": "DOCUMENTNO", "data_type": "VARCHAR2(30 CHAR)" },
            { "name": "DATEINVOICED", "data_type": "DATE" },
            { "name": "EMAIL", "data_type": "VARCHAR2(60)" },
            { "name": "PHONE_NUMBER", "data_type": "NUMBER" },
            { "name": "NOTES", "data_type": "LONG" },
            { "name": "ATTACHMENT", "data_type": "BLOB" },
        ]))
        .unwrap()
    }

    fn pg_invoice() -> Vec<Value> {
        serde_json::from_value(json!([
            { "name": "id", "data_type": "integer" },
            { "name": "documentno", "data_type": "character varying(30)" },
            { "name": "dateinvoiced", "data_type": "timestamp without time zone" },
            { "name": "email", "data_type": "text" },
            { "name": "phone_number", "data_type": "numeric(12,0)" },
            { "name": "tags", "data_type": "text[]" },
            { "name": "payload", "data_type": "jsonb" },
        ]))
        .unwrap()
    }

    /// RFC 0259 D5: DuckDB's own spellings (`duckdb_columns().data_type`).
    fn duck_invoice() -> Vec<Value> {
        serde_json::from_value(json!([
            { "name": "id", "data_type": "BIGINT" },
            { "name": "documentno", "data_type": "VARCHAR" },
            { "name": "dateinvoiced", "data_type": "TIMESTAMP WITH TIME ZONE" },
            { "name": "email", "data_type": "VARCHAR" },
            { "name": "phone_number", "data_type": "DECIMAL(12,0)" },
            { "name": "tags", "data_type": "VARCHAR[]" },
            { "name": "payload", "data_type": "STRUCT(a INTEGER)" },
        ]))
        .unwrap()
    }

    fn ch_invoice() -> Vec<Value> {
        serde_json::from_value(json!([
            { "name": "id", "data_type": "UInt64" },
            { "name": "documentno", "data_type": "LowCardinality(String)" },
            { "name": "dateinvoiced", "data_type": "Nullable(DateTime64(3))" },
            { "name": "email", "data_type": "Nullable(String)" },
            { "name": "phone_number", "data_type": "Nullable(Decimal(12, 0))" },
            { "name": "tags", "data_type": "Array(String)" },
        ]))
        .unwrap()
    }

    fn ms_invoice() -> Vec<Value> {
        serde_json::from_value(json!([
            { "name": "id", "data_type": "int" },
            { "name": "documentno", "data_type": "nvarchar(30)" },
            { "name": "dateinvoiced", "data_type": "datetime2(7)" },
            { "name": "email", "data_type": "varchar(max)" },
            { "name": "phone_number", "data_type": "decimal(12,0)" },
            { "name": "notes", "data_type": "ntext" },
            { "name": "row_guid", "data_type": "uniqueidentifier" },
        ]))
        .unwrap()
    }

    fn ifx_invoice() -> Vec<Value> {
        serde_json::from_value(json!([
            { "name": "id", "data_type": "serial" },
            { "name": "documentno", "data_type": "varchar(30)" },
            { "name": "dateinvoiced", "data_type": "datetime year to second" },
            { "name": "email", "data_type": "lvarchar(200)" },
            { "name": "phone_number", "data_type": "decimal(12,0)" },
            { "name": "notes", "data_type": "text" },
            { "name": "shipped", "data_type": "date" },
        ]))
        .unwrap()
    }

    fn sql(flavor: Flavor, cols: &[Value]) -> Vec<String> {
        measures(flavor, cols)
            .0
            .into_iter()
            .map(|(_, s)| s)
            .collect()
    }

    /// The rule the whole tier rests on, in every dialect: a text column is never the argument of
    /// `MIN` or `MAX`, and a number whose name says personal data gets a count and nothing else.
    #[test]
    fn no_text_or_personal_column_is_ever_an_argument_of_min_or_max() {
        for (flavor, cols, texts, personal) in [
            (
                Flavor::Oracle,
                invoice(),
                ["DOCUMENTNO", "EMAIL"],
                "PHONE_NUMBER",
            ),
            (
                Flavor::Postgres,
                pg_invoice(),
                ["documentno", "email"],
                "phone_number",
            ),
            (
                Flavor::ClickHouse,
                ch_invoice(),
                ["documentno", "email"],
                "phone_number",
            ),
            (
                Flavor::SqlServer,
                ms_invoice(),
                ["documentno", "email"],
                "phone_number",
            ),
            (
                Flavor::Informix,
                ifx_invoice(),
                ["documentno", "email"],
                "phone_number",
            ),
            (
                Flavor::DuckDb,
                duck_invoice(),
                ["documentno", "email"],
                "phone_number",
            ),
        ] {
            let sql = sql(flavor, &cols);
            for text in texts {
                let mentions: Vec<&String> = sql.iter().filter(|s| s.contains(text)).collect();
                assert!(!mentions.is_empty(), "{flavor:?} {text} is counted");
                // A count, or the length of the longest value — never a value.
                assert!(
                    mentions.iter().all(|s| {
                        let s = s.to_lowercase();
                        s.starts_with("count(")
                            || s.starts_with("count_big(")
                            || s.contains("(length(")
                            || s.contains("(len(")
                    }),
                    "{flavor:?} {text}: {mentions:?}"
                );
            }
            let p: Vec<&String> = sql.iter().filter(|s| s.contains(personal)).collect();
            assert_eq!(p.len(), 1, "{flavor:?}: only a count of {personal}: {p:?}");
            assert!(p[0].to_lowercase().starts_with("count"), "{p:?}");
            assert!(!p[0].to_lowercase().contains("min("), "{p:?}");
            assert_eq!(measures(flavor, &cols).1, vec![4], "{flavor:?}");
        }
    }

    #[test]
    fn oracle_sql_is_what_ora_live_always_sent() {
        let sql = sql(Flavor::Oracle, &invoice());
        assert!(sql.contains(&"MIN(\"DATEINVOICED\")".to_string()));
        assert!(sql.contains(&"MAX(LENGTH(\"DOCUMENTNO\"))".to_string()));
        assert!(
            !sql.iter().any(|s| s.contains("NOTES")),
            "LONG is not countable"
        );
        assert!(
            sql.contains(&"COUNT(CASE WHEN \"ATTACHMENT\" IS NOT NULL THEN 1 END)".to_string())
        );
        let plan = plan_table(
            Flavor::Oracle,
            "ADEMPIERE",
            "C_INVOICE",
            &invoice(),
            ProfileTier::P1,
        )
        .unwrap();
        assert_eq!(
            plan.queries,
            ["SELECT COUNT(*) FROM \"ADEMPIERE\".\"C_INVOICE\""]
        );
    }

    #[test]
    fn postgres_and_clickhouse_sql() {
        let pg = sql(Flavor::Postgres, &pg_invoice());
        assert!(pg.contains(&"min(\"dateinvoiced\")::text".to_string()));
        assert!(pg.contains(&"max(length(\"documentno\"::text))".to_string()));
        assert!(pg.contains(&"count(\"payload\")".to_string()));
        assert!(
            !pg.iter()
                .any(|s| s.contains("tags") && !s.starts_with("count(")),
            "an array is counted only"
        );
        let ch = sql(Flavor::ClickHouse, &ch_invoice());
        assert!(ch.contains(&"toString(min(`dateinvoiced`))".to_string()));
        assert!(ch.contains(&"max(length(toString(`documentno`)))".to_string()));
        let plan = plan_table(
            Flavor::ClickHouse,
            "shop",
            "orders",
            &ch_invoice(),
            ProfileTier::P1,
        )
        .unwrap();
        assert_eq!(plan.queries, ["SELECT count() FROM `shop`.`orders`"]);
    }

    #[test]
    fn sqlserver_sql() {
        let ms = sql(Flavor::SqlServer, &ms_invoice());
        assert!(ms.contains(&"CONVERT(nvarchar(100), MIN([dateinvoiced]), 126)".to_string()));
        assert!(ms.contains(&"CONVERT(nvarchar(100), MAX([id]), 126)".to_string()));
        assert!(ms.contains(&"MAX(LEN([documentno]))".to_string()));
        assert!(ms.contains(&"COUNT_BIG([email])".to_string()));
        // `ntext` and `uniqueidentifier` are counted, through `IS NOT NULL`, and nothing else.
        for c in ["notes", "row_guid"] {
            let m: Vec<&String> = ms.iter().filter(|s| s.contains(c)).collect();
            assert_eq!(
                m,
                [&format!(
                    "COUNT_BIG(CASE WHEN [{c}] IS NOT NULL THEN 1 END)"
                )]
            );
        }
        let plan = plan_table(
            Flavor::SqlServer,
            "dbo",
            "orders",
            &ms_invoice(),
            ProfileTier::P1,
        )
        .unwrap();
        assert_eq!(plan.queries, ["SELECT COUNT_BIG(*) FROM [dbo].[orders]"]);
        let p2 = plan_table(
            Flavor::SqlServer,
            "dbo",
            "orders",
            &ms_invoice(),
            ProfileTier::P2,
        )
        .unwrap();
        assert!(p2.queries[0].starts_with("SELECT COUNT_BIG(*), COUNT_BIG([id]), "));
        assert!(p2.queries[0].ends_with(" FROM [dbo].[orders]"));
    }

    #[test]
    fn informix_sql() {
        let ifx = sql(Flavor::Informix, &ifx_invoice());
        assert!(ifx.contains(&"CAST(MIN(\"id\") AS LVARCHAR(100))".to_string()));
        // A date bound renders as ISO 8601 whatever the client's DBDATE says.
        assert!(ifx.contains(
            &"CAST(EXTEND(MAX(\"shipped\"), YEAR TO FRACTION(5)) AS LVARCHAR(100))".to_string()
        ));
        assert!(
            ifx.contains(
                &"CAST(EXTEND(MIN(\"dateinvoiced\"), YEAR TO FRACTION(5)) AS LVARCHAR(100))"
                    .to_string()
            )
        );
        assert!(ifx.contains(&"MAX(LENGTH(\"documentno\"))".to_string()));
        // A legacy `TEXT` blob is counted through `IS NOT NULL`, and nothing else.
        let notes: Vec<&String> = ifx.iter().filter(|s| s.contains("notes")).collect();
        assert_eq!(
            notes,
            [&"COUNT(CASE WHEN \"notes\" IS NOT NULL THEN 1 END)".to_string()]
        );
        let plan = plan_table(
            Flavor::Informix,
            "informix",
            "orders",
            &ifx_invoice(),
            ProfileTier::P1,
        )
        .unwrap();
        assert_eq!(
            plan.queries,
            ["SELECT COUNT(*) FROM \"informix\".\"orders\""]
        );
    }

    #[test]
    fn families_per_engine() {
        assert_eq!(family(Flavor::Postgres, "numeric(14,2)"), Family::Number);
        assert_eq!(
            family(Flavor::Postgres, "timestamp with time zone"),
            Family::Date
        );
        assert_eq!(family(Flavor::Postgres, "character(1)"), Family::Text);
        assert_eq!(family(Flavor::Postgres, "boolean"), Family::Other);
        assert_eq!(family(Flavor::Postgres, "integer[]"), Family::Other);
        assert_eq!(family(Flavor::Postgres, "interval"), Family::Other);
        assert_eq!(
            family(Flavor::ClickHouse, "Nullable(LowCardinality(String))"),
            Family::Text
        );
        assert_eq!(family(Flavor::ClickHouse, "Decimal(18, 4)"), Family::Number);
        assert_eq!(
            family(Flavor::ClickHouse, "Map(String, UInt8)"),
            Family::Other
        );
        assert_eq!(family(Flavor::Oracle, "TIMESTAMP(6)"), Family::Date);
        assert_eq!(family(Flavor::Oracle, "LONG RAW"), Family::Uncountable);
        assert_eq!(family(Flavor::SqlServer, "decimal(18,4)"), Family::Number);
        assert_eq!(family(Flavor::SqlServer, "datetimeoffset(7)"), Family::Date);
        assert_eq!(family(Flavor::SqlServer, "nvarchar(max)"), Family::Text);
        assert_eq!(family(Flavor::SqlServer, "text"), Family::Other);
        assert_eq!(family(Flavor::SqlServer, "bit"), Family::Other);
        assert_eq!(family(Flavor::SqlServer, "varbinary(max)"), Family::Other);
        assert_eq!(family(Flavor::Informix, "money(16,2)"), Family::Number);
        assert_eq!(family(Flavor::Informix, "bigserial"), Family::Number);
        assert_eq!(
            family(Flavor::Informix, "datetime year to fraction(3)"),
            Family::Date
        );
        assert_eq!(
            family(Flavor::Informix, "datetime hour to minute"),
            Family::Other
        );
        assert_eq!(family(Flavor::Informix, "nchar(10)"), Family::Text);
        assert_eq!(family(Flavor::Informix, "byte"), Family::Other);
        assert_eq!(family(Flavor::Informix, "boolean"), Family::Other);
        assert_eq!(
            family(Flavor::Informix, "interval day(3) to second"),
            Family::Other
        );
    }

    #[test]
    fn identifiers_are_quoted_or_refused() {
        assert_eq!(ident(Flavor::Postgres, "we\"ird").unwrap(), "\"we\"\"ird\"");
        assert_eq!(ident(Flavor::ClickHouse, "a`b").unwrap(), "`a\\`b`");
        assert_eq!(ident(Flavor::SqlServer, "we]ird").unwrap(), "[we]]ird]");
        assert_eq!(ident(Flavor::Informix, "we\"ird").unwrap(), "\"we\"\"ird\"");
        assert!(ident(Flavor::Oracle, "A\"B").is_err());
        assert!(ident(Flavor::Oracle, "A B").is_err());
        assert!(ident(Flavor::Postgres, "").is_err());
        assert!(ident(Flavor::Postgres, "a\0b").is_err());
    }

    #[test]
    fn p2_applies_exact_nulls_bounds_and_lengths() {
        let mut detail = json!({ "columns": pg_invoice() });
        let plan = plan_table(
            Flavor::Postgres,
            "public",
            "invoice",
            &pg_invoice(),
            ProfileTier::P2,
        )
        .unwrap();
        assert_eq!(plan.queries.len(), 1);
        // count(*), then: id count/min/max, documentno count/len, dateinvoiced count/min/max,
        // email count/len, phone count, tags count, payload count.
        let s = |v: &str| (!v.is_empty()).then(|| v.to_string());
        let row: Vec<Option<String>> = [
            "10",
            "10",
            "1",
            "10",
            "10",
            "12",
            "8",
            "2003-01-01 00:00:00",
            "2024-12-01 00:00:00",
            "3",
            "40",
            "2",
            "10",
            "0",
        ]
        .iter()
        .map(|v| s(v))
        .collect();
        let outcome = plan
            .apply(&[row], &mut detail, &RedactionConfig::default())
            .unwrap();
        assert_eq!(
            outcome,
            TableOutcome::Profiled {
                measured: 7,
                suppressed: 1
            }
        );
        assert_eq!(detail["profile"]["row_count"], 10);
        assert_eq!(detail["profile"]["tier"], "p2");
        let col = |i: usize| &detail["columns"][i]["profile"];
        assert_eq!(col(0)["min"], "1");
        assert_eq!(col(1)["max_length"], 12);
        assert!(col(1).get("min").is_none());
        assert_eq!(col(2)["nulls"], 2);
        assert_eq!(col(3)["nulls"], 7);
        assert_eq!(col(4)["values_suppressed"], true);
        assert!(col(4).get("min").is_none());
        assert_eq!(col(6)["nulls"], 10);
    }

    #[test]
    fn a_failed_scan_is_recorded_redacted_and_an_empty_count_is_not_zero() {
        let mut detail = json!({ "columns": [] });
        let r = profile_table(
            Flavor::Postgres,
            "public",
            "t",
            &mut detail,
            ProfileTier::P1,
            &RedactionConfig::default(),
            &mut |_| Err("permission denied for table t".into()),
        );
        assert!(r.is_err());
        assert!(
            detail["profile"]["error"]
                .as_str()
                .unwrap()
                .contains("permission")
        );
        let mut detail = json!({ "columns": [] });
        let r = profile_table(
            Flavor::Postgres,
            "public",
            "t",
            &mut detail,
            ProfileTier::P1,
            &RedactionConfig::default(),
            &mut |_| Ok(vec![None]),
        );
        assert!(r.is_err(), "an empty row count is not a count of zero");
    }

    #[test]
    fn many_columns_split_under_the_select_list_limit() {
        let cols: Vec<Value> = (0..600)
            .map(|i| json!({ "name": format!("n{i}"), "data_type": "integer" }))
            .collect();
        let plan = plan_table(Flavor::Postgres, "s", "wide", &cols, ProfileTier::P2).unwrap();
        assert_eq!(plan.queries.len(), 2, "1,800 aggregates in chunks of 900");
        let mut detail = json!({ "columns": cols });
        let answers: Vec<Vec<Option<String>>> = plan
            .slots
            .iter()
            .map(|s| vec![Some("5".to_string()); s.len() + 1])
            .collect();
        let outcome = plan
            .apply(&answers, &mut detail, &RedactionConfig::default())
            .unwrap();
        assert_eq!(
            outcome,
            TableOutcome::Profiled {
                measured: 600,
                suppressed: 0
            }
        );
    }

    #[test]
    fn p0_plans_nothing() {
        let plan = plan_table(Flavor::Postgres, "s", "t", &pg_invoice(), ProfileTier::P0).unwrap();
        assert!(plan.queries.is_empty());
    }
}
