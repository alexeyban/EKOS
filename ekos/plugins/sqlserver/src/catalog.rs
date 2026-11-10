//! The SQL Server catalog queries (`sys.*` views) and their pure mapping into RFC 0211's
//! per-owner objects (`ekos_observation_sdk::live_catalog`).
//!
//! Every query is a constant over the listed schemas, run on the read-only session
//! (`crate::session`). Text that leaves the database — comments (`MS_Description` extended
//! properties), defaults, computed-column and check expressions, module definitions
//! (`sys.sql_modules`) — is redacted here, before anything becomes an artifact (RFC 0043).

use ekos_common::redaction::{RedactionConfig, redact};
use ekos_observation_sdk::live_catalog::{LiveObject, OwnerCatalog};
use serde_json::{Value, json};
use std::collections::BTreeMap;

use crate::session::Rows;

/// The catalog family the artifacts name.
pub const FAMILY: &str = "sys";

/// `N'…'`, a T-SQL Unicode string literal.
pub fn literal(s: &str) -> String {
    format!("N'{}'", s.replace('\'', "''"))
}

/// `IN (…)` over the listed schemas.
fn schema_list(schemas: &[String]) -> String {
    schemas
        .iter()
        .map(|s| literal(s))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The comment (`MS_Description`) of an object (`minor` 0) or of one of its columns.
fn description(major: &str, minor: &str) -> String {
    format!(
        "(SELECT CAST(ep.value AS nvarchar(4000)) FROM sys.extended_properties ep \
         WHERE ep.class = 1 AND ep.major_id = {major} AND ep.minor_id = {minor} \
         AND ep.name = N'MS_Description')"
    )
}

/// The catalog queries for `schemas`, by name. Each one's columns are documented where it is
/// read in [`normalize`].
pub fn queries(schemas: &[String]) -> Vec<(&'static str, String)> {
    let list = schema_list(schemas);
    vec![
        (
            "census",
            format!(
                "SELECT s.name, RTRIM(o.type), COUNT_BIG(*) FROM sys.objects o \
                 JOIN sys.schemas s ON s.schema_id = o.schema_id \
                 WHERE s.name IN ({list}) AND o.is_ms_shipped = 0 \
                 GROUP BY s.name, o.type ORDER BY s.name, RTRIM(o.type)"
            ),
        ),
        (
            "indexes",
            format!(
                "SELECT s.name, COUNT_BIG(*) FROM sys.indexes i \
                 JOIN sys.objects o ON o.object_id = i.object_id \
                 JOIN sys.schemas s ON s.schema_id = o.schema_id \
                 WHERE s.name IN ({list}) AND o.is_ms_shipped = 0 AND o.type IN ('U', 'V') \
                 AND i.index_id > 0 AND i.is_hypothetical = 0 GROUP BY s.name ORDER BY s.name"
            ),
        ),
        (
            "tables",
            format!(
                "SELECT s.name, t.name, {}, t.temporal_type_desc, \
                 CAST(t.is_memory_optimized AS int) \
                 FROM sys.tables t JOIN sys.schemas s ON s.schema_id = t.schema_id \
                 WHERE s.name IN ({list}) AND t.is_ms_shipped = 0 ORDER BY s.name, t.name",
                description("t.object_id", "0")
            ),
        ),
        (
            "columns",
            format!(
                "SELECT s.name, o.name, c.name, \
                 COALESCE(CASE WHEN ut.is_user_defined = 0 THEN ut.name END, \
                          TYPE_NAME(c.system_type_id), ut.name), \
                 c.max_length, c.precision, c.scale, CAST(c.is_nullable AS int), \
                 CAST(c.is_identity AS int), dc.definition, cc.definition, {}, \
                 CASE WHEN ut.is_user_defined = 1 THEN ut.name END, RTRIM(o.type) \
                 FROM sys.columns c JOIN sys.objects o ON o.object_id = c.object_id \
                 JOIN sys.schemas s ON s.schema_id = o.schema_id \
                 JOIN sys.types ut ON ut.user_type_id = c.user_type_id \
                 LEFT JOIN sys.default_constraints dc ON dc.object_id = c.default_object_id \
                 LEFT JOIN sys.computed_columns cc \
                   ON cc.object_id = c.object_id AND cc.column_id = c.column_id \
                 WHERE s.name IN ({list}) AND o.is_ms_shipped = 0 AND o.type IN ('U', 'V') \
                 ORDER BY s.name, o.name, c.column_id",
                description("c.object_id", "c.column_id")
            ),
        ),
        (
            "keys",
            format!(
                "SELECT s.name, t.name, i.name, \
                 CASE WHEN i.is_primary_key = 1 THEN 'PK' \
                      WHEN i.is_unique_constraint = 1 THEN 'UQ' ELSE 'UX' END, c.name \
                 FROM sys.indexes i JOIN sys.tables t ON t.object_id = i.object_id \
                 JOIN sys.schemas s ON s.schema_id = t.schema_id \
                 JOIN sys.index_columns ic \
                   ON ic.object_id = i.object_id AND ic.index_id = i.index_id \
                 JOIN sys.columns c ON c.object_id = ic.object_id AND c.column_id = ic.column_id \
                 WHERE s.name IN ({list}) AND t.is_ms_shipped = 0 AND i.is_unique = 1 \
                 AND i.has_filter = 0 AND i.is_hypothetical = 0 AND ic.key_ordinal > 0 \
                 ORDER BY s.name, t.name, i.name, ic.key_ordinal"
            ),
        ),
        (
            "checks",
            format!(
                "SELECT s.name, t.name, cc.name, cc.definition, CAST(cc.is_not_trusted AS int), \
                 CAST(cc.is_disabled AS int) \
                 FROM sys.check_constraints cc JOIN sys.tables t ON t.object_id = cc.parent_object_id \
                 JOIN sys.schemas s ON s.schema_id = t.schema_id \
                 WHERE s.name IN ({list}) AND t.is_ms_shipped = 0 ORDER BY s.name, t.name, cc.name"
            ),
        ),
        (
            "foreign_keys",
            format!(
                "SELECT s.name, t.name, fk.name, rs.name, rt.name, \
                 fk.delete_referential_action_desc, CAST(fk.is_not_trusted AS int), \
                 CAST(fk.is_disabled AS int), pc.name, rc.name \
                 FROM sys.foreign_keys fk \
                 JOIN sys.tables t ON t.object_id = fk.parent_object_id \
                 JOIN sys.schemas s ON s.schema_id = t.schema_id \
                 JOIN sys.tables rt ON rt.object_id = fk.referenced_object_id \
                 JOIN sys.schemas rs ON rs.schema_id = rt.schema_id \
                 JOIN sys.foreign_key_columns fkc ON fkc.constraint_object_id = fk.object_id \
                 JOIN sys.columns pc \
                   ON pc.object_id = fkc.parent_object_id AND pc.column_id = fkc.parent_column_id \
                 JOIN sys.columns rc ON rc.object_id = fkc.referenced_object_id \
                   AND rc.column_id = fkc.referenced_column_id \
                 WHERE s.name IN ({list}) AND t.is_ms_shipped = 0 \
                 ORDER BY s.name, t.name, fk.name, fkc.constraint_column_id"
            ),
        ),
        (
            "modules",
            format!(
                "SELECT s.name, o.name, RTRIM(o.type), m.definition, p.name, \
                 CAST(tr.is_instead_of_trigger AS int), CAST(tr.is_disabled AS int), {}, \
                 CAST(OBJECTPROPERTY(o.object_id, 'IsEncrypted') AS int) \
                 FROM sys.objects o JOIN sys.schemas s ON s.schema_id = o.schema_id \
                 LEFT JOIN sys.sql_modules m ON m.object_id = o.object_id \
                 LEFT JOIN sys.triggers tr ON tr.object_id = o.object_id \
                 LEFT JOIN sys.objects p ON p.object_id = o.parent_object_id AND o.type = 'TR' \
                 WHERE s.name IN ({list}) AND o.is_ms_shipped = 0 \
                 AND o.type IN ('V', 'P', 'FN', 'IF', 'TF', 'TR') \
                 ORDER BY s.name, o.name",
                description("o.object_id", "0")
            ),
        ),
        (
            "parameters",
            format!(
                "SELECT s.name, o.name, pa.name, TYPE_NAME(pa.user_type_id), pa.max_length, \
                 pa.precision, pa.scale, CAST(pa.is_output AS int), pa.parameter_id \
                 FROM sys.parameters pa JOIN sys.objects o ON o.object_id = pa.object_id \
                 JOIN sys.schemas s ON s.schema_id = o.schema_id \
                 WHERE s.name IN ({list}) AND o.is_ms_shipped = 0 \
                 AND o.type IN ('P', 'FN', 'IF', 'TF') ORDER BY s.name, o.name, pa.parameter_id"
            ),
        ),
        (
            "trigger_events",
            format!(
                "SELECT s.name, o.name, te.type_desc \
                 FROM sys.trigger_events te JOIN sys.objects o ON o.object_id = te.object_id \
                 JOIN sys.schemas s ON s.schema_id = o.schema_id \
                 WHERE s.name IN ({list}) AND o.is_ms_shipped = 0 \
                 ORDER BY s.name, o.name, te.type_desc"
            ),
        ),
    ]
}

/// RFC 0211 §3.4 P0: `sys.partitions.rows` per table (heap or clustered index), which SQL
/// Server documents as approximate — so `row_count_exact: false`. Readable with metadata
/// visibility alone (`sys.dm_db_partition_stats` needs `VIEW DATABASE STATE`).
pub fn row_estimates(schemas: &[String]) -> String {
    format!(
        "SELECT s.name, t.name, SUM(p.rows) FROM sys.partitions p \
         JOIN sys.tables t ON t.object_id = p.object_id \
         JOIN sys.schemas s ON s.schema_id = t.schema_id \
         WHERE s.name IN ({}) AND p.index_id IN (0, 1) AND t.is_ms_shipped = 0 \
         GROUP BY s.name, t.name",
        schema_list(schemas)
    )
}

/// The write check (RFC 0211's `write_check`). SQL Server has no read-only session flag, so the
/// login's rights are asked for instead of a write attempted: tables of the listed schemas it may
/// `INSERT`/`UPDATE`/`DELETE`, listed schemas it may `ALTER` (create, drop or change objects), and
/// whether the database itself is read-only. One row: (tables writable, schemas alterable,
/// updateability).
pub fn write_check(schemas: &[String]) -> String {
    let list = schema_list(schemas);
    format!(
        "SELECT \
         (SELECT COUNT_BIG(*) FROM sys.tables t JOIN sys.schemas s ON s.schema_id = t.schema_id \
          WHERE s.name IN ({list}) AND t.is_ms_shipped = 0 AND ( \
          HAS_PERMS_BY_NAME(QUOTENAME(s.name) + '.' + QUOTENAME(t.name), 'OBJECT', 'INSERT') = 1 \
          OR HAS_PERMS_BY_NAME(QUOTENAME(s.name) + '.' + QUOTENAME(t.name), 'OBJECT', 'UPDATE') = 1 \
          OR HAS_PERMS_BY_NAME(QUOTENAME(s.name) + '.' + QUOTENAME(t.name), 'OBJECT', 'DELETE') = 1)), \
         (SELECT COUNT_BIG(*) FROM sys.schemas s WHERE s.name IN ({list}) \
          AND HAS_PERMS_BY_NAME(QUOTENAME(s.name), 'SCHEMA', 'ALTER') = 1), \
         CAST(DATABASEPROPERTYEX(DB_NAME(), 'Updateability') AS nvarchar(32))"
    )
}

/// The write check's answer in RFC 0211's shape: refused unless the login may change something.
pub fn write_check_result(row: &[Option<String>]) -> Value {
    let n = |i: usize| {
        row.get(i)
            .and_then(Option::as_deref)
            .and_then(|v| v.trim().parse::<i64>().ok())
            .unwrap_or(0)
    };
    let updateability = row.get(2).and_then(Option::as_deref).unwrap_or_default();
    let result = if updateability.eq_ignore_ascii_case("READ_ONLY") {
        Err("the database is READ_ONLY (DATABASEPROPERTYEX Updateability)".to_string())
    } else if n(0) == 0 && n(1) == 0 {
        Err(
            "the login has no INSERT, UPDATE or DELETE permission on any listed table and no \
             ALTER permission on any listed schema (HAS_PERMS_BY_NAME)"
                .to_string(),
        )
    } else {
        Ok(())
    };
    let mut v = ekos_observation_sdk::live_catalog::write_check(result);
    v["method"] = "permissions".into();
    v["writable_tables"] = n(0).into();
    v["alterable_schemas"] = n(1).into();
    v
}

/// One row of a catalog query.
type Row = Vec<Option<String>>;

/// What the catalog queries returned, by query name.
pub type CatalogRows = BTreeMap<String, Rows>;

fn cell(row: &[Option<String>], i: usize) -> Option<&str> {
    row.get(i).and_then(Option::as_deref)
}

fn text(row: &[Option<String>], i: usize) -> String {
    cell(row, i).unwrap_or_default().to_string()
}

fn flag(row: &[Option<String>], i: usize) -> bool {
    cell(row, i).is_some_and(|v| v.trim() == "1")
}

fn int(row: &[Option<String>], i: usize) -> Option<i64> {
    cell(row, i).and_then(|v| v.trim().parse().ok())
}

/// A type as T-SQL writes it: `nvarchar(30)`, `varchar(max)`, `decimal(12,2)`, `datetime2(3)`.
/// `max_length` is in bytes (`-1` = `max`), so `nchar`/`nvarchar` halve it.
pub fn type_text(
    base: &str,
    max_length: Option<i64>,
    precision: Option<i64>,
    scale: Option<i64>,
) -> String {
    let lower = base.to_ascii_lowercase();
    let len = |divide: i64| match max_length {
        Some(-1) => "(max)".to_string(),
        Some(n) => format!("({})", n / divide),
        None => String::new(),
    };
    match lower.as_str() {
        "varchar" | "char" | "varbinary" | "binary" => format!("{base}{}", len(1)),
        "nvarchar" | "nchar" => format!("{base}{}", len(2)),
        "decimal" | "numeric" => {
            format!("{base}({},{})", precision.unwrap_or(18), scale.unwrap_or(0))
        }
        "datetime2" | "datetimeoffset" | "time" => match scale {
            Some(s) => format!("{base}({s})"),
            None => base.to_string(),
        },
        _ => base.to_string(),
    }
}

/// The census type of a `sys.objects.type` code, or `None` for internal objects.
pub fn census_type(code: &str) -> Option<&'static str> {
    Some(match code.trim() {
        "U" => "TABLE",
        "V" => "VIEW",
        "P" => "PROCEDURE",
        "FN" | "IF" | "TF" => "FUNCTION",
        "TR" => "TRIGGER",
        "PC" => "CLR_PROCEDURE",
        "FS" | "FT" | "AF" => "CLR_FUNCTION",
        "TA" => "CLR_TRIGGER",
        "PK" | "UQ" | "F" | "C" | "D" | "EC" => "CONSTRAINT",
        "SO" => "SEQUENCE",
        "SN" => "SYNONYM",
        "TT" => "TABLE_TYPE",
        "X" => "EXTENDED_PROCEDURE",
        "R" => "RULE",
        "RF" => "REPLICATION_FILTER",
        "SQ" | "IT" | "S" | "ST" => return None,
        _ => "OTHER",
    })
}

