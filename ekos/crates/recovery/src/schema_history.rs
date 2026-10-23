//! `SchemaHistoryPass` — RFC 0210 §3.5: migration scripts read as dated change sets.
//!
//! A project that ships its schema as a sequence of scripts named `<yyyyMMddHHmm[ss]>_<description>
//! .sql` writes its history down: which change, when, for which ticket, in which release. Read as
//! competing table definitions those scripts only produce noise (RFC 0172 reported 2,372
//! "conflicts" on iDempiere); read in order, they answer "when and why did this column appear".
//!
//! Per file this pass writes one `Custom("SchemaChangeSet")` (keyed by path) and one
//! `Custom("SchemaChange")` per DDL operation (keyed by path and ordinal) — `create_table`,
//! `add_column`, `modify_column`, `add_constraint`, … — with the table, column, type and the
//! statement as evidence. `semantic::schema_history` links each change to its table at commit.
//!
//! Statements are classified from their tokens, not from a `sqlparser` AST: Oracle's `MODIFY c NOT
//! NULL` reads in `sqlparser` 0.53 as a column whose *type* is `NOT` (measured), and a history that
//! records that is worse than none. The classifier knows the DDL both engines write and counts what
//! it does not recognise, by line. Script facts are **claims**: nothing here says a change was
//! applied (RFC 0211 checks them against the catalog). Deterministic, no LLM.
//!
//! One family convention is read as DDL: iDempiere's PostgreSQL scripts change a column with
//! `INSERT INTO t_alter_column values(table, column, type, null clause, default)`, a row whose
//! trigger runs the `ALTER` ([`alter_column_row`]).

use crate::sql_objects::{clip, file_kir_id};
use async_trait::async_trait;
use ekos_compiler_core::pass::{CompilerPass, PassContext, PassError};
use ekos_kir::{
    KirEvidence, KirGraph, KirId, KirObject, KirRelationship, ObjectKind, RelationshipKind,
    SourceLocation,
};
use ekos_plpgsql::line_of;
use ekos_plsql::script::{
    Tok, TokKind, UnitKind, matching_paren, neutralize_quotes, tokens, units,
};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

/// The change-set kind (one per migration script).
pub const CHANGE_SET: &str = "SchemaChangeSet";
/// The change kind (one per DDL operation).
pub const CHANGE: &str = "SchemaChange";

const LOGIC_VERSION: &str = "schema-history/1";
const MAX_FRAGMENT: usize = 2048;

/// Directory names that say which engine a script is for; never a release name.
const ENGINE_DIRS: &[&str] = &[
    "oracle",
    "postgresql",
    "postgres",
    "pg",
    "mysql",
    "mssql",
    "sqlserver",
    "snowflake",
    "clickhouse",
];

/// What a migration script's file name says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationName {
    /// ISO 8601 local time, to the minute or second the name gives.
    pub applied_at: String,
    pub description: String,
    /// `KEY-NNNN` ticket ids in the description, upper-cased, in order, without repeats.
    pub tickets: Vec<String>,
}

/// `202402020915_IDEMPIERE-6028.sql` → its time, description and tickets. `None` for any other
/// name — including a 12-digit prefix that is not a valid date and time.
pub fn migration_name(file_name: &str) -> Option<MigrationName> {
    let stem = file_name
        .strip_suffix(".sql")
        .or_else(|| file_name.strip_suffix(".SQL"))?;
    let (stamp, description) = stem.split_once('_')?;
    if !(stamp.len() == 12 || stamp.len() == 14) || !stamp.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let n = |a: usize, b: usize| stamp[a..b].parse::<u32>().unwrap_or(99);
    let (month, day, hour, minute) = (n(4, 6), n(6, 8), n(8, 10), n(10, 12));
    let valid = (1..=12).contains(&month) && (1..=31).contains(&day) && hour < 24 && minute < 60;
    if !valid {
        return None;
    }
    let mut applied_at = format!(
        "{}-{}-{}T{}:{}",
        &stamp[..4],
        &stamp[4..6],
        &stamp[6..8],
        &stamp[8..10],
        &stamp[10..12]
    );
    if stamp.len() == 14 {
        applied_at.push(':');
        applied_at.push_str(&stamp[12..14]);
    }
    Some(MigrationName {
        applied_at,
        description: description.to_string(),
        tickets: tickets_in(description),
    })
}

/// `IDEMPIERE-6028`, `Ticket_AP2-671` → `["IDEMPIERE-6028"]`, `["AP2-671"]`.
fn tickets_in(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for part in text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-')) {
        let Some((key, num)) = part.rsplit_once('-') else {
            continue;
        };
        let key_ok = key.len() >= 2
            && key.starts_with(|c: char| c.is_ascii_alphabetic())
            && key.chars().all(|c| c.is_ascii_alphanumeric());
        let num_ok = !num.is_empty() && num.chars().all(|c| c.is_ascii_digit());
        if key_ok && num_ok {
            let t = format!("{}-{num}", key.to_ascii_uppercase());
            if !out.contains(&t) {
                out.push(t);
            }
        }
    }
    out
}

/// The release a script belongs to: its directory, or the one above when that is an engine name
/// (`migration/iD13/oracle/x.sql` → `iD13`).
pub fn release_of(path: &str) -> Option<String> {
    let mut dirs: Vec<&str> = path.split(['/', '\\']).collect();
    dirs.pop();
    let mut last = dirs.pop()?;
    if ENGINE_DIRS.contains(&last.to_ascii_lowercase().as_str()) {
        last = dirs.pop()?;
    }
    Some(last.to_string())
}

/// One column as a `CREATE TABLE` or `ADD` states it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ColumnSpec {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub not_null: Option<bool>,
}

/// A table constraint as stated.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ConstraintSpec {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// `primary_key`, `foreign_key`, `unique`, `check`.
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub columns: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub references: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expression: Option<String>,
}

/// One schema change a statement makes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub operation: &'static str,
    /// The table changed (or indexed). `None` for a dropped index or a sequence.
    pub table: Option<String>,
    /// The object the change names when it is not the table: index, view, sequence, new name.
    pub name: Option<String>,
    pub column: Option<ColumnSpec>,
    pub constraint: Option<ConstraintSpec>,
    /// `create_table`: its columns. `create_index`: the indexed columns, as `name` only.
    pub columns: Vec<ColumnSpec>,
    /// `create_table`: its table constraints.
    pub constraints: Vec<ConstraintSpec>,
}

impl Change {
    fn new(operation: &'static str, table: Option<String>) -> Self {
        Self {
            operation,
            table,
            name: None,
            column: None,
            constraint: None,
            columns: Vec::new(),
            constraints: Vec::new(),
        }
    }
}

