//! The Informix catalog queries (the `sys*` system catalog tables of the connected database) and
//! their pure mapping into RFC 0211's per-owner objects (`ekos_observation_sdk::live_catalog`).
//!
//! Every query is a constant over the listed owners (`systables.owner`), run on the read-only
//! session (`crate::session`). Text that leaves the database — view text (`sysviews`), routine
//! text and `DOCUMENT` clauses (`sysprocbody`), trigger text (`systrigbody`), check conditions
//! (`syschecks`) and literal defaults (`sysdefaults`) — is redacted here, before anything becomes an
//! artifact (RFC 0043). Informix keeps no comments on tables or columns; a routine's `DOCUMENT`
//! text is its comment.
//!
//! Long text is stored in fixed-width `CHAR` pieces (`sysviews.viewtext` 64, `syschecks.checktext`
//! 32, `sysprocbody.data` / `systrigbody.data` 256), split wherever the width ran out — inside a
//! word or right before a blank. A piece is padded back to its width before the next is appended
//! ([`join_pieces`]), so a blank at a boundary survives a driver that trims `CHAR` values.
//!
//! The optimizer's per-column `colmin` / `colmax` (second-smallest and second-largest *values*) and
//! the `sysdistrib` distributions are never read.

use ekos_common::redaction::{RedactionConfig, redact};
use ekos_observation_sdk::live_catalog::{LiveObject, OwnerCatalog};
use serde_json::{Value, json};
use std::collections::BTreeMap;

use crate::session::{Guards, Rows};

/// The catalog family the artifacts name.
pub const FAMILY: &str = "systables";

/// Widths of the `CHAR` pieces long text is stored in, when the catalog does not say
/// (`piece_widths` reads the declared widths: `sysviews.viewtext` is 64 in older servers, 256 in
/// newer ones).
pub const VIEW_PIECE: usize = 64;
pub const CHECK_PIECE: usize = 32;
pub const BODY_PIECE: usize = 256;