/// `NO_ACTION` → `NO ACTION`, as the other catalogs spell it.
fn on_delete(desc: &str) -> String {
    desc.replace('_', " ")
}

/// A check constraint's text without the outer parentheses SQL Server stores around it.
pub fn check_expression(definition: &str) -> String {
    let d = definition.trim();
    match d.strip_prefix('(').and_then(|d| d.strip_suffix(')')) {
        // `(a) AND (b)` is not wrapped as a whole: only strip a pair that matches.
        Some(inner) if balanced(inner) => inner.trim().to_string(),
        _ => d.to_string(),
    }
}

fn balanced(s: &str) -> bool {
    let mut depth = 0i32;
    for c in s.chars() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
            }
            _ => {}
        }
    }
    depth == 0
}

type Key = (String, String);

fn by_object(rows: &Rows) -> BTreeMap<Key, Vec<&Vec<Option<String>>>> {
    let mut m: BTreeMap<Key, Vec<&Vec<Option<String>>>> = BTreeMap::new();
    for r in rows {
        m.entry((text(r, 0), text(r, 1))).or_default().push(r);
    }
    m
}

fn routine_kind(code: &str) -> Option<&'static str> {
    Some(match code.trim() {
        "P" => "procedure",
        "FN" | "IF" | "TF" => "function",
        "TR" => "trigger",
        "V" => "view",
        _ => return None,
    })
}