/// What one statement is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Classified {
    /// DDL: the changes it makes, and how many of its operations were not recognised.
    Changes(Vec<Change>, usize),
    /// `INSERT` / `UPDATE` / `DELETE` / `MERGE` on a table.
    Data(String),
    /// Anything else: a query, a grant, a comment, a PL/SQL unit.
    Other,
}

/// A statement's tokens with the text they index.
struct Stmt<'a> {
    text: &'a str,
    toks: Vec<Tok>,
}

/// Words that end a column's type in a column definition.
const TYPE_STOPS: &[&str] = &[
    "DEFAULT",
    "NOT",
    "NULL",
    "CONSTRAINT",
    "PRIMARY",
    "REFERENCES",
    "CHECK",
    "UNIQUE",
    "COLLATE",
    "GENERATED",
    "ENABLE",
    "DISABLE",
];

/// Words that end a `DEFAULT` value.
const DEFAULT_STOPS: &[&str] = &[
    "NOT",
    "CONSTRAINT",
    "PRIMARY",
    "REFERENCES",
    "CHECK",
    "UNIQUE",
    "ENABLE",
    "DISABLE",
];

impl Stmt<'_> {
    fn is(&self, i: usize, w: &str) -> bool {
        self.toks.get(i).is_some_and(|t| t.is(w))
    }

    fn sym(&self, i: usize, c: u8) -> bool {
        self.toks.get(i).is_some_and(|t| t.is_sym(c))
    }

    fn squash(&self, from: usize, to_incl: usize) -> String {
        let s = &self.text[self.toks[from].start..self.toks[to_incl].end];
        s.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// A possibly dotted, possibly quoted name at `i`, as written, and the index after it.
    fn name(&self, i: usize) -> Option<(String, usize)> {
        let mut parts = Vec::new();
        let mut j = i;
        loop {
            let t = self.toks.get(j)?;
            let part = match &t.kind {
                TokKind::Word(_) => self.text[t.start..t.end].to_string(),
                TokKind::Quoted(q) => q.clone(),
                _ => return None,
            };
            parts.push(part);
            j += 1;
            if !(self.sym(j, b'.') && self.toks.get(j + 1).is_some()) {
                break;
            }
            j += 1;
        }
        Some((parts.join("."), j))
    }

    /// Skips `IF [NOT] EXISTS` at `i`.
    fn skip_if_exists(&self, mut i: usize) -> usize {
        if self.is(i, "IF") {
            i += 1;
            if self.is(i, "NOT") {
                i += 1;
            }
            i += 1;
        }
        i
    }

    /// Token ranges `[from, to)` of the items in `from..to`, split at commas at depth 0.
    fn items(&self, from: usize, to: usize) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        let mut depth = 0i32;
        let mut start = from;
        for j in from..to {
            let t = &self.toks[j];
            if t.is_sym(b'(') {
                depth += 1;
            } else if t.is_sym(b')') {
                depth -= 1;
            } else if depth == 0 && t.is_sym(b',') {
                out.push((start, j));
                start = j + 1;
            }
        }
        if start < to {
            out.push((start, to));
        }
        out
    }

    /// Names inside the parentheses opening at `open`.
    fn name_list(&self, open: usize) -> (Vec<String>, usize) {
        let Some(close) = matching_paren(&self.toks, open) else {
            return (Vec::new(), self.toks.len());
        };
        let names = self
            .items(open + 1, close)
            .into_iter()
            .filter_map(|(a, _)| self.name(a).map(|(n, _)| n))
            .collect();
        (names, close + 1)
    }
}

/// The end of the statement's real tokens (a trailing `;` excluded).
fn body_end(toks: &[Tok]) -> usize {
    if toks.last().is_some_and(|t| t.is_sym(b';')) {
        toks.len() - 1
    } else {
        toks.len()
    }
}

/// Classify one statement (quotes already neutralized for Oracle).
pub fn classify(text: &str) -> Classified {
    let mut toks = tokens(text);
    toks.truncate(body_end(&toks));
    let s = Stmt { text, toks };
    match s
        .toks
        .first()
        .and_then(Tok::ident)
        .map(str::to_ascii_uppercase)
        .as_deref()
    {
        Some("CREATE") => create(&s),
        Some("ALTER") if s.is(1, "TABLE") => alter_table(&s),
        Some("DROP") => drop(&s),
        Some("INSERT") => alter_column_row(&s).unwrap_or_else(|| data_target(&s, "INTO")),
        Some("MERGE") => data_target(&s, "INTO"),
        Some("UPDATE") => s
            .name(1)
            .map_or(Classified::Other, |(n, _)| Classified::Data(n)),
        Some("DELETE") => {
            let at = if s.is(1, "FROM") { 2 } else { 1 };
            s.name(at)
                .map_or(Classified::Other, |(n, _)| Classified::Data(n))
        }
        _ => Classified::Other,
    }
}

/// iDempiere's PostgreSQL scripts change a column by inserting a row into `t_alter_column`, whose
/// trigger runs the `ALTER`: `INSERT INTO t_alter_column values('t','c','NUMERIC(10)',null,'NULL')`
/// — table, column, type, null clause, default clause, each `NULL` when unchanged. That row is a
/// schema change, and read as data it would leave the PostgreSQL history without most of its
/// column changes.
fn alter_column_row(s: &Stmt) -> Option<Classified> {
    let (table, j) = s.name(2).filter(|_| s.is(1, "INTO"))?;
    if !table.eq_ignore_ascii_case("t_alter_column") {
        return None;
    }
    let values = (j..s.toks.len()).find(|&k| s.is(k, "VALUES"))?;
    let (open, close) = (values + 1, matching_paren(&s.toks, values + 1)?);
    let args: Vec<Option<String>> = s
        .items(open + 1, close)
        .into_iter()
        .map(|(a, _)| match s.toks[a].kind {
            TokKind::Str => {
                let lit = &s.text[s.toks[a].start..s.toks[a].end];
                Some(lit[1..lit.len() - 1].replace("''", "'"))
            }
            _ => None,
        })
        .collect();
    let [Some(t), Some(c), data_type, null_clause, default] = args.as_slice() else {
        return None;
    };
    let mut change = Change::new("modify_column", Some(t.clone()));
    change.column = Some(ColumnSpec {
        name: c.clone(),
        data_type: data_type.clone(),
        default: default.clone(),
        not_null: null_clause
            .as_deref()
            .map(|n| n.trim().eq_ignore_ascii_case("NOT NULL")),
    });
    Some(Classified::Changes(vec![change], 0))
}

fn data_target(s: &Stmt, after: &str) -> Classified {
    let at = (0..s.toks.len()).find(|&i| s.is(i, after));
    at.and_then(|i| s.name(i + 1))
        .map_or(Classified::Other, |(n, _)| Classified::Data(n))
}

