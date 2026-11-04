//! What a model is shown for one question (RFC 0240 §2.4) and the deterministic measures of it
//! (§2.5): the raw DDL, the EKOS pack rendered from a project's compiled ledger, question-time
//! value lookup, and schema-linking / join-path / value-hit measures read from the gold SQL.

use super::leakage::normalise;
use super::sqlexec::Db;
use anyhow::Result;
use ekos_kir::{KirObject, KirRelationship, ObjectKind, RelationshipKind};
use serde::{Deserialize, Serialize};
use sqlparser::ast::{Expr, Query, TableFactor, Value, Visit, Visitor};
use sqlparser::dialect::SQLiteDialect;
use sqlparser::parser::Parser;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::ops::ControlFlow;

/// `"name"` / `` `name` `` / `[name]` → `name`.
pub fn unquote(name: &str) -> String {
    let t = name.trim();
    let quoted = (t.starts_with('"') && t.ends_with('"'))
        || (t.starts_with('`') && t.ends_with('`'))
        || (t.starts_with('[') && t.ends_with(']'));
    if quoted && t.len() >= 2 {
        t[1..t.len() - 1].to_string()
    } else {
        t.to_string()
    }
}

fn quote(name: &str) -> String {
    format!("`{}`", name.replace('`', "``"))
}

// ── The database's own schema ────────────────────────────────────────────────────────────────

/// Tables and columns as SQLite reports them — the truth the measures are checked against.
#[derive(Debug, Clone, Default)]
pub struct SchemaInfo {
    /// Lower-case table name → its columns as `(name, declared type, pk position)`.
    pub tables: BTreeMap<String, Vec<(String, String, i64)>>,
    /// Original-case table names, in file order.
    pub table_names: Vec<String>,
}