/// The rows of every catalog query as RFC 0211's per-owner objects. Pure. A listed schema with
/// nothing in it still gets a catalog, so the census says "nothing there".
pub fn normalize(
    schemas: &[String],
    rows: &CatalogRows,
    redaction: &RedactionConfig,
) -> Vec<OwnerCatalog> {
    let empty = Rows::new();
    let get = |name: &str| rows.get(name).unwrap_or(&empty);
    let r = |t: &str| redact(t, redaction);
    let mut owners: BTreeMap<String, OwnerCatalog> = schemas
        .iter()
        .map(|s| {
            (
                s.clone(),
                OwnerCatalog {
                    owner: s.clone(),
                    family: FAMILY.into(),
                    ..Default::default()
                },
            )
        })
        .collect();
    // census: schema, type code, count
    for row in get("census") {
        let (Some(c), Some(t), Some(n)) = (
            owners.get_mut(&text(row, 0)),
            census_type(&text(row, 1)),
            int(row, 2),
        ) else {
            continue;
        };
        c.census.entry(t.to_string()).or_default().valid += n as usize;
    }
    // indexes: schema, count
    for row in get("indexes") {
        if let (Some(c), Some(n)) = (owners.get_mut(&text(row, 0)), int(row, 1)) {
            c.census.entry("INDEX".into()).or_default().valid += n as usize;
        }
    }
    // columns: schema, relation, name, type, max_length, precision, scale, nullable, identity,
    // default, computed, comment, user type, relation type
    let mut columns: BTreeMap<Key, Vec<Value>> = BTreeMap::new();
    for row in get("columns") {
        let schema = text(row, 0);
        let Some(c) = owners.get_mut(&schema) else {
            continue;
        };
        c.columns += 1;
        let mut col = json!({
            "name": text(row, 2),
            "data_type": type_text(&text(row, 3), int(row, 4), int(row, 5), int(row, 6)),
            "nullable": flag(row, 7),
        });
        if flag(row, 8) {
            col["identity"] = "IDENTITY".into();
        }
        for (i, key) in [(9, "default"), (10, "generated"), (11, "comment")] {
            if let Some(v) = cell(row, i).filter(|v| !v.is_empty()) {
                col[key] = r(v).into();
            }
        }
        if let Some(u) = cell(row, 12) {
            col["user_type"] = u.into();
        }
        columns.entry((schema, text(row, 1))).or_default().push(col);
    }
    // tables: schema, name, comment, temporal type, memory-optimized
    for row in get("tables") {
        let (schema, name) = (text(row, 0), text(row, 1));
        let Some(c) = owners.get_mut(&schema) else {
            continue;
        };
        let cols = columns
            .remove(&(schema.clone(), name.clone()))
            .unwrap_or_default();
        let mut detail = json!({ "columns": cols });
        if let Some(v) = cell(row, 2).filter(|v| !v.is_empty()) {
            detail["comment"] = r(v).into();
        }
        match cell(row, 3) {
            Some(t) if t != "NON_TEMPORAL_TABLE" => detail["temporal"] = t.into(),
            _ => {}
        }
        if flag(row, 4) {
            detail["memory_optimized"] = true.into();
        }
        c.objects.push(LiveObject {
            kind: "table".into(),
            owner: schema.clone(),
            name,
            status: None,
            detail,
        });
    }
    add_keys(&mut owners, get("keys"));
    add_checks(&mut owners, get("checks"), redaction);
    add_foreign_keys(&mut owners, get("foreign_keys"));
    add_modules(&mut owners, rows, redaction);
    owners.into_values().collect()
}