const CREATE_MODIFIERS: &[&str] = &[
    "OR",
    "REPLACE",
    "FORCE",
    "NOFORCE",
    "EDITIONABLE",
    "NONEDITIONABLE",
    "GLOBAL",
    "TEMPORARY",
    "TEMP",
    "UNLOGGED",
    "UNIQUE",
    "BITMAP",
    "MATERIALIZED",
];

fn create(s: &Stmt) -> Classified {
    let mut i = 1;
    while s
        .toks
        .get(i)
        .is_some_and(|t| CREATE_MODIFIERS.iter().any(|m| t.is(m)))
    {
        i += 1;
    }
    let what = s
        .toks
        .get(i)
        .and_then(Tok::ident)
        .map(str::to_ascii_uppercase);
    let at = s.skip_if_exists(i + 1);
    let one = |c: Change| Classified::Changes(vec![c], 0);
    match what.as_deref() {
        Some("TABLE") => s
            .name(at)
            .map_or(Classified::Other, |(t, j)| one(create_table(s, t, j))),
        Some("VIEW") => s.name(at).map_or(Classified::Other, |(v, _)| {
            let mut c = Change::new("create_view", None);
            c.name = Some(v);
            one(c)
        }),
        Some("INDEX") => create_index(s, at).map_or(Classified::Other, one),
        Some("SEQUENCE") => s.name(at).map_or(Classified::Other, |(q, _)| {
            let mut c = Change::new("create_sequence", None);
            c.name = Some(q);
            one(c)
        }),
        _ => Classified::Other,
    }
}

fn create_table(s: &Stmt, table: String, at: usize) -> Change {
    let mut c = Change::new("create_table", Some(table));
    if !s.sym(at, b'(') {
        return c; // `CREATE TABLE t AS SELECT …`: columns from a query, not stated
    }
    let close = matching_paren(&s.toks, at).unwrap_or(s.toks.len());
    for (a, b) in s.items(at + 1, close) {
        if is_constraint_start(s, a) {
            c.constraints.extend(constraint(s, a, b));
        } else if let Some(col) = column_def(s, a, b) {
            c.columns.push(col);
        }
    }
    c
}

/// `CREATE [UNIQUE] INDEX [IF NOT EXISTS] name ON table (cols)`.
fn create_index(s: &Stmt, at: usize) -> Option<Change> {
    let (index, j) = s.name(at)?;
    let on = (j..s.toks.len()).find(|&k| s.is(k, "ON"))?;
    let mut k = on + 1;
    if s.is(k, "ONLY") {
        k += 1;
    }
    let (table, after) = s.name(k)?;
    let mut c = Change::new("create_index", Some(table));
    c.name = Some(index);
    let open = (after..s.toks.len()).find(|&x| s.sym(x, b'('));
    if let Some(open) = open {
        c.columns = s
            .name_list(open)
            .0
            .into_iter()
            .map(|name| ColumnSpec {
                name,
                ..Default::default()
            })
            .collect();
    }
    Some(c)
}

fn drop(s: &Stmt) -> Classified {
    let what = s
        .toks
        .get(1)
        .and_then(Tok::ident)
        .map(str::to_ascii_uppercase);
    let op = match what.as_deref() {
        Some("TABLE") => "drop_table",
        Some("VIEW") => "drop_view",
        Some("INDEX") => "drop_index",
        Some("SEQUENCE") => "drop_sequence",
        _ => return Classified::Other,
    };
    let at = s.skip_if_exists(2);
    let Some((name, _)) = s.name(at) else {
        return Classified::Other;
    };
    let c = if op == "drop_table" {
        Change::new(op, Some(name))
    } else {
        let mut c = Change::new(op, None);
        c.name = Some(name);
        c
    };
    Classified::Changes(vec![c], 0)
}

fn alter_table(s: &Stmt) -> Classified {
    let mut i = 2;
    if s.is(i, "ONLY") {
        i += 1;
    }
    i = s.skip_if_exists(i);
    let Some((table, from)) = s.name(i) else {
        return Classified::Other;
    };
    let mut changes = Vec::new();
    let mut unrecognized = 0;
    for (a, b) in s.items(from, s.toks.len()) {
        match operation(s, &table, a, b) {
            Some(mut cs) => changes.append(&mut cs),
            None => unrecognized += 1,
        }
    }
    Classified::Changes(changes, unrecognized)
}

/// One `ALTER TABLE` operation, tokens `a..b`. `None` when not recognised.
fn operation(s: &Stmt, table: &str, a: usize, b: usize) -> Option<Vec<Change>> {
    let verb = s.toks.get(a)?.ident()?.to_ascii_uppercase();
    match verb.as_str() {
        "ADD" => Some(add(s, table, a + 1, b)),
        "MODIFY" => Some(modify(s, table, a + 1, b)),
        "ALTER" => alter_column(s, table, a + 1, b).map(|c| vec![c]),
        "DROP" => drop_part(s, table, a + 1),
        "RENAME" => rename(s, table, a + 1).map(|c| vec![c]),
        _ => None,
    }
}

fn is_constraint_start(s: &Stmt, i: usize) -> bool {
    ["CONSTRAINT", "PRIMARY", "FOREIGN", "UNIQUE", "CHECK"]
        .iter()
        .any(|w| s.is(i, w))
}

/// `ADD [COLUMN] [IF NOT EXISTS] col …`, `ADD [CONSTRAINT c] …`, `ADD (item, …)`.
fn add(s: &Stmt, table: &str, mut i: usize, b: usize) -> Vec<Change> {
    if s.is(i, "COLUMN") {
        i += 1;
    }
    i = s.skip_if_exists(i);
    let ranges = if s.sym(i, b'(') {
        let close = matching_paren(&s.toks, i).unwrap_or(b).min(b);
        s.items(i + 1, close)
    } else {
        vec![(i, b)]
    };
    let mut out = Vec::new();
    for (x, y) in ranges {
        if is_constraint_start(s, x) {
            if let Some(k) = constraint(s, x, y) {
                let mut c = Change::new("add_constraint", Some(table.to_string()));
                c.constraint = Some(k);
                out.push(c);
            }
        } else if let Some(col) = column_def(s, x, y) {
            let mut c = Change::new("add_column", Some(table.to_string()));
            c.column = Some(col);
            out.push(c);
        }
    }
    out
}