/// `'…'`, an Informix string literal.
pub fn literal(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// `IN (…)` over the listed owners.
fn owner_list(owners: &[String]) -> String {
    owners
        .iter()
        .map(|s| literal(s))
        .collect::<Vec<_>>()
        .join(", ")
}

/// `i.part1, …, i.part16`: an index's column numbers (negative for a descending column, 0 past
/// the last).
fn parts(alias: &str) -> String {
    (1..=16)
        .map(|n| format!("{alias}.part{n}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Only SPL routines a user wrote: built-in and external (C, Java) routines have no SPL text.
const SPL: &str = "p.internal = 'f' AND p.langid IN \
     (SELECT l.langid FROM sysroutinelangs l WHERE LOWER(l.langname) = 'spl')";

/// The catalog queries for `owners`, by name. Each one's columns are documented where it is read
/// in [`normalize`].
pub fn queries(owners: &[String]) -> Vec<(&'static str, String)> {
    let list = owner_list(owners);
    vec![
        (
            // The declared widths of the text pieces below: (table, column, width).
            "piece_widths",
            "SELECT TRIM(t.tabname), TRIM(c.colname), c.collength \
             FROM syscolumns c JOIN systables t ON t.tabid = c.tabid \
             WHERE t.tabid < 100 AND ((t.tabname = 'sysviews' AND c.colname = 'viewtext') \
             OR (t.tabname = 'syschecks' AND c.colname = 'checktext') \
             OR (t.tabname IN ('sysprocbody', 'systrigbody') AND c.colname = 'data')) \
             ORDER BY 1"
                .to_string(),
        ),
        (
            "census_tables",
            format!(
                "SELECT TRIM(owner), tabtype, COUNT(*) FROM systables \
                 WHERE tabid >= 100 AND owner IN ({list}) GROUP BY 1, 2 ORDER BY 1, 2"
            ),
        ),
        (
            "census_routines",
            format!(
                "SELECT TRIM(p.owner), p.isproc, \
                 CASE WHEN {SPL} THEN 'spl' ELSE 'external' END, COUNT(*) \
                 FROM sysprocedures p WHERE p.owner IN ({list}) AND p.internal = 'f' \
                 GROUP BY 1, 2, 3 ORDER BY 1, 2, 3"
            ),
        ),
        (
            "census_triggers",
            format!(
                "SELECT TRIM(owner), COUNT(*) FROM systriggers WHERE owner IN ({list}) \
                 GROUP BY 1 ORDER BY 1"
            ),
        ),
        (
            "census_indexes",
            format!(
                "SELECT TRIM(t.owner), COUNT(*) FROM sysindexes i, systables t \
                 WHERE t.tabid = i.tabid AND t.tabid >= 100 AND t.owner IN ({list}) \
                 GROUP BY 1 ORDER BY 1"
            ),
        ),
        (
            "census_constraints",
            format!(
                "SELECT TRIM(t.owner), COUNT(*) FROM sysconstraints c, systables t \
                 WHERE t.tabid = c.tabid AND t.tabid >= 100 AND t.owner IN ({list}) \
                 AND c.constrtype IN ('P', 'U', 'R', 'C') GROUP BY 1 ORDER BY 1"
            ),
        ),
        (
            "tables",
            format!(
                "SELECT TRIM(owner), TRIM(tabname), tabtype FROM systables \
                 WHERE tabid >= 100 AND tabtype IN ('T', 'V') AND owner IN ({list}) \
                 ORDER BY 1, 2"
            ),
        ),
        (
            "columns",
            format!(
                "SELECT TRIM(t.owner), TRIM(t.tabname), TRIM(c.colname), c.coltype, c.collength, \
                 TRIM(x.name), d.type, d.default, c.colno \
                 FROM systables t JOIN syscolumns c ON c.tabid = t.tabid \
                 LEFT JOIN sysxtdtypes x ON x.extended_id = c.extended_id AND c.extended_id > 0 \
                 LEFT JOIN sysdefaults d ON d.tabid = c.tabid AND d.colno = c.colno \
                 WHERE t.tabid >= 100 AND t.tabtype IN ('T', 'V') AND t.owner IN ({list}) \
                 ORDER BY 1, 2, 9"
            ),
        ),
        (
            "keys",
            format!(
                "SELECT TRIM(t.owner), TRIM(t.tabname), TRIM(c.constrname), c.constrtype, {} \
                 FROM sysconstraints c JOIN systables t ON t.tabid = c.tabid \
                 JOIN sysindexes i ON i.tabid = c.tabid AND i.idxname = c.idxname \
                 WHERE c.constrtype IN ('P', 'U') AND t.owner IN ({list}) ORDER BY 1, 2, 3",
                parts("i")
            ),
        ),
        (
            "unique_indexes",
            format!(
                "SELECT TRIM(t.owner), TRIM(t.tabname), TRIM(i.idxname), 'X', {} \
                 FROM sysindexes i JOIN systables t ON t.tabid = i.tabid \
                 WHERE i.idxtype = 'U' AND t.tabid >= 100 AND t.tabtype = 'T' \
                 AND t.owner IN ({list}) AND NOT EXISTS (SELECT 1 FROM sysconstraints k \
                 WHERE k.tabid = i.tabid AND k.idxname = i.idxname) ORDER BY 1, 2, 3",
                parts("i")
            ),
        ),
        (
            "checks",
            format!(
                "SELECT TRIM(t.owner), TRIM(t.tabname), TRIM(c.constrname), k.seqno, k.checktext \
                 FROM sysconstraints c JOIN systables t ON t.tabid = c.tabid \
                 JOIN syschecks k ON k.constrid = c.constrid \
                 WHERE c.constrtype = 'C' AND k.type = 'T' AND t.owner IN ({list}) \
                 ORDER BY 1, 2, 3, 4"
            ),
        ),
        (
            "foreign_keys",
            format!(
                "SELECT TRIM(t.owner), TRIM(t.tabname), TRIM(c.constrname), TRIM(pt.owner), \
                 TRIM(pt.tabname), r.delrule, {}, {} \
                 FROM sysconstraints c JOIN systables t ON t.tabid = c.tabid \
                 JOIN sysreferences r ON r.constrid = c.constrid \
                 JOIN sysconstraints pc ON pc.constrid = r.primary \
                 JOIN systables pt ON pt.tabid = r.ptabid \
                 JOIN sysindexes ci ON ci.tabid = c.tabid AND ci.idxname = c.idxname \
                 JOIN sysindexes px ON px.tabid = pc.tabid AND px.idxname = pc.idxname \
                 WHERE c.constrtype = 'R' AND t.owner IN ({list}) ORDER BY 1, 2, 3",
                parts("ci"),
                parts("px")
            ),
        ),
        (
            // The column names of tables a listed foreign key references, which may belong to an
            // owner that is not listed.
            "referenced_columns",
            format!(
                "SELECT TRIM(t.owner), TRIM(t.tabname), c.colno, TRIM(c.colname) \
                 FROM syscolumns c JOIN systables t ON t.tabid = c.tabid \
                 WHERE t.tabid IN (SELECT r.ptabid FROM sysreferences r \
                 JOIN sysconstraints k ON k.constrid = r.constrid \
                 JOIN systables kt ON kt.tabid = k.tabid WHERE kt.owner IN ({list})) \
                 ORDER BY 1, 2, 3"
            ),
        ),
        (
            "views",
            format!(
                "SELECT TRIM(t.owner), TRIM(t.tabname), v.seqno, v.viewtext \
                 FROM sysviews v JOIN systables t ON t.tabid = v.tabid \
                 WHERE t.owner IN ({list}) ORDER BY 1, 2, 3"
            ),
        ),
        (
            "routines",
            format!(
                "SELECT TRIM(p.owner), TRIM(p.procname), p.procid, p.isproc, p.numargs \
                 FROM sysprocedures p WHERE p.owner IN ({list}) AND {SPL} ORDER BY 1, 2, 3"
            ),
        ),
        (
            "routine_text",
            format!(
                "SELECT b.procid, b.datakey, b.seqno, b.data FROM sysprocbody b \
                 JOIN sysprocedures p ON p.procid = b.procid \
                 WHERE p.owner IN ({list}) AND {SPL} AND b.datakey IN ('T', 'D') \
                 ORDER BY 1, 2, 3"
            ),
        ),
        (
            "triggers",
            format!(
                "SELECT TRIM(r.owner), TRIM(r.trigname), r.trigid, TRIM(t.owner), \
                 TRIM(t.tabname), r.event FROM systriggers r JOIN systables t ON t.tabid = r.tabid \
                 WHERE r.owner IN ({list}) ORDER BY 1, 2"
            ),
        ),
        (
            "trigger_text",
            format!(
                "SELECT b.trigid, b.datakey, b.seqno, b.data FROM systrigbody b \
                 JOIN systriggers r ON r.trigid = b.trigid \
                 WHERE r.owner IN ({list}) AND b.datakey IN ('D', 'A') ORDER BY 1, 2 DESC, 3"
            ),
        ),
    ]
}

/// RFC 0211 §3.4 P0: `systables.nrows`, which `UPDATE STATISTICS` maintains — an estimate
/// (`row_count_exact: false`), and 0 for a table never analysed.
pub fn row_estimates(owners: &[String]) -> String {
    format!(
        "SELECT TRIM(owner), TRIM(tabname), nrows FROM systables \
         WHERE tabid >= 100 AND tabtype = 'T' AND owner IN ({}) ORDER BY 1, 2",
        owner_list(owners)
    )
}

/// Who the session's user counts as when a grant is checked: itself, `public`, and its roles.
fn grantee(alias: &str) -> String {
    format!(
        "({alias}.grantee = USER OR {alias}.grantee = 'public' OR {alias}.grantee IN \
         (SELECT ra.rolename FROM sysroleauth ra WHERE ra.grantee = USER))"
    )
}

/// The write check (RFC 0211's `write_check`). Informix has no read-only login, so the user's
/// rights are asked for instead of a write attempted. One row: (tables of the listed owners it
/// owns or may `UPDATE` / `INSERT` / `DELETE` / `ALTER`, by table or column grant, directly, as
/// `public` or through a role; database-level `RESOURCE` or `DBA` grants — either may create
/// objects; `DBA` grants).
///
/// `systabauth.tabauth` is nine flags: select, update, column-level, insert, delete, index, alter,
/// references, under — a letter where granted, `-` where not.
pub fn write_check(owners: &[String]) -> String {
    let list = owner_list(owners);
    let flag =
        |pos: usize, letter: char| format!("UPPER(SUBSTR(a.tabauth, {pos}, 1)) = '{letter}'");
    let table_rights = [flag(2, 'U'), flag(4, 'I'), flag(5, 'D'), flag(7, 'A')].join(" OR ");
    let (tab_grantee, col_grantee) = (grantee("a"), grantee("ca"));
    format!(
        "SELECT \
         (SELECT COUNT(*) FROM systables t WHERE t.tabid >= 100 AND t.tabtype = 'T' \
          AND t.owner IN ({list}) AND (t.owner = USER \
          OR EXISTS (SELECT 1 FROM systabauth a WHERE a.tabid = t.tabid AND {tab_grantee} \
                     AND ({table_rights})) \
          OR EXISTS (SELECT 1 FROM syscolauth ca WHERE ca.tabid = t.tabid AND {col_grantee} \
                     AND UPPER(SUBSTR(ca.colauth, 2, 1)) = 'U'))), \
         (SELECT COUNT(*) FROM sysusers u WHERE (u.username = USER OR u.username = 'public') \
          AND u.usertype IN ('D', 'R')), \
         (SELECT COUNT(*) FROM sysusers u WHERE (u.username = USER OR u.username = 'public') \
          AND u.usertype = 'D') \
         FROM systables WHERE tabid = 1"
    )
}

/// The write check's answer in RFC 0211's shape: refused unless the user may change something.
/// `guards` are the session's own locks, reported beside it.
pub fn write_check_result(row: &[Option<String>], guards: &Guards) -> Value {
    let n = |i: usize| {
        row.get(i)
            .and_then(Option::as_deref)
            .and_then(|v| v.trim().parse::<f64>().ok())
            .map_or(0, |f| f as i64)
    };
    let result = if n(0) == 0 && n(1) == 0 {
        Err(
            "the user owns no listed table, holds no UPDATE, INSERT, DELETE or ALTER grant on one \
             (directly, as public or through a role), and has no RESOURCE or DBA privilege \
             (systabauth, syscolauth, sysusers)"
                .to_string(),
        )
    } else {
        Ok(())
    };
    let mut v = ekos_observation_sdk::live_catalog::write_check(result);
    v["method"] = "permissions".into();
    v["writable_tables"] = n(0).into();
    v["can_create_objects"] = (n(1) > 0).into();
    v["dba"] = (n(2) > 0).into();
    v["session"] = json!({
        "transactions_rolled_back": guards.transactions,
        "read_only_transactions": guards.read_only_transactions,
        "isolation": guards.isolation,
        "delimited_identifiers": guards.delimited_identifiers,
    });
    if !guards.transactions {
        v["note"] = "the database has no transaction logging, so statements cannot be rolled \
                     back; only the planned text and the user's rights keep this session read-only"
            .into();
    }
    v
}

/// What the catalog queries returned, by query name.
pub type CatalogRows = BTreeMap<String, Rows>;

fn cell(row: &[Option<String>], i: usize) -> Option<&str> {
    row.get(i).and_then(Option::as_deref)
}

fn text(row: &[Option<String>], i: usize) -> String {
    cell(row, i).unwrap_or_default().trim().to_string()
}

fn int(row: &[Option<String>], i: usize) -> Option<i64> {
    cell(row, i).and_then(|v| {
        let v = v.trim();
        v.parse::<i64>()
            .ok()
            .or_else(|| v.parse::<f64>().ok().map(|f| f as i64))
    })
}

/// Fixed-width pieces, in order, as one text: each piece but the last padded back to `width`
/// bytes, the whole trimmed at the end.
pub fn join_pieces<'a>(pieces: impl IntoIterator<Item = &'a str>, width: usize) -> String {
    let mut out = String::new();
    for p in pieces {
        out.push_str(p);
        let short = width.saturating_sub(p.len());
        out.extend(std::iter::repeat_n(' ', short));
    }
    out.trim_end().to_string()
}

/// A `DATETIME` / `INTERVAL` qualifier field: 0 year … 10 second, 11–15 fraction(1–5).
fn field(code: i64) -> String {
    match code {
        0 => "year".into(),
        2 => "month".into(),
        4 => "day".into(),
        6 => "hour".into(),
        8 => "minute".into(),
        10 => "second".into(),
        11..=15 => format!("fraction({})", code - 10),
        _ => format!("field{code}"),
    }
}

/// Digits a field takes after the leading one.
fn digits(code: i64) -> i64 {
    match code {
        11..=15 => code - 10,
        _ => 2,
    }
}

/// `collength` of a `DATETIME` / `INTERVAL`: (digits, first field, last field).
fn qualifier(collength: i64) -> (i64, i64, i64) {
    let c = collength & 0xFFFF;
    (c >> 8, (c & 0xF0) >> 4, c & 0x0F)
}

/// A column's type as Informix writes it, from `syscolumns.coltype` / `collength` and, for an
/// extended type (`coltype` 40 or 41), its `sysxtdtypes.name`. The bit 0x100 of `coltype` is
/// `NOT NULL`. Returns (type, nullable, serial kind).
pub fn type_text(
    coltype: i64,
    collength: i64,
    xtd_name: Option<&str>,
) -> (String, bool, Option<&'static str>) {
    let nullable = coltype & 0x100 == 0;
    let base = coltype & 0xFF;
    let len = collength & 0xFFFF;
    let precision_scale = |name: &str| {
        let (p, s) = (len >> 8, len & 0xFF);
        if s == 0xFF {
            format!("{name}({p})")
        } else {
            format!("{name}({p},{s})")
        }
    };
    let var = |name: &str| {
        let (max, min) = (len & 0xFF, len >> 8);
        if min > 0 {
            format!("{name}({max},{min})")
        } else {
            format!("{name}({max})")
        }
    };
    let mut serial = None;
    let t = match base {
        0 => format!("char({len})"),
        1 => "smallint".into(),
        2 => "integer".into(),
        3 => "float".into(),
        4 => "smallfloat".into(),
        5 => precision_scale("decimal"),
        6 => {
            serial = Some("SERIAL");
            "serial".into()
        }
        7 => "date".into(),
        8 => precision_scale("money"),
        10 => {
            let (_, first, last) = qualifier(len);
            format!("datetime {} to {}", field(first), field(last))
        }
        11 => "byte".into(),
        12 => "text".into(),
        13 => var("varchar"),
        14 => {
            let (n, first, last) = qualifier(len);
            let rest: i64 = (first..=last)
                .skip(1)
                .filter(|f| (*f <= 10 && *f % 2 == 0) || (*f == last && *f > 10))
                .map(digits)
                .sum();
            let lead = if first > 10 {
                field(first)
            } else {
                format!("{}({})", field(first), n - rest)
            };
            if first == last {
                format!("interval {lead}")
            } else {
                format!("interval {lead} to {}", field(last))
            }
        }
        15 => format!("nchar({len})"),
        16 => var("nvarchar"),
        17 => "int8".into(),
        18 => {
            serial = Some("SERIAL8");
            "serial8".into()
        }
        19 => "set".into(),
        20 => "multiset".into(),
        21 => "list".into(),
        22 => "row".into(),
        43 => format!("lvarchar({len})"),
        45 => "boolean".into(),
        52 => "bigint".into(),
        53 => {
            serial = Some("BIGSERIAL");
            "bigserial".into()
        }
        40 | 41 => match xtd_name.map(str::to_ascii_lowercase).as_deref() {
            Some("lvarchar") => format!("lvarchar({len})"),
            Some(n) if !n.is_empty() => n.to_string(),
            _ => format!("opaque({base})"),
        },
        _ => match xtd_name.filter(|n| !n.is_empty()) {
            Some(n) => n.to_ascii_lowercase(),
            None => format!("type({base})"),
        },
    };
    (t, nullable, serial)
}

/// A `sysdefaults` row as SQL: a literal's text, or the keyword it stands for.
///
/// A numeric literal is stored as an internal encoding, a blank, then the text (`AAE 1` for `1`):
/// only the text after the last blank is the value.
fn default_text(kind: &str, value: Option<&str>, data_type: &str) -> Option<String> {
    let numeric = ekos_observation_sdk::profile::family(
        ekos_observation_sdk::profile::Flavor::Informix,
        data_type,
    ) == ekos_observation_sdk::profile::Family::Number;
    Some(match kind.trim() {
        "L" if numeric => value?.split_whitespace().last()?.to_string(),
        "L" => value?.trim().to_string(),
        "U" => "USER".into(),
        "C" => "CURRENT".into(),
        "N" => "NULL".into(),
        "T" => "TODAY".into(),
        "S" => "DBSERVERNAME".into(),
        _ => return None,
    })
}

/// The census type of a `systables.tabtype`.
fn table_census(code: &str) -> &'static str {
    match code.trim() {
        "T" => "TABLE",
        "V" => "VIEW",
        "S" | "P" => "SYNONYM",
        "E" => "EXTERNAL_TABLE",
        "Q" => "SEQUENCE",
        _ => "OTHER",
    }
}

/// The census type of a routine: `isproc` (`t`/`f`) and whether it is SPL.
fn routine_census(isproc: &str, language: &str) -> &'static str {
    match (language.trim(), isproc.trim()) {
        ("spl", "t") => "PROCEDURE",
        ("spl", _) => "FUNCTION",
        _ => "EXTERNAL_ROUTINE",
    }
}

/// A trigger's event code: upper case on a table, lower case for an `INSTEAD OF` trigger on a view.
fn trigger_event(code: &str) -> (String, bool) {
    let c = code.trim();
    let event = match c.to_ascii_uppercase().as_str() {
        "I" => "INSERT",
        "U" => "UPDATE",
        "D" => "DELETE",
        "S" => "SELECT",
        _ => "OTHER",
    };
    (event.into(), c.chars().any(|ch| ch.is_ascii_lowercase()))
}

/// `NO ACTION` unless the reference cascades.
fn on_delete(rule: &str) -> &'static str {
    match rule.trim() {
        "C" => "CASCADE",
        _ => "NO ACTION",
    }
}