fn add_keys(owners: &mut BTreeMap<String, OwnerCatalog>, rows: &Rows) {
    // schema, table, index, PK|UQ|UX, column (in key order)
    let mut keys: BTreeMap<(String, String, String), (String, Vec<String>)> = BTreeMap::new();
    for row in rows {
        keys.entry((text(row, 0), text(row, 1), text(row, 2)))
            .or_insert_with(|| (text(row, 3), Vec::new()))
            .1
            .push(text(row, 4));
    }
    for ((schema, table, name), (kind, cols)) in keys {
        let Some(c) = owners.get_mut(&schema) else {
            continue;
        };
        let mut detail = json!({ "table": table, "columns": cols, "validated": true });
        if kind == "UX" {
            // A unique index, not a declared constraint: unique all the same.
            detail["index"] = true.into();
        }
        c.objects.push(LiveObject {
            kind: if kind == "PK" {
                "primary_key"
            } else {
                "unique_constraint"
            }
            .into(),
            owner: schema.clone(),
            name: format!("{table}.{name}"),
            status: None,
            detail,
        });
    }
}

fn add_checks(
    owners: &mut BTreeMap<String, OwnerCatalog>,
    rows: &Rows,
    redaction: &RedactionConfig,
) {
    // schema, table, name, definition, not trusted, disabled
    for row in rows {
        let Some(c) = owners.get_mut(&text(row, 0)) else {
            continue;
        };
        let table = text(row, 1);
        c.objects.push(LiveObject {
            kind: "check_constraint".into(),
            owner: c.owner.clone(),
            name: format!("{table}.{}", text(row, 2)),
            status: flag(row, 5).then(|| "DISABLED".to_string()),
            detail: json!({
                "table": table,
                "columns": [],
                "validated": !flag(row, 4),
                "condition": redact(&check_expression(&text(row, 3)), redaction),
            }),
        });
    }
}