/// `MODIFY [COLUMN] col [type] [DEFAULT x] [[NOT] NULL]`, or `MODIFY (item, …)`.
fn modify(s: &Stmt, table: &str, mut i: usize, b: usize) -> Vec<Change> {
    if s.is(i, "COLUMN") {
        i += 1;
    }
    let ranges = if s.sym(i, b'(') {
        let close = matching_paren(&s.toks, i).unwrap_or(b).min(b);
        s.items(i + 1, close)
    } else {
        vec![(i, b)]
    };
    ranges
        .into_iter()
        .filter_map(|(x, y)| column_def(s, x, y))
        .map(|col| {
            let mut c = Change::new("modify_column", Some(table.to_string()));
            c.column = Some(col);
            c
        })
        .collect()
}

/// PostgreSQL's `ALTER [COLUMN] c TYPE t | SET DATA TYPE t | SET DEFAULT x | DROP DEFAULT |
/// SET NOT NULL | DROP NOT NULL`.
fn alter_column(s: &Stmt, table: &str, mut i: usize, b: usize) -> Option<Change> {
    if s.is(i, "COLUMN") {
        i += 1;
    }
    let (name, j) = s.name(i)?;
    let mut col = ColumnSpec {
        name,
        ..Default::default()
    };
    let rest = |k: usize| (k < b).then(|| s.squash(k, b - 1));
    if s.is(j, "TYPE") {
        col.data_type = rest(j + 1).map(|t| until_using(&t));
    } else if s.is(j, "SET") && s.is(j + 1, "DATA") && s.is(j + 2, "TYPE") {
        col.data_type = rest(j + 3).map(|t| until_using(&t));
    } else if s.is(j, "SET") && s.is(j + 1, "DEFAULT") {
        col.default = rest(j + 2);
    } else if s.is(j, "DROP") && s.is(j + 1, "DEFAULT") {
        col.default = Some("NULL".into());
    } else if s.is(j, "SET") && s.is(j + 1, "NOT") {
        col.not_null = Some(true);
    } else if s.is(j, "DROP") && s.is(j + 1, "NOT") {
        col.not_null = Some(false);
    } else {
        return None;
    }
    let mut c = Change::new("modify_column", Some(table.to_string()));
    c.column = Some(col);
    Some(c)
}

/// `numeric(10) USING x::numeric` → `numeric(10)`.
fn until_using(t: &str) -> String {
    let upper = t.to_ascii_uppercase();
    match upper.find(" USING ") {
        Some(at) => t[..at].trim().to_string(),
        None => t.to_string(),
    }
}

/// `DROP COLUMN c`, `DROP (c1, c2)`, `DROP CONSTRAINT k`, `DROP PRIMARY KEY`.
fn drop_part(s: &Stmt, table: &str, mut i: usize) -> Option<Vec<Change>> {
    let column = |name: String| {
        let mut c = Change::new("drop_column", Some(table.to_string()));
        c.column = Some(ColumnSpec {
            name,
            ..Default::default()
        });
        c
    };
    if s.is(i, "CONSTRAINT") {
        let (name, _) = s.name(s.skip_if_exists(i + 1))?;
        let mut c = Change::new("drop_constraint", Some(table.to_string()));
        c.constraint = Some(ConstraintSpec {
            name: Some(name),
            kind: "unknown",
            ..Default::default()
        });
        return Some(vec![c]);
    }
    if s.is(i, "PRIMARY") {
        let mut c = Change::new("drop_constraint", Some(table.to_string()));
        c.constraint = Some(ConstraintSpec {
            kind: "primary_key",
            ..Default::default()
        });
        return Some(vec![c]);
    }
    if s.sym(i, b'(') {
        return Some(s.name_list(i).0.into_iter().map(column).collect());
    }
    if s.is(i, "COLUMN") {
        i += 1;
    }
    i = s.skip_if_exists(i);
    s.name(i).map(|(n, _)| vec![column(n)])
}

/// `RENAME COLUMN a TO b`, `RENAME TO t`, `RENAME CONSTRAINT a TO b`.
fn rename(s: &Stmt, table: &str, i: usize) -> Option<Change> {
    if s.is(i, "TO") {
        let (to, _) = s.name(i + 1)?;
        let mut c = Change::new("rename_table", Some(table.to_string()));
        c.name = Some(to);
        return Some(c);
    }
    let (op, at) = if s.is(i, "COLUMN") {
        ("rename_column", i + 1)
    } else if s.is(i, "CONSTRAINT") {
        ("rename_constraint", i + 1)
    } else {
        ("rename_column", i)
    };
    let (from, j) = s.name(at)?;
    if !s.is(j, "TO") {
        return None;
    }
    let (to, _) = s.name(j + 1)?;
    let mut c = Change::new(op, Some(table.to_string()));
    if op == "rename_column" {
        c.column = Some(ColumnSpec {
            name: from,
            ..Default::default()
        });
    } else {
        c.constraint = Some(ConstraintSpec {
            name: Some(from),
            kind: "unknown",
            ..Default::default()
        });
    }
    c.name = Some(to);
    Some(c)
}

/// A column definition, tokens `a..b`: name, type, default, nullability.
fn column_def(s: &Stmt, a: usize, b: usize) -> Option<ColumnSpec> {
    let (name, mut j) = s.name(a)?;
    let mut col = ColumnSpec {
        name,
        ..Default::default()
    };
    let type_start = j;
    let mut depth = 0i32;
    while j < b {
        let t = &s.toks[j];
        if t.is_sym(b'(') {
            depth += 1;
        } else if t.is_sym(b')') {
            depth -= 1;
        } else if depth == 0 && TYPE_STOPS.iter().any(|w| t.is(w)) {
            break;
        }
        j += 1;
    }
    if j > type_start {
        col.data_type = Some(s.squash(type_start, j - 1));
    }
    column_attributes(s, &mut col, j, b);
    Some(col)
}

/// `DEFAULT x`, `NOT NULL`, `NULL` after a column's type.
fn column_attributes(s: &Stmt, col: &mut ColumnSpec, mut j: usize, b: usize) {
    while j < b {
        if s.is(j, "DEFAULT") && j + 1 < b {
            let from = j + 1;
            let mut k = from + 1; // the value's first token may itself be `NULL`
            while k < b
                && !DEFAULT_STOPS.iter().any(|w| s.is(k, w))
                && !(s.is(k, "NULL") && k > from)
            {
                k += 1;
            }
            col.default = Some(s.squash(from, k - 1));
            j = k;
        } else if s.sym(j, b'(') {
            // A `CHECK (x IS NOT NULL)` says nothing about the column's own nullability.
            j = matching_paren(&s.toks, j).map_or(b, |c| c + 1);
        } else if s.is(j, "NOT") && s.is(j + 1, "NULL") {
            col.not_null = Some(true);
            j += 2;
        } else if s.is(j, "NULL") {
            col.not_null = Some(false);
            j += 1;
        } else {
            j += 1;
        }
    }
}