impl SchemaInfo {
    pub fn read(db: &Db) -> Result<SchemaInfo> {
        let conn = db.connection();
        let mut names: Vec<String> = Vec::new();
        let mut stmt = conn.prepare(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' \
             ORDER BY rowid",
        )?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        for r in rows {
            names.push(r?);
        }
        let mut tables = BTreeMap::new();
        for t in &names {
            let mut stmt = conn.prepare(&format!("PRAGMA table_info({})", quote(t)))?;
            let cols = stmt
                .query_map([], |r| {
                    Ok((
                        r.get::<_, String>(1)?,
                        r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                        r.get::<_, i64>(5)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            tables.insert(t.to_lowercase(), cols);
        }
        Ok(SchemaInfo {
            tables,
            table_names: names,
        })
    }

    pub fn has_table(&self, t: &str) -> bool {
        self.tables.contains_key(&t.to_lowercase())
    }

    pub fn has_column(&self, c: &str) -> bool {
        let c = c.to_lowercase();
        self.tables
            .values()
            .any(|cols| cols.iter().any(|(n, _, _)| n.to_lowercase() == c))
    }
}

/// Cell A/B: the `CREATE TABLE` statements exactly as stored in the database file.
pub fn raw_ddl(db: &Db) -> Result<String> {
    let mut stmt = db.connection().prepare(
        "SELECT sql FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' \
         AND sql IS NOT NULL ORDER BY rowid",
    )?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    let mut out = String::new();
    for r in rows {
        out.push_str(r?.trim());
        out.push_str(";\n\n");
    }
    Ok(out)
}

// ── The EKOS pack ────────────────────────────────────────────────────────────────────────────

/// A column as the compiled ledger records it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PackColumn {
    pub name: String,
    pub data_type: String,
    pub primary_key: bool,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PackTable {
    pub name: String,
    pub description: Option<String>,
    pub columns: Vec<PackColumn>,
}

/// Cell C/D: one database's compiled knowledge — `Table` objects with their columns and
/// descriptions, and the `ForeignKey` edges as join paths. Built from the ledger only.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DbPack {
    pub tables: Vec<PackTable>,
    /// `a.x = b.y`, one per foreign key.
    pub joins: Vec<String>,
}

impl DbPack {
    pub fn from_ledger(objects: &[KirObject], relationships: &[KirRelationship]) -> DbPack {
        let mut tables: Vec<PackTable> = objects
            .iter()
            .filter(|o| o.kind == ObjectKind::Table)
            .map(|o| PackTable {
                name: unquote(&o.name),
                description: o
                    .properties
                    .get("description")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
                columns: o
                    .properties
                    .get("columns")
                    .and_then(|v| v.as_array())
                    .map(|cols| cols.iter().filter_map(pack_column).collect())
                    .unwrap_or_default(),
            })
            .collect();
        tables.sort_by(|a, b| a.name.cmp(&b.name));
        tables.dedup_by(|a, b| a.name == b.name);
        let mut joins: Vec<String> = relationships
            .iter()
            .filter(|r| r.kind == RelationshipKind::ForeignKey)
            .filter_map(|r| r.properties.get("fk_desc").and_then(|v| v.as_str()))
            .map(|d| d.replace(['"', '`'], "").replace(" → ", " = "))
            .collect();
        joins.sort();
        joins.dedup();
        DbPack { tables, joins }
    }

    pub fn table_names(&self) -> BTreeSet<String> {
        self.tables.iter().map(|t| t.name.to_lowercase()).collect()
    }

    pub fn column_names(&self) -> BTreeSet<String> {
        self.tables
            .iter()
            .flat_map(|t| t.columns.iter().map(|c| c.name.to_lowercase()))
            .collect()
    }

    /// The pack as schema text: DDL-shaped, each column annotated with what the ledger knows.
    pub fn render(&self) -> String {
        self.render_with(None, Shown::default())
    }

    /// The pack with each column's profile (RFC 0241) appended to its comment; `examples`
    /// false leaves the example values out (one of the varied renderings).
    pub fn render_with(&self, profile: Option<&Profile>, shown: Shown) -> String {
        let mut out = String::new();
        for t in &self.tables {
            if let Some(d) = &t.description {
                out.push_str(&format!("-- {}\n", one_line(d)));
            }
            if let Some(tp) = profile.and_then(|p| p.table(&t.name)) {
                out.push_str(&format!("-- {} rows\n", tp.rows));
            }
            out.push_str(&format!("CREATE TABLE {} (\n", quote(&t.name)));
            let pk: Vec<&str> = t
                .columns
                .iter()
                .filter(|c| c.primary_key)
                .map(|c| c.name.as_str())
                .collect();
            for (i, c) in t.columns.iter().enumerate() {
                let comma = if i + 1 < t.columns.len() || !pk.is_empty() {
                    ","
                } else {
                    ""
                };
                out.push_str(&format!("    {} {}{comma}", quote(&c.name), c.data_type));
                let mut notes = Vec::new();
                if let Some(d) = &c.description {
                    notes.push(one_line(d));
                }
                if let Some(cp) = profile.and_then(|p| p.column(&t.name, &c.name)) {
                    notes.push(cp.describe(shown));
                }
                if !notes.is_empty() {
                    out.push_str(&format!(" -- {}", notes.join(" | ")));
                }
                out.push('\n');
            }
            if !pk.is_empty() {
                let pk: Vec<String> = pk.iter().map(|c| quote(c)).collect();
                out.push_str(&format!("    PRIMARY KEY ({})\n", pk.join(", ")));
            }
            out.push_str(");\n\n");
        }
        if !self.joins.is_empty() {
            out.push_str("-- Join paths (foreign keys):\n");
            for j in &self.joins {
                out.push_str(&format!("--   {j}\n"));
            }
        }
        out
    }
}

fn pack_column(v: &serde_json::Value) -> Option<PackColumn> {
    Some(PackColumn {
        name: unquote(v.get("name")?.as_str()?),
        data_type: v
            .get("data_type")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string(),
        primary_key: v.get("primary_key").and_then(|p| p.as_bool()) == Some(true),
        description: v
            .get("description")
            .and_then(|d| d.as_str())
            .map(str::to_string),
    })
}

fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

// ── Question-time value lookup ───────────────────────────────────────────────────────────────

/// The most distinct values a column may have and still be indexed for lookup.
pub const MAX_DISTINCT: i64 = 5000;
/// Longest value indexed.
pub const MAX_VALUE_CHARS: i64 = 80;
/// Most matches shown per question, and per column.
pub const MAX_MATCHES: usize = 12;
const MAX_PER_COLUMN: usize = 3;
/// Words a question uses for its own phrasing; a value spelled like one ("What" is a card name in
/// BIRD's card_games) is not evidence the question names it.
const STOP_VALUES: &[&str] = &[
    "yes", "the", "and", "none", "null", "other", "all", "not", "for", "with", "true", "false",
    "unknown", "total", "average", "number", "what", "which", "who", "whom", "whose", "how",
    "when", "where", "why", "list", "name", "names", "give", "show", "find", "among", "many",
    "much", "most", "least", "more", "less", "each", "every", "any", "are", "was", "were", "has",
    "have", "had", "did", "does", "this", "that", "these", "those", "there", "their", "from",
    "into", "than", "then", "only", "also", "both", "per", "between", "please", "need", "want",
];

/// Distinct text values per column, built once by `prepare` from the database file. Not stored
/// in any ledger: value lookup is a question-time step.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ValueIndex {
    /// `(table, column, values)`.
    pub columns: Vec<(String, String, Vec<String>)>,
}

/// A database value whose text the question mentions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValueMatch {
    pub table: String,
    pub column: String,
    pub value: String,
}

impl ValueIndex {
    pub fn build(db: &Db, schema: &SchemaInfo) -> Result<ValueIndex> {
        let conn = db.connection();
        let mut columns = Vec::new();
        for t in &schema.table_names {
            for (c, ty, _) in &schema.tables[&t.to_lowercase()] {
                let ty = ty.to_uppercase();
                let textual =
                    ty.is_empty() || ["CHAR", "TEXT", "CLOB"].iter().any(|k| ty.contains(k));
                if !textual {
                    continue;
                }
                let (qt, qc) = (quote(t), quote(c));
                let distinct: i64 =
                    conn.query_row(&format!("SELECT COUNT(DISTINCT {qc}) FROM {qt}"), [], |r| {
                        r.get(0)
                    })?;
                if distinct == 0 || distinct > MAX_DISTINCT {
                    continue;
                }
                let mut stmt = conn.prepare(&format!(
                    "SELECT DISTINCT {qc} FROM {qt} WHERE typeof({qc}) = 'text' \
                     AND length({qc}) BETWEEN 1 AND {MAX_VALUE_CHARS} ORDER BY {qc}"
                ))?;
                let values = stmt
                    .query_map([], |r| r.get::<_, String>(0))?
                    .collect::<Result<Vec<_>, _>>()?;
                if !values.is_empty() {
                    columns.push((t.clone(), c.clone(), values));
                }
            }
        }
        Ok(ValueIndex { columns })
    }