fn add_foreign_keys(owners: &mut BTreeMap<String, OwnerCatalog>, rows: &Rows) {
    // schema, table, name, ref schema, ref table, delete action, not trusted, disabled, column,
    // referenced column (in constraint order)
    let mut fks: BTreeMap<(String, String, String), Vec<&Row>> = BTreeMap::new();
    for row in rows {
        fks.entry((text(row, 0), text(row, 1), text(row, 2)))
            .or_default()
            .push(row);
    }
    for ((schema, table, name), cols) in fks {
        let Some(c) = owners.get_mut(&schema) else {
            continue;
        };
        let first = cols[0];
        c.objects.push(LiveObject {
            kind: "foreign_key".into(),
            owner: schema.clone(),
            name: format!("{table}.{name}"),
            status: flag(first, 7).then(|| "DISABLED".to_string()),
            detail: json!({
                "table": table,
                "columns": cols.iter().map(|r| text(r, 8)).collect::<Vec<_>>(),
                "references_owner": text(first, 3),
                "references_table": text(first, 4),
                "references_columns": cols.iter().map(|r| text(r, 9)).collect::<Vec<_>>(),
                "validated": !flag(first, 6),
                "on_delete": on_delete(&text(first, 5)),
            }),
        });
    }
}

/// `@id int`, `@total decimal(12,2) OUTPUT`.
fn parameter(row: &[Option<String>]) -> String {
    let mut p = format!(
        "{} {}",
        text(row, 2),
        type_text(&text(row, 3), int(row, 4), int(row, 5), int(row, 6))
    );
    if flag(row, 7) {
        p.push_str(" OUTPUT");
    }
    p
}