/// A table constraint, tokens `a..b`: `[CONSTRAINT name] PRIMARY KEY (…) | FOREIGN KEY (…)
/// REFERENCES t [(…)] | UNIQUE (…) | CHECK (…)`.
fn constraint(s: &Stmt, a: usize, b: usize) -> Option<ConstraintSpec> {
    let mut i = a;
    let mut name = None;
    if s.is(i, "CONSTRAINT") {
        let (n, j) = s.name(i + 1)?;
        name = Some(n);
        i = j;
    }
    let mut k = ConstraintSpec {
        name,
        ..Default::default()
    };
    let list_at = |from: usize| (from..b).find(|&x| s.sym(x, b'('));
    if s.is(i, "PRIMARY") || s.is(i, "UNIQUE") {
        k.kind = if s.is(i, "PRIMARY") {
            "primary_key"
        } else {
            "unique"
        };
        k.columns = list_at(i).map(|o| s.name_list(o).0).unwrap_or_default();
    } else if s.is(i, "FOREIGN") {
        k.kind = "foreign_key";
        let (cols, after) = list_at(i).map_or((Vec::new(), i), |o| s.name_list(o));
        k.columns = cols;
        let r = (after..b).find(|&x| s.is(x, "REFERENCES"))?;
        k.references = s.name(r + 1).map(|(t, _)| t);
    } else if s.is(i, "CHECK") {
        k.kind = "check";
        let open = list_at(i)?;
        let close = matching_paren(&s.toks, open)?;
        if close > open + 1 {
            k.expression = Some(s.squash(open + 1, close - 1));
        }
    } else {
        return None;
    }
    Some(k)
}

/// Per-file counters, for `ekos recover`'s summary.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SchemaHistoryStats {
    pub change_sets: usize,
    pub changes: usize,
    pub by_operation: BTreeMap<String, usize>,
    pub data_statements: usize,
    pub other_statements: usize,
    /// `ALTER TABLE` operations the classifier did not recognise.
    pub unrecognized: usize,
}

pub struct SchemaHistoryPass {
    pass_id: String,
    source_path: String,
    sql: String,
    dialect_name: String,
    file_key: Option<String>,
    stats: Arc<Mutex<SchemaHistoryStats>>,
}

impl SchemaHistoryPass {
    pub fn new(
        source_path: impl Into<String>,
        sql: impl Into<String>,
        dialect_name: impl Into<String>,
    ) -> Self {
        let source_path = source_path.into();
        Self {
            pass_id: format!("schema-history:{source_path}"),
            source_path,
            sql: sql.into(),
            dialect_name: dialect_name.into(),
            file_key: None,
            stats: Arc::new(Mutex::new(SchemaHistoryStats::default())),
        }
    }

    pub fn with_file(mut self, file_key: impl Into<String>) -> Self {
        self.file_key = Some(file_key.into());
        self
    }

    pub fn stats_handle(&self) -> Arc<Mutex<SchemaHistoryStats>> {
        Arc::clone(&self.stats)
    }

    /// Whether `path` names a migration script.
    pub fn applies_to(path: &str) -> bool {
        let file = path.rsplit(['/', '\\']).next().unwrap_or(path);
        migration_name(file).is_some()
    }
}

#[async_trait]
impl CompilerPass for SchemaHistoryPass {
    fn name(&self) -> &str {
        &self.pass_id
    }

    fn cache_inputs(&self) -> Vec<String> {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(LOGIC_VERSION.as_bytes());
        hasher.update(self.source_path.as_bytes());
        hasher.update(self.dialect_name.as_bytes());
        hasher.update(self.file_key.as_deref().unwrap_or_default().as_bytes());
        hasher.update(self.sql.as_bytes());
        vec![hex::encode(hasher.finalize())]
    }

    async fn run(&mut self, ctx: &mut PassContext) -> Result<(), PassError> {
        let (graph, stats) = recover_schema_history(
            &self.source_path,
            &self.sql,
            &self.dialect_name,
            self.file_key.as_deref(),
        );
        *self.stats.lock().unwrap() = stats;
        if graph.objects.is_empty() {
            return Ok(());
        }
        let knowledge = ekos_artifact::KnowledgeArtifact::new(&self.pass_id, vec![], graph);
        let json = serde_json::to_value(&knowledge)
            .map_err(|e| PassError::failed(format!("serialize KnowledgeArtifact: {e}")))?;
        ctx.artifact_store
            .write(&knowledge.id, &json)
            .map_err(|e| PassError::failed(format!("write artifact: {e}")))?;
        Ok(())
    }
}

fn kid(seed: &str) -> KirId {
    KirId(Uuid::new_v5(&Uuid::NAMESPACE_URL, seed.as_bytes()))
}

/// Byte ranges of the file's SQL statements: Oracle scripts split the way SQL*Plus reads them
/// (PL/SQL units and SQL*Plus commands are not statements here), others on `;` outside quotes.
fn statement_ranges(sql: &str, oracle: bool) -> Vec<(usize, usize)> {
    if oracle {
        return units(sql)
            .into_iter()
            .filter(|u| u.kind == UnitKind::Statement)
            .map(|u| (u.start, u.end))
            .collect();
    }
    let chars: Vec<char> = sql.chars().collect();
    let mut byte_at = Vec::with_capacity(chars.len() + 1);
    let mut b = 0;
    for c in &chars {
        byte_at.push(b);
        b += c.len_utf8();
    }
    byte_at.push(b);
    ekos_sql_dialect_sdk::lex::statement_spans(&chars)
        .into_iter()
        .map(|(s, e)| {
            let s = ekos_sql_dialect_sdk::lex::skip_trivia(&chars, s).min(e);
            (byte_at[s], byte_at[e])
        })
        .filter(|(s, e)| s < e)
        .collect()
}