    /// Values the question names, longest first.
    pub fn lookup(&self, question: &str) -> Vec<ValueMatch> {
        let q = format!(" {} ", normalise(question));
        let mut found: Vec<(usize, usize, ValueMatch)> = Vec::new();
        for (table, column, values) in &self.columns {
            let mut hits: Vec<&String> = values
                .iter()
                .filter(|v| {
                    let n = normalise(v);
                    n.len() >= 3
                        && !STOP_VALUES.contains(&n.as_str())
                        && (n.chars().any(char::is_alphabetic) || n.len() >= 4)
                        && q.contains(&format!(" {n} "))
                })
                .collect();
            hits.sort_by_key(|v| std::cmp::Reverse(v.len()));
            for v in hits.into_iter().take(MAX_PER_COLUMN) {
                found.push((
                    v.len(),
                    values.len(),
                    ValueMatch {
                        table: table.clone(),
                        column: column.clone(),
                        value: v.clone(),
                    },
                ));
            }
        }
        found.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        found.into_iter().take(MAX_MATCHES).map(|f| f.2).collect()
    }
}

/// The lookup result as prompt text.
pub fn render_values(matches: &[ValueMatch]) -> String {
    if matches.is_empty() {
        return String::new();
    }
    let mut out = String::from("-- Values in the database that the question mentions:\n");
    for m in matches {
        out.push_str(&format!(
            "--   {}.{} = '{}'\n",
            quote(&m.table),
            quote(&m.column),
            m.value.replace('\'', "''")
        ));
    }
    out
}

// ── Column profiles (RFC 0241) ───────────────────────────────────────────────────────────────

/// Share of a text column's distinct values that must hold a comma for it to be read as a list
/// ("Artifact,Creature"); prose columns are excluded by length.
const LIST_SHARE: f64 = 0.05;
const LIST_MAX_CHARS: usize = 60;
const EXAMPLE_CHARS: usize = 40;

/// What a column's profile line shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shown {
    pub examples: bool,
    /// RFC 0241 profile v2: the count of empty-string values, and that they mean "missing".
    pub empties: bool,
}

impl Default for Shown {
    fn default() -> Self {
        Self {
            examples: true,
            empties: false,
        }
    }
}

/// The profile file's format; `prepare` rebuilds a file older than this.
pub const PROFILE_VERSION: u32 = 2;

/// What the data says about one column.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ColumnProfile {
    pub nulls: u64,
    /// Text values that are empty or blank: a second way a value is missing ("no Q3 time" is
    /// `''` in BIRD's formula_1, not NULL). Profile v2.
    #[serde(default)]
    pub empty: u64,
    pub distinct: u64,
    pub unique: bool,
    pub examples: Vec<String>,
    /// Values are comma-separated lists: match an element with `LIKE`, never `=`.
    pub list_valued: bool,
}