fn add_modules(
    owners: &mut BTreeMap<String, OwnerCatalog>,
    rows: &CatalogRows,
    redaction: &RedactionConfig,
) {
    let empty = Rows::new();
    let params = by_object(rows.get("parameters").unwrap_or(&empty));
    let events = by_object(rows.get("trigger_events").unwrap_or(&empty));
    // schema, name, type code, definition, trigger's table, instead of, disabled, comment,
    // encrypted
    for row in rows.get("modules").unwrap_or(&empty) {
        let (schema, name) = (text(row, 0), text(row, 1));
        let (Some(c), Some(kind)) = (owners.get_mut(&schema), routine_kind(&text(row, 2))) else {
            continue;
        };
        let key = (schema.clone(), name.clone());
        let mut detail = json!({ "text": redact(&text(row, 3), redaction), "language": "tsql" });
        if flag(row, 8) || cell(row, 3).is_none() {
            // `WITH ENCRYPTION`: the catalog keeps no readable text.
            detail["encrypted"] = true.into();
        }
        if let Some(v) = cell(row, 7).filter(|v| !v.is_empty()) {
            detail["comment"] = redact(v, redaction).into();
        }
        match kind {
            "trigger" => {
                detail["table"] = text(row, 4).into();
                detail["timing"] = if flag(row, 5) { "INSTEAD OF" } else { "AFTER" }.into();
                detail["events"] = events
                    .get(&key)
                    .map(|rs| rs.iter().map(|r| text(r, 2)).collect::<Vec<_>>())
                    .unwrap_or_default()
                    .into();
            }
            "procedure" | "function" => {
                let ps = params.get(&key).cloned().unwrap_or_default();
                // Parameter 0 of a scalar function is its return value.
                let (ret, args): (Vec<_>, Vec<_>) =
                    ps.into_iter().partition(|r| int(r, 8) == Some(0));
                detail["arguments"] = args.iter().map(|r| parameter(r)).collect::<Vec<_>>().into();
                if let Some(r) = ret.first() {
                    detail["returns"] =
                        type_text(&text(r, 3), int(r, 4), int(r, 5), int(r, 6)).into();
                } else if text(row, 2).trim() != "P" {
                    detail["returns"] = "TABLE".into();
                }
            }
            _ => {}
        }
        let status = (kind == "trigger" && flag(row, 6)).then(|| "DISABLED".to_string());
        c.objects.push(LiveObject {
            kind: kind.into(),
            owner: schema,
            name,
            status,
            detail,
        });
    }
}

/// RFC 0157's rule for live catalogs: a catalog shorter than the server's own count is an error,
/// never a smaller model. Tables, views and SQL routines read must equal `sys.objects`' counts.
pub fn reconcile(catalogs: &[OwnerCatalog]) -> Result<(), String> {
    for c in catalogs {
        let listed = c.listed();
        for (census, kinds) in [
            ("TABLE", &["table"][..]),
            ("VIEW", &["view"]),
            ("PROCEDURE", &["procedure"]),
            ("FUNCTION", &["function"]),
            ("TRIGGER", &["trigger"]),
        ] {
            let counted = c.census.get(census).map_or(0, |n| n.valid);
            let read: usize = kinds.iter().filter_map(|k| listed.get(*k)).sum();
            if counted != read {
                return Err(format!(
                    "schema {}: sys.objects counts {counted} {census} object(s) but the catalog \
                     read {read}; refusing a partial catalog (does the login see every object?)",
                    c.owner
                ));
            }
        }
    }
    Ok(())
}