/// Recover one migration script's change set. Pure. Empty when the file is not a migration script.
pub fn recover_schema_history(
    source_path: &str,
    sql: &str,
    dialect_name: &str,
    file_key: Option<&str>,
) -> (KirGraph, SchemaHistoryStats) {
    let mut graph = KirGraph::new();
    let mut stats = SchemaHistoryStats::default();
    let file = source_path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(source_path);
    let Some(name) = migration_name(file) else {
        return (graph, stats);
    };
    let oracle = matches!(dialect_name, "oracle" | "plsql");
    let set_id = kid(&format!("schema-change-set:{source_path}"));
    let mut data: BTreeMap<String, usize> = BTreeMap::new();
    let mut unrecognized_lines: Vec<u32> = Vec::new();
    let mut ddl_statements = 0usize;
    let ctx = SetCtx {
        source_path,
        set_id,
        name: &name,
        engine: dialect_name,
    };
    for (start, end) in statement_ranges(sql, oracle) {
        let raw = &sql[start..end];
        let text = if oracle {
            neutralize_quotes(raw)
        } else {
            raw.to_string()
        };
        let line = line_of(sql, start);
        match classify(&text) {
            Classified::Changes(changes, unknown) => {
                if unknown > 0 {
                    stats.unrecognized += unknown;
                    unrecognized_lines.push(line);
                }
                ddl_statements += usize::from(!changes.is_empty());
                for c in changes {
                    *stats
                        .by_operation
                        .entry(c.operation.to_string())
                        .or_default() += 1;
                    emit_change(&mut graph, &ctx, stats.changes, &c, raw, line);
                    stats.changes += 1;
                }
            }
            Classified::Data(table) => {
                stats.data_statements += 1;
                *data.entry(table).or_default() += 1;
            }
            Classified::Other => stats.other_statements += 1,
        }
    }
    // Not the file name: a `File` object already carries that, and the engine twins share it.
    let set_name = format!("{} {} ({dialect_name})", name.applied_at, name.description);
    let mut set = KirObject::new(set_name, ObjectKind::Custom(CHANGE_SET.into()));
    set.id = set_id;
    for (k, v) in [
        ("applied_at", json!(name.applied_at)),
        ("description", json!(name.description)),
        ("tickets", json!(name.tickets)),
        ("release", json!(release_of(source_path))),
        ("engine", json!(dialect_name)),
        ("source_path", json!(source_path)),
        ("ddl_statements", json!(ddl_statements)),
        ("changes", json!(stats.changes)),
        ("data_statements", json!(stats.data_statements)),
        ("data_tables", json!(data)),
        ("other_statements", json!(stats.other_statements)),
        ("unrecognized_lines", json!(unrecognized_lines)),
    ] {
        set.properties.insert(k.into(), v);
    }
    let mut ev = KirEvidence::new(
        SourceLocation {
            path: source_path.to_string(),
            line: Some(1),
            column: None,
        },
        format!("migration script {file}"),
    );
    ev.id = kid(&format!("schema-change-set-evidence:{source_path}"));
    set.evidence.push(graph.add_evidence(ev));
    if let Some(key) = file_key {
        graph.add_relationship(KirRelationship::deterministic(
            RelationshipKind::Contains,
            file_kir_id(key),
            set_id,
            "",
        ));
    }
    graph.add_object(set);
    stats.change_sets = 1;
    (graph, stats)
}

struct SetCtx<'a> {
    source_path: &'a str,
    set_id: KirId,
    name: &'a MigrationName,
    engine: &'a str,
}

fn change_name(c: &Change) -> String {
    let table = c.table.as_deref().unwrap_or("");
    let target = match (&c.column, &c.constraint, &c.name) {
        (Some(col), _, _) => format!("{table}.{}", col.name),
        (None, Some(k), _) if k.name.is_some() => {
            format!("{table}.{}", k.name.as_deref().unwrap_or(""))
        }
        (None, _, Some(n)) if table.is_empty() => n.clone(),
        _ => table.to_string(),
    };
    format!("{} {target}", c.operation)
}