impl ColumnProfile {
    /// `12% null; 6412 distinct; examples: 'a', 'b'` (or `unique`, `list of values: …`).
    pub fn describe(&self, shown: Shown) -> String {
        let mut parts = Vec::new();
        if self.nulls > 0 {
            parts.push(format!("{} null", self.nulls));
        }
        if shown.empties && self.empty > 0 {
            parts.push(format!(
                "{} empty strings ('' also means missing: exclude with <> '' as well as IS NOT NULL)",
                self.empty
            ));
        }
        parts.push(if self.unique {
            format!("unique ({} values)", self.distinct)
        } else {
            format!("{} distinct", self.distinct)
        });
        if self.list_valued {
            parts.push("comma-separated list: match one element with LIKE '%x%'".into());
        }
        if shown.examples && !self.examples.is_empty() {
            let ex: Vec<String> = self
                .examples
                .iter()
                .map(|e| format!("'{}'", e.replace('\'', "''")))
                .collect();
            parts.push(format!("e.g. {}", ex.join(", ")));
        }
        parts.join("; ")
    }
}

/// A column of comma-separated lists ("Artifact,Creature"): at least [`LIST_SHARE`] of its
/// distinct values hold a comma, values are short, and most elements of those values also occur
/// as values on their own — which is what tells a list from a name with a comma in it
/// ("Kaiso, Memory of Loyalty") or a serialised object.
fn is_list_column(distinct_values: &[String]) -> bool {
    if distinct_values.is_empty()
        || distinct_values
            .iter()
            .any(|v| v.chars().count() > LIST_MAX_CHARS)
    {
        return false;
    }
    let with_comma: Vec<&String> = distinct_values.iter().filter(|v| v.contains(',')).collect();
    if (with_comma.len() as f64) < LIST_SHARE * distinct_values.len() as f64 {
        return false;
    }
    let single: BTreeSet<&str> = distinct_values
        .iter()
        .filter(|v| !v.contains(','))
        .map(|v| v.trim())
        .collect();
    let elements: Vec<&str> = with_comma
        .iter()
        .flat_map(|v| v.split(','))
        .map(str::trim)
        .collect();
    let known = elements.iter().filter(|e| single.contains(*e)).count();
    known * 2 >= elements.len()
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TableProfile {
    pub rows: u64,
    /// Lower-case column name → profile.
    pub columns: BTreeMap<String, ColumnProfile>,
}

/// Row counts and column profiles of one database, built by `prepare` from the database file
/// and kept beside the value index — never in a ledger.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    /// [`PROFILE_VERSION`] when built; files from before versioning read as 1.
    #[serde(default = "profile_v1")]
    pub version: u32,
    /// Lower-case table name → profile.
    pub tables: BTreeMap<String, TableProfile>,
}

fn profile_v1() -> u32 {
    1
}

impl Profile {
    pub fn build(db: &Db, schema: &SchemaInfo) -> Result<Profile> {
        let conn = db.connection();
        let mut tables = BTreeMap::new();
        for t in &schema.table_names {
            let qt = quote(t);
            let rows: i64 =
                conn.query_row(&format!("SELECT COUNT(*) FROM {qt}"), [], |r| r.get(0))?;
            let mut columns = BTreeMap::new();
            for (c, _, _) in &schema.tables[&t.to_lowercase()] {
                let qc = quote(c);
                let (nulls, distinct, empty): (i64, i64, i64) = conn.query_row(
                    &format!(
                        "SELECT COUNT(*) - COUNT({qc}), COUNT(DISTINCT {qc}), \
                         COALESCE(SUM(typeof({qc}) = 'text' AND trim({qc}) = ''), 0) FROM {qt}"
                    ),
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )?;
                let mut stmt = conn.prepare(&format!(
                    "SELECT DISTINCT CAST({qc} AS TEXT) FROM {qt} WHERE {qc} IS NOT NULL LIMIT 2000"
                ))?;
                let sample: Vec<String> = stmt
                    .query_map([], |r| r.get::<_, Option<String>>(0))?
                    .filter_map(|v| v.ok().flatten())
                    .collect();
                let list_valued = is_list_column(&sample);
                let examples = sample
                    .iter()
                    .filter(|v| !v.trim().is_empty())
                    .take(3)
                    .map(|v| v.chars().take(EXAMPLE_CHARS).collect())
                    .collect();
                columns.insert(
                    c.to_lowercase(),
                    ColumnProfile {
                        nulls: nulls as u64,
                        empty: empty as u64,
                        distinct: distinct as u64,
                        unique: distinct > 0 && distinct == rows - nulls && nulls == 0,
                        examples,
                        list_valued,
                    },
                );
            }
            tables.insert(
                t.to_lowercase(),
                TableProfile {
                    rows: rows as u64,
                    columns,
                },
            );
        }
        Ok(Profile {
            version: PROFILE_VERSION,
            tables,
        })
    }

    pub fn table(&self, t: &str) -> Option<&TableProfile> {
        self.tables.get(&t.to_lowercase())
    }

    pub fn column(&self, t: &str, c: &str) -> Option<&ColumnProfile> {
        self.table(t)?.columns.get(&c.to_lowercase())
    }