/// The unit tests' fixture rows, for the crate's other tests.
#[cfg(test)]
pub(crate) fn tests_fixture() -> CatalogRows {
    tests::fixture()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(cells: &[&str]) -> Vec<Option<String>> {
        cells
            .iter()
            .map(|c| (*c != "NULL").then(|| c.to_string()))
            .collect()
    }

    pub(crate) fn fixture() -> CatalogRows {
        let mut m = CatalogRows::new();
        m.insert(
            "census".into(),
            vec![
                row(&["sales", "U", "2"]),
                row(&["sales", "V", "1"]),
                row(&["sales", "P", "1"]),
                row(&["sales", "FN", "1"]),
                row(&["sales", "TR", "1"]),
                row(&["sales", "PK", "2"]),
                row(&["sales", "F", "1"]),
                row(&["sales", "C", "1"]),
                row(&["sales", "D", "1"]),
                row(&["sales", "IT", "4"]),
            ],
        );
        m.insert("indexes".into(), vec![row(&["sales", "3"])]);
        m.insert(
            "tables".into(),
            vec![
                row(&["sales", "customers", "Who buys", "NON_TEMPORAL_TABLE", "0"]),
                row(&[
                    "sales",
                    "orders",
                    "NULL",
                    "SYSTEM_VERSIONED_TEMPORAL_TABLE",
                    "0",
                ]),
            ],
        );
        m.insert(
            "columns".into(),
            vec![
                row(&[
                    "sales",
                    "customers",
                    "id",
                    "int",
                    "4",
                    "10",
                    "0",
                    "0",
                    "1",
                    "NULL",
                    "NULL",
                    "NULL",
                    "NULL",
                    "U",
                ]),
                row(&[
                    "sales",
                    "customers",
                    "name",
                    "nvarchar",
                    "200",
                    "0",
                    "0",
                    "1",
                    "0",
                    "NULL",
                    "NULL",
                    "Display name",
                    "NULL",
                    "U",
                ]),
                row(&[
                    "sales", "orders", "id", "bigint", "8", "19", "0", "0", "1", "NULL", "NULL",
                    "NULL", "NULL", "U",
                ]),
                row(&[
                    "sales",
                    "orders",
                    "customer_id",
                    "int",
                    "4",
                    "10",
                    "0",
                    "1",
                    "0",
                    "NULL",
                    "NULL",
                    "NULL",
                    "NULL",
                    "U",
                ]),
                row(&[
                    "sales", "orders", "total", "decimal", "9", "12", "2", "0", "0", "((0))",
                    "NULL", "NULL", "NULL", "U",
                ]),
                row(&[
                    "sales", "orders", "note", "varchar", "-1", "0", "0", "1", "0", "NULL", "NULL",
                    "NULL", "NULL", "U",
                ]),
                row(&[
                    "sales",
                    "big_orders",
                    "id",
                    "bigint",
                    "8",
                    "19",
                    "0",
                    "0",
                    "0",
                    "NULL",
                    "NULL",
                    "NULL",
                    "NULL",
                    "V",
                ]),
            ],
        );
        m.insert(
            "keys".into(),
            vec![
                row(&["sales", "customers", "PK_customers", "PK", "id"]),
                row(&["sales", "orders", "PK_orders", "PK", "id"]),
                row(&["sales", "orders", "UX_orders_ref", "UX", "customer_id"]),
                row(&["sales", "orders", "UX_orders_ref", "UX", "id"]),
            ],
        );
        m.insert(
            "checks".into(),
            vec![row(&[
                "sales",
                "orders",
                "CK_orders_total",
                "([total]>=(0))",
                "1",
                "0",
            ])],
        );
        m.insert(
            "foreign_keys".into(),
            vec![row(&[
                "sales",
                "orders",
                "FK_orders_customers",
                "sales",
                "customers",
                "CASCADE",
                "0",
                "0",
                "customer_id",
                "id",
            ])],
        );
        m.insert(
            "modules".into(),
            vec![
                row(&["sales", "big_orders", "V", "CREATE VIEW sales.big_orders AS SELECT id FROM sales.orders WHERE total > 100", "NULL", "NULL", "NULL", "NULL", "0"]),
                row(&["sales", "close_order", "P", "CREATE PROCEDURE sales.close_order @id bigint AS UPDATE sales.orders SET note = 'closed' WHERE id = @id", "NULL", "NULL", "NULL", "Closes one order", "0"]),
                row(&["sales", "order_total", "FN", "CREATE FUNCTION sales.order_total(@id bigint) RETURNS decimal(12,2) AS BEGIN RETURN (SELECT total FROM sales.orders WHERE id = @id) END", "NULL", "NULL", "NULL", "NULL", "0"]),
                row(&["sales", "orders_audit", "TR", "CREATE TRIGGER sales.orders_audit ON sales.orders AFTER INSERT AS SELECT 1", "orders", "0", "1", "NULL", "0"]),
            ],
        );
        m.insert(
            "parameters".into(),
            vec![
                row(&[
                    "sales",
                    "close_order",
                    "@id",
                    "bigint",
                    "8",
                    "19",
                    "0",
                    "0",
                    "1",
                ]),
                row(&[
                    "sales",
                    "order_total",
                    "",
                    "decimal",
                    "9",
                    "12",
                    "2",
                    "1",
                    "0",
                ]),
                row(&[
                    "sales",
                    "order_total",
                    "@id",
                    "bigint",
                    "8",
                    "19",
                    "0",
                    "0",
                    "1",
                ]),
            ],
        );
        m.insert(
            "trigger_events".into(),
            vec![row(&["sales", "orders_audit", "INSERT"])],
        );
        m
    }

    fn catalog() -> OwnerCatalog {
        normalize(&["sales".into()], &fixture(), &RedactionConfig::default()).remove(0)
    }

    #[test]
    fn census_counts_what_sys_objects_counts_and_skips_internal_objects() {
        let c = catalog();
        assert_eq!(c.family, "sys");
        assert_eq!(c.census["TABLE"].valid, 2);
        assert_eq!(c.census["CONSTRAINT"].valid, 5);
        assert_eq!(c.census["INDEX"].valid, 3);
        assert!(
            !c.census.contains_key("OTHER"),
            "internal tables are not counted"
        );
        assert_eq!(c.columns, 7, "table and view columns");
        reconcile(&[c]).unwrap();
    }

    #[test]
    fn tables_carry_typed_columns_comments_and_temporal_state() {
        let c = catalog();
        let customers = c.objects.iter().find(|o| o.name == "customers").unwrap();
        assert_eq!(customers.detail["comment"], "Who buys");
        let cols = customers.detail["columns"].as_array().unwrap();
        assert_eq!(cols[0]["identity"], "IDENTITY");
        assert_eq!(cols[0]["nullable"], false);
        assert_eq!(cols[1]["data_type"], "nvarchar(100)", "bytes halved");
        assert_eq!(cols[1]["comment"], "Display name");
        let orders = c.objects.iter().find(|o| o.name == "orders").unwrap();
        assert_eq!(orders.detail["temporal"], "SYSTEM_VERSIONED_TEMPORAL_TABLE");
        let cols = orders.detail["columns"].as_array().unwrap();
        assert_eq!(cols[2]["data_type"], "decimal(12,2)");
        assert_eq!(cols[2]["default"], "((0))");
        assert_eq!(cols[3]["data_type"], "varchar(max)");
    }

    #[test]
    fn keys_checks_and_foreign_keys_keep_their_table_and_column_order() {
        let c = catalog();
        let ux = c
            .objects
            .iter()
            .find(|o| o.name == "orders.UX_orders_ref")
            .unwrap();
        assert_eq!(ux.kind, "unique_constraint");
        assert_eq!(ux.detail["columns"], json!(["customer_id", "id"]));
        assert_eq!(ux.detail["index"], true);
        let ck = c
            .objects
            .iter()
            .find(|o| o.kind == "check_constraint")
            .unwrap();
        assert_eq!(ck.detail["condition"], "[total]>=(0)");
        assert_eq!(ck.detail["validated"], false, "WITH NOCHECK is not trusted");
        let fk = c.objects.iter().find(|o| o.kind == "foreign_key").unwrap();
        assert_eq!(fk.name, "orders.FK_orders_customers");
        assert_eq!(fk.detail["references_table"], "customers");
        assert_eq!(fk.detail["on_delete"], "CASCADE");
        assert_eq!(fk.detail["validated"], true);
    }

    #[test]
    fn modules_become_views_routines_and_triggers_with_their_text() {
        let c = catalog();
        let listed = c.listed();
        assert_eq!(
            (
                listed["view"],
                listed["procedure"],
                listed["function"],
                listed["trigger"]
            ),
            (1, 1, 1, 1)
        );
        let p = c.objects.iter().find(|o| o.kind == "procedure").unwrap();
        assert_eq!(p.detail["arguments"], json!(["@id bigint"]));
        assert_eq!(p.detail["comment"], "Closes one order");
        let f = c.objects.iter().find(|o| o.kind == "function").unwrap();
        assert_eq!(f.detail["returns"], "decimal(12,2)");
        assert_eq!(f.detail["arguments"], json!(["@id bigint"]));
        let t = c.objects.iter().find(|o| o.kind == "trigger").unwrap();
        assert_eq!(t.detail["table"], "orders");
        assert_eq!(t.detail["events"], json!(["INSERT"]));
        assert_eq!(t.status.as_deref(), Some("DISABLED"));
    }

    #[test]
    fn a_short_catalog_is_refused() {
        let mut rows = fixture();
        rows.get_mut("tables").unwrap().pop();
        let c = normalize(&["sales".into()], &rows, &RedactionConfig::default());
        let e = reconcile(&c).unwrap_err();
        assert!(e.contains("2 TABLE") && e.contains("read 1"), "{e}");
    }

    #[test]
    fn a_listed_schema_with_nothing_in_it_still_has_a_catalog() {
        let c = normalize(
            &["empty".into()],
            &CatalogRows::new(),
            &RedactionConfig::default(),
        );
        assert_eq!(c.len(), 1);
        assert!(c[0].objects.is_empty());
        reconcile(&c).unwrap();
    }

    #[test]
    fn the_write_check_reads_permissions_not_a_write() {
        let refused = write_check_result(&row(&["0", "0", "READ_WRITE"]));
        assert_eq!(refused["result"], "refused_by_server");
        assert_eq!(refused["method"], "permissions");
        assert_eq!(
            write_check_result(&row(&["3", "0", "READ_WRITE"]))["result"],
            "user_can_write"
        );
        assert_eq!(
            write_check_result(&row(&["0", "1", "READ_WRITE"]))["result"],
            "user_can_write"
        );
        let ro = write_check_result(&row(&["3", "1", "READ_ONLY"]));
        assert_eq!(ro["result"], "refused_by_server");
        for q in queries(&["s".into()]) {
            assert!(q.1.trim_start().starts_with("SELECT"), "{}", q.0);
        }
        assert!(write_check(&["s".into()]).starts_with("SELECT"));
    }

    #[test]
    fn literals_and_types_are_spelled_as_t_sql_does() {
        assert_eq!(literal("o'brien"), "N'o''brien'");
        assert_eq!(type_text("nchar", Some(20), None, None), "nchar(10)");
        assert_eq!(
            type_text("varbinary", Some(-1), None, None),
            "varbinary(max)"
        );
        assert_eq!(
            type_text("datetime2", Some(8), Some(27), Some(7)),
            "datetime2(7)"
        );
        assert_eq!(type_text("int", Some(4), Some(10), Some(0)), "int");
        assert_eq!(check_expression("([a]>(0))"), "[a]>(0)");
        assert_eq!(
            check_expression("([a]>(0)) AND ([b]<(1))"),
            "([a]>(0)) AND ([b]<(1))"
        );
        assert_eq!(census_type("FN"), Some("FUNCTION"));
        assert_eq!(census_type("IT"), None);
    }

    #[test]
    fn redaction_runs_on_text_that_leaves_the_database() {
        let mut rows = fixture();
        rows.get_mut("modules").unwrap()[1][3] =
            Some("CREATE PROCEDURE sales.p AS SELECT 'AKIAIOSFODNN7EXAMPLE' AS k".into());
        let c = normalize(&["sales".into()], &rows, &RedactionConfig::default()).remove(0);
        let p = c.objects.iter().find(|o| o.kind == "procedure").unwrap();
        assert!(
            !p.detail["text"]
                .as_str()
                .unwrap()
                .contains("AKIAIOSFODNN7EXAMPLE")
        );
    }
}