fn emit_change(
    graph: &mut KirGraph,
    ctx: &SetCtx,
    ordinal: usize,
    c: &Change,
    raw: &str,
    line: u32,
) {
    let id = kid(&format!("schema-change:{}:{ordinal}", ctx.source_path));
    let mut o = KirObject::new(change_name(c), ObjectKind::Custom(CHANGE.into()));
    o.id = id;
    let opt = |v: &Option<String>| v.as_ref().map_or(Value::Null, |s| json!(s));
    for (k, v) in [
        ("operation", json!(c.operation)),
        ("table", opt(&c.table)),
        ("name", opt(&c.name)),
        ("ordinal", json!(ordinal)),
        ("applied_at", json!(ctx.name.applied_at)),
        ("tickets", json!(ctx.name.tickets)),
        ("engine", json!(ctx.engine)),
        ("source_path", json!(ctx.source_path)),
        ("line", json!(line)),
    ] {
        o.properties.insert(k.into(), v);
    }
    if let Some(col) = &c.column {
        o.properties.insert("column".into(), json!(col));
    }
    if let Some(k) = &c.constraint {
        o.properties.insert("constraint".into(), json!(k));
    }
    if !c.columns.is_empty() {
        o.properties.insert("columns".into(), json!(c.columns));
    }
    if !c.constraints.is_empty() {
        o.properties
            .insert("constraints".into(), json!(c.constraints));
    }
    let mut ev = KirEvidence::new(
        SourceLocation {
            path: ctx.source_path.to_string(),
            line: Some(line),
            column: None,
        },
        clip(raw.trim(), MAX_FRAGMENT),
    );
    ev.id = kid(&format!(
        "schema-change-evidence:{}:{ordinal}",
        ctx.source_path
    ));
    o.evidence.push(graph.add_evidence(ev));
    graph.add_relationship(KirRelationship::deterministic(
        RelationshipKind::Contains,
        ctx.set_id,
        id,
        "",
    ));
    graph.add_object(o);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn changes(sql: &str) -> Vec<Change> {
        match classify(sql) {
            Classified::Changes(c, 0) => c,
            other => panic!("{sql}: {other:?}"),
        }
    }

    #[test]
    fn both_kinds_have_a_keyed_registry_row() {
        for kind in [CHANGE_SET, CHANGE] {
            assert!(
                ekos_kir::custom_kinds::is_structurally_keyed(kind),
                "{kind} must be registered as structurally keyed"
            );
        }
    }

    #[test]
    fn migration_names() {
        let n = migration_name("202402020915_IDEMPIERE-6028.sql").unwrap();
        assert_eq!(n.applied_at, "2024-02-02T09:15");
        assert_eq!(n.tickets, ["IDEMPIERE-6028"]);
        let n = migration_name("20130621183000_IDEMPIERE-966.sql").unwrap();
        assert_eq!(n.applied_at, "2013-06-21T18:30:00");
        assert_eq!(
            migration_name("201810122100_Ticket_AP2-671.sql")
                .unwrap()
                .tickets,
            ["AP2-671"]
        );
        assert!(
            migration_name("201804121850_Extend_Export_Feature.sql")
                .unwrap()
                .tickets
                .is_empty()
        );
        for bad in [
            "schema.sql",
            "999999999999_x.sql",
            "2024_x.sql",
            "202402020915.sql",
            "202402020915_x.txt",
        ] {
            assert!(migration_name(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn release_skips_the_engine_directory() {
        assert_eq!(
            release_of("migration/iD13/oracle/x.sql").as_deref(),
            Some("iD13")
        );
        assert_eq!(
            release_of("migration/iD13/postgresql/x.sql").as_deref(),
            Some("iD13")
        );
        assert_eq!(release_of("db/changes/x.sql").as_deref(), Some("changes"));
        assert_eq!(release_of("x.sql"), None);
    }

    #[test]
    fn oracle_alter_table_forms() {
        let c = changes("ALTER TABLE AD_WF_Responsible ADD Override_ID NUMBER(10) DEFAULT NULL");
        assert_eq!(c[0].operation, "add_column");
        assert_eq!(c[0].table.as_deref(), Some("AD_WF_Responsible"));
        let col = c[0].column.as_ref().unwrap();
        assert_eq!(
            (
                col.name.as_str(),
                col.data_type.as_deref(),
                col.default.as_deref()
            ),
            ("Override_ID", Some("NUMBER(10)"), Some("NULL"))
        );
        assert_eq!(col.not_null, None);

        let c =
            changes("ALTER TABLE C_PaySelectionCheck ADD WriteOffAmt NUMBER DEFAULT 0 NOT NULL");
        let col = c[0].column.as_ref().unwrap();
        assert_eq!(
            (col.default.as_deref(), col.not_null),
            (Some("0"), Some(true))
        );

        let c = changes("ALTER TABLE a MODIFY created DEFAULT SysDate");
        let col = c[0].column.as_ref().unwrap();
        assert_eq!(
            (
                c[0].operation,
                col.data_type.as_deref(),
                col.default.as_deref()
            ),
            ("modify_column", None, Some("SysDate"))
        );

        let c = changes("ALTER TABLE X MODIFY A NOT NULL");
        let col = c[0].column.as_ref().unwrap();
        assert_eq!(
            (col.data_type.as_deref(), col.not_null),
            (None, Some(true)),
            "never a type named NOT"
        );

        let c = changes(
            "ALTER TABLE M_ATTRIBUTESET MODIFY (LOTCHAREOVERWRITE CHAR(1 CHAR), SERNO VARCHAR2(36 CHAR) NULL)",
        );
        assert_eq!(c.len(), 2);
        assert_eq!(c[1].column.as_ref().unwrap().not_null, Some(false));

        let c = changes("ALTER TABLE AD_ACCESSLOG DROP (tmp_col_1_tmp,tmp_col_2_tmp)");
        assert_eq!(
            c.iter().map(|x| x.operation).collect::<Vec<_>>(),
            ["drop_column", "drop_column"]
        );

        let c = changes(
            "ALTER TABLE AD_WF_Responsible ADD CONSTRAINT Override_ADWFResponsible FOREIGN KEY (Override_ID) REFERENCES ad_wf_responsible(ad_wf_responsible_id) DEFERRABLE INITIALLY DEFERRED",
        );
        let k = c[0].constraint.as_ref().unwrap();
        assert_eq!(
            (c[0].operation, k.kind, k.references.as_deref()),
            ("add_constraint", "foreign_key", Some("ad_wf_responsible"))
        );
        assert_eq!(k.columns, ["Override_ID"]);

        // Oracle: a foreign key with no referenced column list is the primary key.
        let c = changes(
            "ALTER TABLE W ADD CONSTRAINT C FOREIGN KEY (AD_Column_ID) REFERENCES AD_Column",
        );
        assert_eq!(
            c[0].constraint.as_ref().unwrap().references.as_deref(),
            Some("AD_Column")
        );
    }

    #[test]
    fn postgresql_alter_table_forms() {
        let c = changes(
            "ALTER TABLE ad_wf_responsible ADD COLUMN override_id NUMERIC(10) DEFAULT NULL",
        );
        assert_eq!(
            c[0].column.as_ref().unwrap().data_type.as_deref(),
            Some("NUMERIC(10)")
        );
        let c = changes(
            "ALTER TABLE t ALTER COLUMN a TYPE numeric(10,2) USING a::numeric, ALTER COLUMN b SET NOT NULL, ALTER b DROP DEFAULT",
        );
        assert_eq!(c.len(), 3);
        assert_eq!(
            c[0].column.as_ref().unwrap().data_type.as_deref(),
            Some("numeric(10,2)")
        );
        assert_eq!(c[1].column.as_ref().unwrap().not_null, Some(true));
        assert_eq!(
            c[2].column.as_ref().unwrap().default.as_deref(),
            Some("NULL")
        );
        let c = changes("ALTER TABLE IF EXISTS t RENAME COLUMN a TO b");
        assert_eq!(
            (c[0].operation, c[0].name.as_deref()),
            ("rename_column", Some("b"))
        );
    }

    #[test]
    fn create_and_drop() {
        let c = changes(
            "CREATE TABLE AD_RequiredPlugin (AD_Client_ID NUMBER(10) NOT NULL, IsActive CHAR(1) DEFAULT 'Y' CHECK (IsActive IN ('Y','N')) NOT NULL, PluginName VARCHAR2(1000 CHAR) DEFAULT NULL , CONSTRAINT AD_RequiredPlugin_UU_idx PRIMARY KEY (AD_RequiredPlugin_UU))",
        );
        assert_eq!(c[0].operation, "create_table");
        let cols: Vec<(&str, Option<&str>, Option<bool>)> = c[0]
            .columns
            .iter()
            .map(|x| (x.name.as_str(), x.default.as_deref(), x.not_null))
            .collect();
        assert_eq!(
            cols,
            [
                ("AD_Client_ID", None, Some(true)),
                ("IsActive", Some("'Y'"), Some(true)),
                ("PluginName", Some("NULL"), None)
            ]
        );
        assert_eq!(c[0].constraints[0].kind, "primary_key");
        let c = changes("CREATE UNIQUE INDEX ad_x_uu ON AD_X (AD_X_UU)");
        assert_eq!(
            (
                c[0].operation,
                c[0].table.as_deref(),
                c[0].columns[0].name.as_str()
            ),
            ("create_index", Some("AD_X"), "AD_X_UU")
        );
        assert_eq!(
            changes("DROP TABLE T CASCADE CONSTRAINTS")[0].operation,
            "drop_table"
        );
        assert_eq!(
            changes("CREATE OR REPLACE FORCE VIEW RV_X AS SELECT 1 FROM dual")[0]
                .name
                .as_deref(),
            Some("RV_X")
        );
        assert_eq!(
            classify("INSERT INTO AD_Column (a) VALUES (1)"),
            Classified::Data("AD_Column".into())
        );
        assert_eq!(
            classify("UPDATE AD_Field SET x=1"),
            Classified::Data("AD_Field".into())
        );
        assert_eq!(classify("SELECT 1 FROM dual"), Classified::Other);
        let c = changes(
            "INSERT INTO t_alter_column values('pa_documentstatus','Name_PrintColor_ID','NUMERIC(10)',null,'NULL')",
        );
        let col = c[0].column.as_ref().unwrap();
        assert_eq!(
            (c[0].operation, c[0].table.as_deref()),
            ("modify_column", Some("pa_documentstatus"))
        );
        assert_eq!(
            (
                col.data_type.as_deref(),
                col.not_null,
                col.default.as_deref()
            ),
            (Some("NUMERIC(10)"), None, Some("NULL"))
        );
        let c = changes("INSERT INTO t_alter_column values('t','c',null,'NOT NULL',null)");
        assert_eq!(c[0].column.as_ref().unwrap().not_null, Some(true));
    }

    #[test]
    fn unknown_alter_operations_are_counted_not_guessed() {
        assert!(
            matches!(classify("ALTER TABLE t ENABLE ROW MOVEMENT"), Classified::Changes(c, 1) if c.is_empty())
        );
    }

    const SCRIPT: &str = "SET SQLBLANKLINES ON
SET DEFINE OFF

-- 02-Feb-2024, 8:59:49 PM IST
INSERT INTO AD_Element (AD_Element_ID,ColumnName) VALUES (203907,'Override_ID')
;

-- 02-Feb-2024, 9:01:29 PM IST
ALTER TABLE AD_WF_Responsible ADD Override_ID NUMBER(10) DEFAULT NULL
;

ALTER TABLE AD_WF_Responsible ADD CONSTRAINT Override_ADWFResponsible FOREIGN KEY (Override_ID) REFERENCES ad_wf_responsible(ad_wf_responsible_id) DEFERRABLE INITIALLY DEFERRED
;
";

    #[test]
    fn a_script_becomes_a_change_set() {
        let path = "migration/iD13/oracle/202402020915_IDEMPIERE-6028.sql";
        let (g, stats) = recover_schema_history(path, SCRIPT, "oracle", Some("x"));
        assert_eq!(stats.changes, 2);
        assert_eq!(stats.data_statements, 1);
        let set = g
            .objects
            .iter()
            .find(|o| o.kind == ObjectKind::Custom(CHANGE_SET.into()))
            .unwrap();
        assert_eq!(set.name, "2024-02-02T09:15 IDEMPIERE-6028 (oracle)");
        assert_eq!(set.properties["release"], "iD13");
        assert_eq!(set.properties["engine"], "oracle");
        assert_eq!(set.properties["applied_at"], "2024-02-02T09:15");
        assert_eq!(set.properties["data_tables"], json!({"AD_Element": 1}));
        let changes: Vec<&KirObject> = g
            .objects
            .iter()
            .filter(|o| o.kind == ObjectKind::Custom(CHANGE.into()))
            .collect();
        assert_eq!(changes[0].name, "add_column AD_WF_Responsible.Override_ID");
        assert_eq!(changes[0].properties["line"], 9);
        assert_eq!(changes[0].properties["tickets"], json!(["IDEMPIERE-6028"]));
        assert_eq!(
            changes[1].name,
            "add_constraint AD_WF_Responsible.Override_ADWFResponsible"
        );
        // File → set → changes.
        assert_eq!(
            g.relationships
                .iter()
                .filter(|r| r.kind == RelationshipKind::Contains)
                .count(),
            3
        );
        let again = recover_schema_history(path, SCRIPT, "oracle", Some("x")).0;
        assert_eq!(
            g.objects.iter().map(|o| o.id).collect::<Vec<_>>(),
            again.objects.iter().map(|o| o.id).collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_postgresql_twin_reads_the_same() {
        let pg =
            "ALTER TABLE AD_WF_Responsible ADD COLUMN Override_ID NUMERIC(10) DEFAULT NULL \n;\n";
        let (_, stats) = recover_schema_history(
            "migration/iD13/postgresql/202402020915_IDEMPIERE-6028.sql",
            pg,
            "postgres",
            None,
        );
        assert_eq!(stats.by_operation.get("add_column"), Some(&1));
    }

    /// RFC 0210 §5.4 on a real corpus: `EKOS_MIGRATION_CORPUS=<dir> cargo test -p ekos-recovery
    /// schema_history_corpus -- --ignored --nocapture`. The engine comes from the directory name.
    #[test]
    #[ignore]
    fn schema_history_corpus() {
        let Ok(root) = std::env::var("EKOS_MIGRATION_CORPUS") else {
            return;
        };
        let mut by_engine: BTreeMap<String, SchemaHistoryStats> = BTreeMap::new();
        let mut sets = 0;
        let mut unrecognized: BTreeMap<String, usize> = BTreeMap::new();
        for e in walkdir::WalkDir::new(&root).into_iter().flatten() {
            let path = e.path().to_string_lossy().replace('\\', "/");
            let engine = if path.contains("/oracle/") {
                "oracle"
            } else if path.contains("/postgresql/") {
                "postgres"
            } else {
                continue;
            };
            let Ok(bytes) = std::fs::read(e.path()) else {
                continue;
            };
            let sql = String::from_utf8_lossy(&bytes);
            let (g, st) = recover_schema_history(&path, &sql, engine, None);
            if g.objects.is_empty() {
                continue;
            }
            sets += 1;
            for (start, end) in statement_ranges(&sql, engine == "oracle") {
                let text = neutralize_quotes(&sql[start..end]);
                if let Classified::Changes(_, n) = classify(&text)
                    && n > 0
                {
                    let head: String = text
                        .split_whitespace()
                        .take(5)
                        .collect::<Vec<_>>()
                        .join(" ");
                    *unrecognized
                        .entry(head.chars().take(90).collect())
                        .or_default() += 1;
                }
            }
            let t = by_engine.entry(engine.into()).or_default();
            t.changes += st.changes;
            t.data_statements += st.data_statements;
            t.other_statements += st.other_statements;
            t.unrecognized += st.unrecognized;
            for (k, v) in st.by_operation {
                *t.by_operation.entry(k).or_default() += v;
            }
        }
        println!("change sets {sets}");
        for (engine, t) in &by_engine {
            println!(
                "{engine}: {} changes, {} data statements, {} other, {} unrecognized\n  {:?}",
                t.changes, t.data_statements, t.other_statements, t.unrecognized, t.by_operation
            );
        }
        let mut u: Vec<_> = unrecognized.into_iter().collect();
        u.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
        for (h, n) in u.iter().take(15) {
            println!("{n:5} {h}");
        }
    }

    #[test]
    fn other_files_are_not_history() {
        let (g, _) = recover_schema_history("db/schema.sql", SCRIPT, "oracle", None);
        assert!(g.objects.is_empty());
        assert!(!SchemaHistoryPass::applies_to("db/schema.sql"));
        assert!(SchemaHistoryPass::applies_to(
            "migration/iD13/oracle/202402020915_IDEMPIERE-6028.sql"
        ));
    }
}