    /// The profiles as a comment block, to follow raw DDL.
    pub fn render_block(&self, schema: &SchemaInfo, shown: Shown) -> String {
        let mut out = String::from("-- Column profiles:\n");
        for t in &schema.table_names {
            let Some(tp) = self.table(t) else { continue };
            out.push_str(&format!("-- {} ({} rows)\n", quote(t), tp.rows));
            for (c, _, _) in &schema.tables[&t.to_lowercase()] {
                if let Some(cp) = tp.columns.get(&c.to_lowercase()) {
                    out.push_str(&format!("--   {}: {}\n", quote(c), cp.describe(shown)));
                }
            }
        }
        out
    }
}

// ── Value lookup v2 (RFC 0241) ───────────────────────────────────────────────────────────────

/// One value the question names, with every column it occurs in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValueGroup {
    pub value: String,
    /// `(table, column, list_valued)`.
    pub columns: Vec<(String, String, bool)>,
}

impl ValueIndex {
    /// Value lookup v2: elements of list columns are matched one by one; a one-word value counts
    /// only when the question spells it exactly as stored (not as its first word) or quotes it;
    /// one group per value; rarest (fewest columns) first, then longest.
    pub fn lookup_v2(&self, question: &str, profile: Option<&Profile>) -> Vec<ValueGroup> {
        let q = format!(" {} ", normalise(question));
        let mut groups: BTreeMap<String, Vec<(String, String, bool)>> = BTreeMap::new();
        for (table, column, values) in &self.columns {
            let list = profile
                .and_then(|p| p.column(table, column))
                .is_some_and(|c| c.list_valued);
            let mut candidates: BTreeSet<&str> = BTreeSet::new();
            for v in values {
                if list {
                    candidates.extend(v.split(',').map(str::trim));
                } else {
                    candidates.insert(v.as_str());
                }
            }
            for v in candidates {
                let n = normalise(v);
                if n.len() < 3
                    || STOP_VALUES.contains(&n.as_str())
                    || !(n.chars().any(char::is_alphabetic) || n.len() >= 4)
                    || !q.contains(&format!(" {n} "))
                {
                    continue;
                }
                if !n.contains(' ') && !named_in(question, v) {
                    continue;
                }
                let cols = groups.entry(v.to_string()).or_default();
                if !cols.iter().any(|(t, c, _)| t == table && c == column) {
                    cols.push((table.clone(), column.clone(), list));
                }
            }
        }
        let mut out: Vec<ValueGroup> = groups
            .into_iter()
            .map(|(value, columns)| ValueGroup { value, columns })
            .collect();
        out.sort_by(|a, b| {
            a.columns
                .len()
                .cmp(&b.columns.len())
                .then(b.value.len().cmp(&a.value.len()))
                .then(a.value.cmp(&b.value))
        });
        // The same text in several spellings ('From the Vault' / 'From The Vault') is one idea.
        let mut seen = BTreeSet::new();
        out.retain(|g| seen.insert(g.value.to_lowercase()));
        out.truncate(MAX_MATCHES);
        out
    }
}

/// Whether the question names a one-word value: spelled exactly as stored (case included)
/// somewhere other than as its first word, or inside quotes, or a value with digits.
fn named_in(question: &str, value: &str) -> bool {
    let lower_q = question.to_lowercase();
    let lower_v = value.to_lowercase();
    let mut from = 0;
    while let Some(pos) = lower_q[from..].find(&lower_v) {
        let start = from + pos;
        let end = start + lower_v.len();
        from = end;
        let before = question[..start].chars().next_back();
        let after = question[end..].chars().next();
        let boundary = |c: Option<char>| c.is_none_or(|c| !c.is_alphanumeric());
        if !boundary(before) || !boundary(after) {
            continue;
        }
        if matches!(before, Some('\'') | Some('"')) && matches!(after, Some('\'') | Some('"')) {
            return true;
        }
        let first_word = question[..start].trim().is_empty()
            || question[..start].trim_end().ends_with(['.', '?', '!']);
        if question.get(start..end) == Some(value) && !first_word {
            return true;
        }
        if value.chars().any(|c| c.is_ascii_digit()) {
            return true;
        }
    }
    false
}

/// Value lookup v2 as prompt text.
pub fn render_value_groups(groups: &[ValueGroup]) -> String {
    if groups.is_empty() {
        return String::new();
    }
    let mut out = String::from("-- Values in the database that the question mentions:\n");
    for g in groups {
        let cols: Vec<String> = g
            .columns
            .iter()
            .map(|(t, c, list)| {
                if *list {
                    format!(
                        "{}.{} (list column: {} LIKE '%{}%')",
                        quote(t),
                        quote(c),
                        quote(c),
                        g.value.replace('\'', "''")
                    )
                } else {
                    format!("{}.{}", quote(t), quote(c))
                }
            })
            .collect();
        out.push_str(&format!(
            "--   '{}' in {}\n",
            g.value.replace('\'', "''"),
            cols.join(", ")
        ));
    }
    out
}