/// A check constraint's text without the outer parentheses Informix keeps around it.
pub fn check_expression(definition: &str) -> String {
    let d = definition.trim();
    match d.strip_prefix('(').and_then(|d| d.strip_suffix(')')) {
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
/// (owner, table) → column number → name.
type ColumnNames = BTreeMap<Key, BTreeMap<i64, String>>;

/// Index parts as column names, in key order (a descending part is negative).
fn key_columns(
    row: &[Option<String>],
    from: usize,
    names: Option<&BTreeMap<i64, String>>,
) -> Vec<String> {
    (from..from + 16)
        .filter_map(|i| int(row, i))
        .filter(|n| *n != 0)
        .map(|n| {
            names
                .and_then(|m| m.get(&n.abs()).cloned())
                .unwrap_or_else(|| format!("#{}", n.abs()))
        })
        .collect()
}

/// The declared widths of the text pieces, from `piece_widths`, else the defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PieceWidths {
    pub view: usize,
    pub check: usize,
    pub routine: usize,
    pub trigger: usize,
}

impl PieceWidths {
    /// `piece_widths` rows: (table, column, width).
    pub fn from_rows(rows: &Rows) -> Self {
        let mut w = PieceWidths {
            view: VIEW_PIECE,
            check: CHECK_PIECE,
            routine: BODY_PIECE,
            trigger: BODY_PIECE,
        };
        for row in rows {
            let Some(n) = int(row, 2).filter(|n| *n > 0).map(|n| n as usize) else {
                continue;
            };
            match text(row, 0).as_str() {
                "sysviews" => w.view = n,
                "syschecks" => w.check = n,
                "sysprocbody" => w.routine = n,
                "systrigbody" => w.trigger = n,
                _ => {}
            }
        }
        w
    }
}

fn new_catalog(owner: &str) -> OwnerCatalog {
    OwnerCatalog {
        owner: owner.to_string(),
        family: FAMILY.into(),
        ..Default::default()
    }
}

/// The rows of every catalog query as RFC 0211's per-owner objects. Pure. A listed owner with
/// nothing in it still gets a catalog, so the census says "nothing there".
pub fn normalize(
    owners: &[String],
    rows: &CatalogRows,
    redaction: &RedactionConfig,
) -> Vec<OwnerCatalog> {
    let empty = Rows::new();
    let get = |name: &str| rows.get(name).unwrap_or(&empty);
    let mut cats: BTreeMap<String, OwnerCatalog> =
        owners.iter().map(|s| (s.clone(), new_catalog(s))).collect();
    census(&mut cats, rows);
    // columns: owner, relation, name, coltype, collength, xtd name, default type, default, colno
    let mut names: ColumnNames = BTreeMap::new();
    let mut columns: BTreeMap<Key, Vec<Value>> = BTreeMap::new();
    for row in get("columns") {
        let (owner, rel) = (text(row, 0), text(row, 1));
        let Some(c) = cats.get_mut(&owner) else {
            continue;
        };
        c.columns += 1;
        let (data_type, nullable, serial) = type_text(
            int(row, 3).unwrap_or(0),
            int(row, 4).unwrap_or(0),
            cell(row, 5).map(str::trim),
        );
        let mut col = json!({ "name": text(row, 2), "data_type": data_type, "nullable": nullable });
        if let Some(s) = serial {
            col["identity"] = s.into();
        }
        if let Some(d) = cell(row, 6).and_then(|k| default_text(k, cell(row, 7), &data_type)) {
            col["default"] = redact(&d, redaction).into();
        }
        if let Some(n) = int(row, 8) {
            names
                .entry((owner.clone(), rel.clone()))
                .or_default()
                .insert(n, text(row, 2));
        }
        columns.entry((owner, rel)).or_default().push(col);
    }
    for row in get("referenced_columns") {
        if let Some(n) = int(row, 2) {
            names
                .entry((text(row, 0), text(row, 1)))
                .or_default()
                .entry(n)
                .or_insert_with(|| text(row, 3));
        }
    }
    // tables: owner, name, tabtype (views are listed from their text below)
    for row in get("tables") {
        let (owner, name) = (text(row, 0), text(row, 1));
        let (Some(c), "T") = (cats.get_mut(&owner), text(row, 2).as_str()) else {
            continue;
        };
        let cols = columns
            .remove(&(owner.clone(), name.clone()))
            .unwrap_or_default();
        c.objects.push(LiveObject {
            kind: "table".into(),
            owner,
            name,
            status: None,
            detail: json!({ "columns": cols }),
        });
    }
    add_keys(&mut cats, get("keys"), &names);
    add_keys(&mut cats, get("unique_indexes"), &names);
    let widths = PieceWidths::from_rows(get("piece_widths"));
    add_checks(&mut cats, get("checks"), widths.check, redaction);
    add_foreign_keys(&mut cats, get("foreign_keys"), &names);
    add_views(&mut cats, get("views"), widths.view, redaction);
    add_routines(&mut cats, rows, widths.routine, redaction);
    add_triggers(&mut cats, rows, widths.trigger, redaction);
    cats.into_values().collect()
}

fn census(cats: &mut BTreeMap<String, OwnerCatalog>, rows: &CatalogRows) {
    let empty = Rows::new();
    let get = |name: &str| rows.get(name).unwrap_or(&empty);
    let mut add = |owner: String, kind: &str, n: Option<i64>| {
        if let (Some(c), Some(n)) = (cats.get_mut(&owner), n) {
            c.census.entry(kind.to_string()).or_default().valid += n as usize;
        }
    };
    // census_tables: owner, tabtype, count
    for row in get("census_tables") {
        add(text(row, 0), table_census(&text(row, 1)), int(row, 2));
    }
    // census_routines: owner, isproc, spl|external, count
    for row in get("census_routines") {
        add(
            text(row, 0),
            routine_census(&text(row, 1), &text(row, 2)),
            int(row, 3),
        );
    }
    for (query, kind) in [
        ("census_triggers", "TRIGGER"),
        ("census_indexes", "INDEX"),
        ("census_constraints", "CONSTRAINT"),
    ] {
        for row in get(query) {
            add(text(row, 0), kind, int(row, 1));
        }
    }
}

fn add_keys(cats: &mut BTreeMap<String, OwnerCatalog>, rows: &Rows, names: &ColumnNames) {
    // owner, table, name, P|U|X (X: a unique index no constraint stands on), part1 … part16
    for row in rows {
        let (owner, table) = (text(row, 0), text(row, 1));
        let Some(c) = cats.get_mut(&owner) else {
            continue;
        };
        let kind = text(row, 3);
        let cols = key_columns(row, 4, names.get(&(owner.clone(), table.clone())));
        let mut detail = json!({ "table": table, "columns": cols, "validated": true });
        if kind == "X" {
            // A unique index, not a declared constraint: unique all the same.
            detail["index"] = true.into();
        }
        c.objects.push(LiveObject {
            kind: if kind == "P" {
                "primary_key"
            } else {
                "unique_constraint"
            }
            .into(),
            owner,
            name: format!("{table}.{}", text(row, 2)),
            status: None,
            detail,
        });
    }
}

fn add_checks(
    cats: &mut BTreeMap<String, OwnerCatalog>,
    rows: &Rows,
    width: usize,
    redaction: &RedactionConfig,
) {
    // owner, table, name, seqno, piece
    let mut pieces: BTreeMap<(String, String, String), Vec<String>> = BTreeMap::new();
    for row in rows {
        pieces
            .entry((text(row, 0), text(row, 1), text(row, 2)))
            .or_default()
            .push(cell(row, 4).unwrap_or_default().to_string());
    }
    for ((owner, table, name), ps) in pieces {
        let Some(c) = cats.get_mut(&owner) else {
            continue;
        };
        let condition = check_expression(&join_pieces(ps.iter().map(String::as_str), width));
        c.objects.push(LiveObject {
            kind: "check_constraint".into(),
            owner,
            name: format!("{table}.{name}"),
            status: None,
            detail: json!({
                "table": table,
                "columns": [],
                "validated": true,
                "condition": redact(&condition, redaction),
            }),
        });
    }
}

fn add_foreign_keys(cats: &mut BTreeMap<String, OwnerCatalog>, rows: &Rows, names: &ColumnNames) {
    // owner, table, name, ref owner, ref table, delete rule, child part1…16, parent part1…16
    for row in rows {
        let (owner, table) = (text(row, 0), text(row, 1));
        let Some(c) = cats.get_mut(&owner) else {
            continue;
        };
        let (ref_owner, ref_table) = (text(row, 3), text(row, 4));
        let child = key_columns(row, 6, names.get(&(owner.clone(), table.clone())));
        let parent = key_columns(row, 22, names.get(&(ref_owner.clone(), ref_table.clone())));
        c.objects.push(LiveObject {
            kind: "foreign_key".into(),
            owner,
            name: format!("{table}.{}", text(row, 2)),
            status: None,
            detail: json!({
                "table": table,
                "columns": child,
                "references_owner": ref_owner,
                "references_table": ref_table,
                "references_columns": parent,
                "validated": true,
                "on_delete": on_delete(&text(row, 5)),
            }),
        });
    }
}

/// Pieces grouped by an object key, in the order the query sorted them.
fn grouped<K: Ord>(
    rows: &Rows,
    key: impl Fn(&[Option<String>]) -> K,
    piece: usize,
) -> BTreeMap<K, Vec<String>> {
    let mut out: BTreeMap<K, Vec<String>> = BTreeMap::new();
    for row in rows {
        out.entry(key(row))
            .or_default()
            .push(cell(row, piece).unwrap_or_default().to_string());
    }
    out
}

fn add_views(
    cats: &mut BTreeMap<String, OwnerCatalog>,
    rows: &Rows,
    width: usize,
    redaction: &RedactionConfig,
) {
    // owner, name, seqno, piece
    for ((owner, name), ps) in grouped(rows, |r| (text(r, 0), text(r, 1)), 3) {
        let Some(c) = cats.get_mut(&owner) else {
            continue;
        };
        let sql = join_pieces(ps.iter().map(String::as_str), width);
        c.objects.push(LiveObject {
            kind: "view".into(),
            owner,
            name,
            status: None,
            detail: json!({ "text": redact(&sql, redaction), "language": "informix" }),
        });
    }
}

fn add_routines(
    cats: &mut BTreeMap<String, OwnerCatalog>,
    rows: &CatalogRows,
    width: usize,
    redaction: &RedactionConfig,
) {
    let empty = Rows::new();
    // routine_text: procid, T|D, seqno, piece
    let texts = grouped(
        rows.get("routine_text").unwrap_or(&empty),
        |r| (int(r, 0).unwrap_or(-1), text(r, 1)),
        3,
    );
    let routines = rows.get("routines").unwrap_or(&empty);
    // Overloads share a name; each gets its `procid` so the targets differ.
    let mut seen: BTreeMap<Key, usize> = BTreeMap::new();
    for row in routines {
        *seen.entry((text(row, 0), text(row, 1))).or_default() += 1;
    }
    // routines: owner, name, procid, isproc, numargs
    for row in routines {
        let (owner, name) = (text(row, 0), text(row, 1));
        let procid = int(row, 2).unwrap_or(-1);
        let overloaded = seen
            .get(&(owner.clone(), name.clone()))
            .is_some_and(|n| *n > 1);
        let Some(c) = cats.get_mut(&owner) else {
            continue;
        };
        let body = |key: &str| {
            texts
                .get(&(procid, key.to_string()))
                .map(|ps| join_pieces(ps.iter().map(String::as_str), width))
        };
        let sql = body("T").unwrap_or_default();
        let mut detail = json!({
            "text": redact(&sql, redaction),
            "language": "spl",
            "arguments_count": int(row, 4).unwrap_or(0),
        });
        if sql.is_empty() {
            detail["text_missing"] = true.into();
        }
        if let Some(doc) = body("D").filter(|d| !d.is_empty()) {
            detail["comment"] = redact(&doc, redaction).into();
        }
        c.objects.push(LiveObject {
            kind: if text(row, 3) == "t" {
                "procedure"
            } else {
                "function"
            }
            .into(),
            owner,
            name: if overloaded {
                format!("{name}#{procid}")
            } else {
                name
            },
            status: None,
            detail,
        });
    }
}

fn add_triggers(
    cats: &mut BTreeMap<String, OwnerCatalog>,
    rows: &CatalogRows,
    width: usize,
    redaction: &RedactionConfig,
) {
    let empty = Rows::new();
    // trigger_text: trigid, D|A, seqno, piece (the header, then the action)
    let texts = grouped(
        rows.get("trigger_text").unwrap_or(&empty),
        |r| (int(r, 0).unwrap_or(-1), text(r, 1)),
        3,
    );
    // triggers: owner, name, trigid, table owner, table, event
    for row in rows.get("triggers").unwrap_or(&empty) {
        let owner = text(row, 0);
        let Some(c) = cats.get_mut(&owner) else {
            continue;
        };
        let trigid = int(row, 2).unwrap_or(-1);
        // The header (`D`) and the action (`A`) are separate texts: joined by a line break.
        let part = |key: &str| {
            texts
                .get(&(trigid, key.to_string()))
                .map_or_else(String::new, |ps| {
                    join_pieces(ps.iter().map(String::as_str), width)
                })
        };
        let sql = format!("{}\n{}", part("D"), part("A").trim_start());
        let (event, instead_of) = trigger_event(&text(row, 5));
        c.objects.push(LiveObject {
            kind: "trigger".into(),
            owner,
            name: text(row, 1),
            status: None,
            detail: json!({
                "text": redact(&sql, redaction),
                "action": redact(part("A").trim(), redaction),
                "language": "informix",
                "table": text(row, 4),
                "table_owner": text(row, 3),
                "events": [event],
                "timing": if instead_of { "INSTEAD OF" } else { "AFTER" },
            }),
        });
    }
}

/// RFC 0157's rule for live catalogs: a catalog shorter than the server's own count is an error,
/// never a smaller model. Tables, views, SPL routines and triggers read must equal the census.
pub fn reconcile(catalogs: &[OwnerCatalog]) -> Result<(), String> {
    for c in catalogs {
        let listed = c.listed();
        for (census, kind) in [
            ("TABLE", "table"),
            ("VIEW", "view"),
            ("PROCEDURE", "procedure"),
            ("FUNCTION", "function"),
            ("TRIGGER", "trigger"),
        ] {
            let counted = c.census.get(census).map_or(0, |n| n.valid);
            let read = listed.get(kind).copied().unwrap_or(0);
            if counted != read {
                return Err(format!(
                    "owner {}: the catalog counts {counted} {census} object(s) but the read found \
                     {read}; refusing a partial catalog (does the user see every object?)",
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

    /// `owner, table, name, kind` and sixteen index parts.
    fn key_row(head: &[&str], parts: &[&str]) -> Vec<Option<String>> {
        let mut r = row(head);
        for i in 0..16 {
            r.push(Some(parts.get(i).copied().unwrap_or("0").to_string()));
        }
        r
    }

    pub(crate) fn fixture() -> CatalogRows {
        let mut m = CatalogRows::new();
        m.insert(
            "census_tables".into(),
            vec![
                row(&["sales", "T", "2"]),
                row(&["sales", "V", "1"]),
                row(&["sales", "Q", "1"]),
            ],
        );
        m.insert(
            "census_routines".into(),
            vec![
                row(&["sales", "f", "spl", "1"]),
                row(&["sales", "t", "spl", "1"]),
                row(&["sales", "f", "external", "2"]),
            ],
        );
        m.insert("census_triggers".into(), vec![row(&["sales", "1"])]);
        m.insert("census_indexes".into(), vec![row(&["sales", "4"])]);
        m.insert("census_constraints".into(), vec![row(&["sales", "5"])]);
        m.insert(
            "tables".into(),
            vec![
                row(&["sales", "big_orders", "V"]),
                row(&["sales", "customers", "T"]),
                row(&["sales", "orders", "T"]),
            ],
        );
        m.insert(
            "columns".into(),
            vec![
                // serial NOT NULL (6 | 0x100)
                row(&[
                    "sales",
                    "customers",
                    "id",
                    "262",
                    "4",
                    "NULL",
                    "NULL",
                    "NULL",
                    "1",
                ]),
                row(&[
                    "sales",
                    "customers",
                    "name",
                    "269",
                    "100",
                    "NULL",
                    "NULL",
                    "NULL",
                    "2",
                ]),
                // datetime year to second, default CURRENT
                row(&[
                    "sales",
                    "customers",
                    "created_at",
                    "266",
                    "3594",
                    "NULL",
                    "C",
                    "NULL",
                    "3",
                ]),
                // serial8 NOT NULL
                row(&[
                    "sales", "orders", "id", "274", "8", "NULL", "NULL", "NULL", "1",
                ]),
                row(&[
                    "sales",
                    "orders",
                    "customer_id",
                    "2",
                    "4",
                    "NULL",
                    "NULL",
                    "NULL",
                    "2",
                ]),
                // decimal(12,2) NOT NULL default 0
                row(&[
                    "sales", "orders", "total", "261", "3074", "NULL", "L", "AAE 0.00", "3",
                ]),
                // lvarchar(2000) through sysxtdtypes
                row(&[
                    "sales", "orders", "note", "40", "2000", "lvarchar", "NULL", "NULL", "4",
                ]),
                row(&[
                    "sales",
                    "big_orders",
                    "id",
                    "17",
                    "8",
                    "NULL",
                    "NULL",
                    "NULL",
                    "1",
                ]),
            ],
        );
        m.insert(
            "keys".into(),
            vec![
                key_row(&["sales", "customers", "pk_customers", "P"], &["1"]),
                key_row(&["sales", "orders", "pk_orders", "P"], &["1"]),
            ],
        );
        m.insert(
            "unique_indexes".into(),
            vec![key_row(
                &["sales", "orders", "ux_orders_ref", "X"],
                &["2", "-1"],
            )],
        );
        m.insert(
            "checks".into(),
            vec![row(&[
                "sales",
                "orders",
                "ck_orders_total",
                "0",
                "(total >= 0.00 )               ",
            ])],
        );
        let mut fk = row(&[
            "sales",
            "orders",
            "fk_orders_customers",
            "sales",
            "customers",
            "C",
        ]);
        for i in 0..32 {
            fk.push(Some(
                if i == 0 {
                    "2"
                } else if i == 16 {
                    "1"
                } else {
                    "0"
                }
                .into(),
            ));
        }
        m.insert("foreign_keys".into(), vec![fk]);
        m.insert(
            "referenced_columns".into(),
            vec![row(&["sales", "customers", "1", "id"])],
        );
        let view = "create view \"sales\".big_orders (id) as select x.id from \"sales\".orders x \
                    where (x.total > 100.00 ) ;";
        m.insert(
            "views".into(),
            view.as_bytes()
                .chunks(VIEW_PIECE)
                .enumerate()
                .map(|(i, p)| {
                    // A driver that trims `CHAR` values drops a piece's trailing blanks.
                    let piece = std::str::from_utf8(p).unwrap().trim_end();
                    row(&["sales", "big_orders", &i.to_string(), piece])
                })
                .collect(),
        );
        m.insert(
            "routines".into(),
            vec![
                row(&["sales", "close_order", "301", "t", "1"]),
                row(&["sales", "order_total", "302", "f", "1"]),
            ],
        );
        m.insert(
            "routine_text".into(),
            vec![
                row(&["301", "D", "0", "Closes one order"]),
                row(&["301", "T", "0", "create procedure \"sales\".close_order(p_id int8)\n  update \"sales\".orders set note = 'closed' where id = p_id;\nend procedure"]),
                row(&["302", "T", "0", "create function \"sales\".order_total(p_id int8) returning decimal(12,2);\n  define t decimal(12,2);\n  select total into t from \"sales\".orders where id = p_id;\n  return t;\nend function"]),
            ],
        );
        m.insert(
            "triggers".into(),
            vec![row(&["sales", "orders_audit", "7", "sales", "orders", "I"])],
        );
        m.insert(
            "trigger_text".into(),
            vec![
                row(&["7", "D", "0", "create trigger \"sales\".orders_audit insert on \"sales\".orders referencing new as n for each row"]),
                row(&["7", "A", "0", "(insert into \"sales\".order_log (order_id) values (n.id))"]),
            ],
        );
        m
    }

    fn catalog() -> OwnerCatalog {
        normalize(&["sales".into()], &fixture(), &RedactionConfig::default()).remove(0)
    }

    #[test]
    fn census_counts_what_the_catalog_counts() {
        let c = catalog();
        assert_eq!(c.family, "systables");
        assert_eq!(c.census["TABLE"].valid, 2);
        assert_eq!(c.census["SEQUENCE"].valid, 1);
        assert_eq!(
            c.census["EXTERNAL_ROUTINE"].valid, 2,
            "C routines: counted, not read"
        );
        assert_eq!(c.census["INDEX"].valid, 4);
        assert_eq!(c.columns, 8, "table and view columns");
        reconcile(&[c]).unwrap();
    }

    #[test]
    fn columns_are_decoded_from_coltype_and_collength() {
        let c = catalog();
        let customers = c.objects.iter().find(|o| o.name == "customers").unwrap();
        let cols = customers.detail["columns"].as_array().unwrap();
        assert_eq!(cols[0]["data_type"], "serial");
        assert_eq!(cols[0]["identity"], "SERIAL");
        assert_eq!(cols[0]["nullable"], false);
        assert_eq!(cols[1]["data_type"], "varchar(100)");
        assert_eq!(cols[2]["data_type"], "datetime year to second");
        assert_eq!(cols[2]["default"], "CURRENT");
        let orders = c.objects.iter().find(|o| o.name == "orders").unwrap();
        let cols = orders.detail["columns"].as_array().unwrap();
        assert_eq!(cols[0]["identity"], "SERIAL8");
        assert_eq!(cols[1]["nullable"], true);
        assert_eq!(cols[2]["data_type"], "decimal(12,2)");
        assert_eq!(cols[2]["default"], "0.00");
        assert_eq!(cols[3]["data_type"], "lvarchar(2000)");
    }

    #[test]
    fn types_spell_as_informix_writes_them() {
        assert_eq!(
            type_text(5, 0x0AFF, None).0,
            "decimal(10)",
            "floating decimal"
        );
        assert_eq!(type_text(8, 0x1002, None).0, "money(16,2)");
        assert_eq!(type_text(13, 0x0A64, None).0, "varchar(100,10)");
        assert_eq!(type_text(0, 8, None).0, "char(8)");
        // datetime year to fraction(3): 17 digits, year (0) to fraction(3) (13)
        assert_eq!(
            type_text(10, (17 << 8) | 13, None).0,
            "datetime year to fraction(3)"
        );
        // interval day(3) to second: 9 digits, day (4) to second (10)
        assert_eq!(
            type_text(14, (9 << 8) | (4 << 4) | 10, None).0,
            "interval day(3) to second"
        );
        // interval minute(3) to fraction(3): 3 + 2 + 3 digits
        assert_eq!(
            type_text(14, (8 << 8) | (8 << 4) | 13, None).0,
            "interval minute(3) to fraction(3)"
        );
        // interval hour(4) to hour
        assert_eq!(
            type_text(14, (4 << 8) | (6 << 4) | 6, None).0,
            "interval hour(4)"
        );
        assert_eq!(type_text(41, 1, Some("boolean")).0, "boolean");
        assert_eq!(type_text(41, 72, Some("blob")).0, "blob");
        assert_eq!(type_text(52, 8, None), ("bigint".into(), true, None));
        assert_eq!(
            type_text(53 | 0x100, 8, None),
            ("bigserial".into(), false, Some("BIGSERIAL"))
        );
        assert_eq!(type_text(12, 56, None).0, "text");
    }

    #[test]
    fn keys_checks_and_foreign_keys_name_their_columns() {
        let c = catalog();
        let pk = c
            .objects
            .iter()
            .find(|o| o.name == "orders.pk_orders")
            .unwrap();
        assert_eq!(pk.kind, "primary_key");
        assert_eq!(pk.detail["columns"], json!(["id"]));
        let ux = c
            .objects
            .iter()
            .find(|o| o.name == "orders.ux_orders_ref")
            .unwrap();
        assert_eq!(ux.kind, "unique_constraint");
        assert_eq!(
            ux.detail["columns"],
            json!(["customer_id", "id"]),
            "a descending part too"
        );
        assert_eq!(ux.detail["index"], true);
        let ck = c
            .objects
            .iter()
            .find(|o| o.kind == "check_constraint")
            .unwrap();
        assert_eq!(ck.detail["condition"], "total >= 0.00");
        let fk = c.objects.iter().find(|o| o.kind == "foreign_key").unwrap();
        assert_eq!(fk.name, "orders.fk_orders_customers");
        assert_eq!(fk.detail["columns"], json!(["customer_id"]));
        assert_eq!(fk.detail["references_table"], "customers");
        assert_eq!(fk.detail["references_columns"], json!(["id"]));
        assert_eq!(fk.detail["on_delete"], "CASCADE");
    }

    #[test]
    fn a_view_is_its_pieces_joined_at_their_width() {
        let c = catalog();
        let v = c.objects.iter().find(|o| o.kind == "view").unwrap();
        let text = v.detail["text"].as_str().unwrap();
        assert!(
            text.contains("from \"sales\".orders x where (x.total > 100.00 )"),
            "pieces split inside a name and before a blank are whole again: {text}"
        );
        assert_eq!(
            join_pieces(["ab", "cd"], 3),
            "ab cd",
            "a blank the driver trimmed at a boundary comes back"
        );
    }

    #[test]
    fn piece_widths_come_from_the_catalog() {
        let w = PieceWidths::from_rows(&vec![
            row(&["syschecks", "checktext", "32"]),
            row(&["sysviews", "viewtext", "256"]),
        ]);
        assert_eq!((w.view, w.check, w.routine), (256, 32, BODY_PIECE));
        assert_eq!(PieceWidths::from_rows(&Vec::new()).view, VIEW_PIECE);
    }

    #[test]
    fn routines_carry_their_text_and_document_clause() {
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
        assert_eq!(p.detail["comment"], "Closes one order");
        assert_eq!(p.detail["language"], "spl");
        assert!(
            p.detail["text"]
                .as_str()
                .unwrap()
                .starts_with("create procedure")
        );
        let t = c.objects.iter().find(|o| o.kind == "trigger").unwrap();
        assert_eq!(t.detail["table"], "orders");
        assert_eq!(t.detail["events"], json!(["INSERT"]));
        let body = t.detail["text"].as_str().unwrap();
        assert!(
            body.contains("for each row") && body.contains("insert into"),
            "{body}"
        );
    }

    #[test]
    fn overloads_get_distinct_names() {
        let mut rows = fixture();
        rows.get_mut("routines")
            .unwrap()
            .push(row(&["sales", "order_total", "303", "f", "2"]));
        rows.get_mut("census_routines").unwrap()[0] = row(&["sales", "f", "spl", "2"]);
        let c = normalize(&["sales".into()], &rows, &RedactionConfig::default()).remove(0);
        let mut names: Vec<&str> = c
            .objects
            .iter()
            .filter(|o| o.kind == "function")
            .map(|o| o.name.as_str())
            .collect();
        names.sort();
        assert_eq!(names, ["order_total#302", "order_total#303"]);
        reconcile(&[c]).unwrap();
    }

    #[test]
    fn a_short_catalog_is_refused() {
        let mut rows = fixture();
        rows.get_mut("tables").unwrap().pop();
        let c = normalize(&["sales".into()], &rows, &RedactionConfig::default());
        let e = reconcile(&c).unwrap_err();
        assert!(e.contains("2 TABLE") && e.contains("found 1"), "{e}");
    }

    #[test]
    fn a_listed_owner_with_nothing_in_it_still_has_a_catalog() {
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
    fn the_write_check_reads_rights_and_reports_the_session_locks() {
        let logged = Guards {
            transactions: true,
            read_only_transactions: true,
            isolation: Some(crate::session::ISOLATIONS[0].into()),
            delimited_identifiers: true,
        };
        let refused = write_check_result(&row(&["0", "0", "0"]), &logged);
        assert_eq!(refused["result"], "refused_by_server");
        assert_eq!(refused["method"], "permissions");
        assert_eq!(refused["session"]["read_only_transactions"], true);
        assert!(refused.get("note").is_none());
        assert_eq!(
            write_check_result(&row(&["3", "0", "0"]), &logged)["result"],
            "user_can_write"
        );
        let resource = write_check_result(&row(&["0", "1", "0"]), &Guards::default());
        assert_eq!(resource["result"], "user_can_write");
        assert!(
            resource["note"]
                .as_str()
                .unwrap()
                .contains("no transaction logging")
        );
        for (name, q) in queries(&["s".into()]) {
            assert!(q.trim_start().starts_with("SELECT"), "{name}");
        }
        let w = write_check(&["s".into()]);
        assert!(w.starts_with("SELECT") && w.ends_with("FROM systables WHERE tabid = 1"));
        assert!(row_estimates(&["s".into()]).contains("nrows"));
    }

    #[test]
    fn values_the_optimizer_keeps_are_never_read() {
        let all: String = queries(&["s".into()])
            .into_iter()
            .map(|(_, q)| q)
            .chain([row_estimates(&["s".into()]), write_check(&["s".into()])])
            .collect();
        for value_column in ["colmin", "colmax", "sysdistrib", "encdat"] {
            assert!(!all.contains(value_column), "{value_column}");
        }
    }

    #[test]
    fn literals_are_quoted() {
        assert_eq!(literal("o'brien"), "'o''brien'");
        assert_eq!(
            check_expression("(a > 0) AND (b < 1)"),
            "(a > 0) AND (b < 1)"
        );
    }

    #[test]
    fn redaction_runs_on_text_that_leaves_the_database() {
        let mut rows = fixture();
        rows.get_mut("routine_text").unwrap()[1][3] = Some(
            "create procedure \"sales\".p() define k char(20); let k = 'AKIAIOSFODNN7EXAMPLE'; end procedure"
                .into(),
        );
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