// ── Reading SQL ──────────────────────────────────────────────────────────────────────────────

/// The SQL a model answered with: the last ```sql block, else the last code block, else the
/// text from the first `SELECT`/`WITH`. Trailing semicolons are dropped.
pub fn extract_sql(answer: &str) -> Option<String> {
    let blocks: Vec<&str> = answer.split("```").collect();
    let mut best: Option<String> = None;
    for (i, b) in blocks.iter().enumerate() {
        if i % 2 == 1 {
            let body = b
                .strip_prefix("sqlite")
                .or_else(|| b.strip_prefix("sql"))
                .or_else(|| b.strip_prefix("SQL"))
                .unwrap_or(b);
            if !body.trim().is_empty() {
                best = Some(body.to_string());
            }
        }
    }
    let sql = match best {
        Some(b) => b,
        None => {
            let upper = answer.to_uppercase();
            let start = ["SELECT", "WITH"]
                .iter()
                .filter_map(|k| upper.find(k))
                .min()?;
            answer[start..].to_string()
        }
    };
    let sql = strip_comment_lines(&sql);
    let sql = sql.trim().trim_end_matches(';').trim().to_string();
    (!sql.is_empty()).then_some(sql)
}

fn strip_comment_lines(sql: &str) -> String {
    sql.lines()
        .filter(|l| !l.trim_start().starts_with("--"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Tables, columns, join pairs and string literals one SQL statement names (lower-cased).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SqlShape {
    pub tables: BTreeSet<String>,
    pub columns: BTreeSet<String>,
    /// Unordered pairs of tables joined by an equality between their columns.
    pub joins: BTreeSet<(String, String)>,
    pub literals: BTreeSet<String>,
}

#[derive(Default)]
struct ShapeVisitor {
    tables: BTreeSet<String>,
    ctes: BTreeSet<String>,
    alias: HashMap<String, String>,
    columns: BTreeSet<String>,
    qualified_eq: Vec<(String, String)>,
    literals: BTreeSet<String>,
}

impl Visitor for ShapeVisitor {
    type Break = ();

    fn pre_visit_query(&mut self, q: &Query) -> ControlFlow<()> {
        if let Some(with) = &q.with {
            for cte in &with.cte_tables {
                self.ctes.insert(cte.alias.name.value.to_lowercase());
            }
        }
        ControlFlow::Continue(())
    }

    fn pre_visit_table_factor(&mut self, f: &TableFactor) -> ControlFlow<()> {
        if let TableFactor::Table { name, alias, .. } = f {
            let table = name
                .0
                .last()
                .map(|i| i.value.to_lowercase())
                .unwrap_or_default();
            self.tables.insert(table.clone());
            self.alias.insert(table.clone(), table.clone());
            if let Some(a) = alias {
                self.alias.insert(a.name.value.to_lowercase(), table);
            }
        }
        ControlFlow::Continue(())
    }

    fn pre_visit_expr(&mut self, e: &Expr) -> ControlFlow<()> {
        match e {
            Expr::Identifier(i) => {
                self.columns.insert(i.value.to_lowercase());
            }
            Expr::CompoundIdentifier(parts) => {
                if let Some(c) = parts.last() {
                    self.columns.insert(c.value.to_lowercase());
                }
            }
            Expr::BinaryOp {
                left,
                op: sqlparser::ast::BinaryOperator::Eq,
                right,
            } => {
                if let (Expr::CompoundIdentifier(l), Expr::CompoundIdentifier(r)) =
                    (left.as_ref(), right.as_ref())
                    && l.len() >= 2
                    && r.len() >= 2
                {
                    self.qualified_eq.push((
                        l[l.len() - 2].value.to_lowercase(),
                        r[r.len() - 2].value.to_lowercase(),
                    ));
                }
            }
            Expr::Value(Value::SingleQuotedString(s)) => {
                self.literals.insert(s.clone());
            }
            _ => {}
        }
        ControlFlow::Continue(())
    }
}

/// `None` when the statement does not parse as SQLite.
pub fn sql_shape(sql: &str) -> Option<SqlShape> {
    let stmts = Parser::parse_sql(&SQLiteDialect {}, sql).ok()?;
    let mut v = ShapeVisitor::default();
    let _ = stmts.visit(&mut v);
    let resolve = |q: &str| v.alias.get(q).cloned().unwrap_or_else(|| q.to_string());
    let joins = v
        .qualified_eq
        .iter()
        .map(|(a, b)| {
            let (a, b) = (resolve(a), resolve(b));
            if a <= b { (a, b) } else { (b, a) }
        })
        .filter(|(a, b)| a != b)
        .collect();
    let tables = v.tables.difference(&v.ctes).cloned().collect();
    Some(SqlShape {
        tables,
        columns: v.columns,
        joins,
        literals: v.literals,
    })
}

/// Join pairs the context offers: from `a.x = b.y` lines.
pub fn join_pairs(joins: &[String]) -> BTreeSet<(String, String)> {
    joins
        .iter()
        .filter_map(|j| {
            let (l, r) = j.split_once(" = ")?;
            let a = l.split('.').next()?.trim().to_lowercase();
            let b = r.split('.').next()?.trim().to_lowercase();
            Some(if a <= b { (a, b) } else { (b, a) })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_the_last_sql_block() {
        let a = "Let me think.\n```sql\nSELECT 1;\n```\nBetter:\n```sql\n-- final\nSELECT a FROM t;\n```";
        assert_eq!(extract_sql(a).as_deref(), Some("SELECT a FROM t"));
        assert_eq!(
            extract_sql("The query is SELECT x FROM y;").as_deref(),
            Some("SELECT x FROM y")
        );
        assert_eq!(extract_sql("I don't know."), None);
    }

    #[test]
    fn the_shape_of_a_join_resolves_aliases_and_skips_ctes() {
        let s = sql_shape(
            "WITH c AS (SELECT 1 AS x) SELECT T1.name FROM customers AS T1 \
             INNER JOIN orders AS T2 ON T1.id = T2.customer_id WHERE T2.city = 'Paris'",
        )
        .unwrap();
        assert_eq!(
            s.tables,
            ["customers", "orders"]
                .map(String::from)
                .into_iter()
                .collect()
        );
        assert!(s.columns.contains("customer_id") && s.columns.contains("city"));
        assert_eq!(
            s.joins,
            [("customers".to_string(), "orders".to_string())].into()
        );
        assert_eq!(s.literals, ["Paris".to_string()].into());
        assert!(sql_shape("SELEC nonsense FROM").is_none());
    }

    #[test]
    fn a_pack_reads_tables_columns_and_foreign_keys_from_the_ledger() {
        let mut t = KirObject::new("\"orders\"", ObjectKind::Table);
        t.properties.insert(
            "columns".into(),
            serde_json::json!([
                {"name": "\"id\"", "data_type": "INTEGER", "primary_key": true},
                {"name": "\"city\"", "data_type": "TEXT", "description": "delivery city"}
            ]),
        );
        let c = KirObject::new("customers", ObjectKind::Table);
        let mut fk = KirRelationship::new(RelationshipKind::ForeignKey, t.id, c.id);
        fk.properties.insert(
            "fk_desc".into(),
            serde_json::json!("\"orders\".\"customer_id\" → \"customers\".\"id\""),
        );
        let pack = DbPack::from_ledger(&[t, c], &[fk]);
        assert_eq!(pack.joins, vec!["orders.customer_id = customers.id"]);
        let text = pack.render();
        assert!(text.contains("`city` TEXT, -- delivery city"), "{text}");
        assert!(text.contains("PRIMARY KEY (`id`)"), "{text}");
        assert_eq!(
            join_pairs(&pack.joins),
            [("customers".to_string(), "orders".to_string())].into()
        );
    }

    #[test]
    fn a_profile_marks_list_columns_counts_and_uniqueness() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("p.sqlite");
        let c = rusqlite::Connection::open(&path).unwrap();
        c.execute_batch(
            "CREATE TABLE cards (id INTEGER PRIMARY KEY, name TEXT, types TEXT, text TEXT);
             INSERT INTO cards VALUES
               (1,'Fireball','Sorcery',NULL),(2,'Fireball','Sorcery',NULL),
               (3,'Ogre','Creature','A big ogre, angry and slow, who likes to smash things all day long'),
               (4,'Golem','Artifact,Creature',NULL),(5,'Elf','Creature',NULL),
               (6,'Kaiso, Memory of Loyalty','Artifact',NULL);",
        )
        .unwrap();
        drop(c);
        let db = Db::open(&path).unwrap();
        let schema = SchemaInfo::read(&db).unwrap();
        let p = Profile::build(&db, &schema).unwrap();
        assert_eq!(p.table("cards").unwrap().rows, 6);
        assert!(p.column("cards", "id").unwrap().unique);
        let name = p.column("cards", "name").unwrap();
        assert_eq!((name.distinct, name.unique), (5, false));
        // A name with a comma in it is not a list.
        assert!(!name.list_valued);
        assert!(p.column("cards", "types").unwrap().list_valued);
        // Prose with commas is not a list.
        assert!(!p.column("cards", "text").unwrap().list_valued);
        assert_eq!(p.column("cards", "text").unwrap().nulls, 5);
        let pack = DbPack {
            tables: vec![PackTable {
                name: "cards".into(),
                description: None,
                columns: vec![PackColumn {
                    name: "types".into(),
                    data_type: "TEXT".into(),
                    primary_key: false,
                    description: Some("card types".into()),
                }],
            }],
            joins: vec![],
        };
        let text = pack.render_with(Some(&p), Shown::default());
        assert!(text.contains("-- 6 rows"), "{text}");
        assert!(
            text.contains("card types | 4 distinct; comma-separated list"),
            "{text}"
        );
        assert!(text.contains("e.g. 'Sorcery'"), "{text}");
        let no_examples = Shown {
            examples: false,
            ..Shown::default()
        };
        assert!(!pack.render_with(Some(&p), no_examples).contains("e.g."));
        assert!(
            p.render_block(&schema, Shown::default())
                .contains("-- `cards` (6 rows)")
        );
    }

    #[test]
    fn profile_v2_counts_empty_strings_as_missing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("q.sqlite");
        let c = rusqlite::Connection::open(&path).unwrap();
        c.execute_batch(
            "CREATE TABLE qualifying (id INTEGER PRIMARY KEY, q3 TEXT);
             INSERT INTO qualifying VALUES (1,'1:35.554'),(2,''),(3,NULL),(4,'  '),(5,'1:36.001');",
        )
        .unwrap();
        drop(c);
        let db = Db::open(&path).unwrap();
        let schema = SchemaInfo::read(&db).unwrap();
        let p = Profile::build(&db, &schema).unwrap();
        assert_eq!(p.version, PROFILE_VERSION);
        let q3 = p.column("qualifying", "q3").unwrap();
        assert_eq!((q3.nulls, q3.empty), (1, 2));
        assert!(q3.examples.iter().all(|e| !e.trim().is_empty()), "{q3:?}");
        let v2 = Shown {
            empties: true,
            ..Shown::default()
        };
        assert!(
            q3.describe(v2)
                .contains("2 empty strings ('' also means missing")
        );
        // Profile v1 rendering is unchanged, so earlier cells stay reproducible.
        assert!(!q3.describe(Shown::default()).contains("empty"));
        // A file written before versioning reads as v1.
        let old: Profile = serde_json::from_str(r#"{"tables":{}}"#).unwrap();
        assert_eq!(old.version, 1);
    }

    #[test]
    fn value_lookup_v2_groups_splits_lists_and_wants_names() {
        let idx = ValueIndex {
            columns: vec![
                (
                    "cards".into(),
                    "types".into(),
                    vec![
                        "Artifact,Creature".into(),
                        "Creature".into(),
                        "Sorcery".into(),
                    ],
                ),
                (
                    "cards".into(),
                    "name".into(),
                    vec!["Order".into(), "Fireball".into()],
                ),
                ("sets".into(), "name".into(), vec!["Fireball".into()]),
            ],
        };
        let mut profile = Profile::default();
        profile.tables.insert(
            "cards".into(),
            TableProfile {
                rows: 3,
                columns: [(
                    "types".to_string(),
                    ColumnProfile {
                        list_valued: true,
                        ..Default::default()
                    },
                )]
                .into(),
            },
        );
        // 'creature' is not spelled as stored ('Creature'); 'Fireball' is; 'order' is not.
        let g = idx.lookup_v2(
            "In what order were the Fireball cards printed, counting each creature once?",
            Some(&profile),
        );
        assert_eq!(g.len(), 1, "{g:?}");
        assert_eq!(g[0].value, "Fireball");
        assert_eq!(g[0].columns.len(), 2, "one line, both columns");
        // Quoted list elements are found inside the list column.
        let g = idx.lookup_v2("How many 'Creature' cards are there?", Some(&profile));
        assert_eq!(g[0].value, "Creature");
        assert!(g[0].columns[0].2, "marked as a list column");
        let text = render_value_groups(&g);
        assert!(text.contains("`types` LIKE '%Creature%'"), "{text}");
        // As the first word of a question, a capital means nothing.
        assert!(idx.lookup_v2("Fireball is a card?", None).is_empty());
    }

    #[test]
    fn value_lookup_finds_whole_words_only() {
        let idx = ValueIndex {
            columns: vec![
                (
                    "client".into(),
                    "city".into(),
                    vec!["Paris".into(), "Par".into(), "New York".into()],
                ),
                ("client".into(), "flag".into(), vec!["yes".into()]),
            ],
        };
        let m = idx.lookup("How many clients in new york or Paris said yes?");
        let what = ValueIndex {
            columns: vec![("cards".into(), "faceName".into(), vec!["What".into()])],
        };
        assert!(what.lookup("What are the cards in deck a?").is_empty());
        let values: Vec<&str> = m.iter().map(|m| m.value.as_str()).collect();
        assert_eq!(values, vec!["New York", "Paris"]);
    }
}
