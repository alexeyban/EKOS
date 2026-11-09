//! What a check knows about a database: tables, columns, keys, foreign keys, and — when a
//! profile was built — per-column facts. Built from a project's ledger ([`Schema::from_ledger`])
//! or by a caller that has more (the BIRD bench adds profiles and a value index).

use ekos_kir::{KirObject, KirRelationship, ObjectKind, RelationshipKind};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// Most lines a search or a value lookup returns.
pub const MAX_LINES: usize = 15;

/// What the data says about one column. Every field is a count or a sample, never a min/max.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ColumnStats {
    pub nulls: u64,
    /// Blank text values — a second way a value is missing.
    pub empty: u64,
    pub distinct: u64,
    pub unique: bool,
    pub examples: Vec<String>,
    /// Values are comma-separated lists: match an element with `LIKE`, never `=`.
    pub list_valued: bool,
    /// Numeric values that stand for "unknown" (`0`, `-1`) with their row counts.
    pub sentinels: Vec<(String, u64)>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Column {
    pub name: String,
    pub data_type: String,
    pub primary_key: bool,
    pub description: Option<String>,
    pub stats: Option<ColumnStats>,
    /// RFC 0246: other names a semantic model gives the column.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub synonyms: Vec<String>,
    /// RFC 0246: a semantic model marks it a time axis.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_time: bool,
}

impl Column {
    /// Declared type has integer affinity (SQLite's rule: the type name contains `INT`).
    pub fn is_integer(&self) -> bool {
        self.data_type.to_uppercase().contains("INT")
    }

    /// Declared type is real-valued.
    pub fn is_real(&self) -> bool {
        let t = self.data_type.to_uppercase();
        ["REAL", "FLOA", "DOUB", "DECIMAL"]
            .iter()
            .any(|k| t.contains(k))
    }

    /// Declared type is text (or undeclared, which SQLite stores as given).
    pub fn is_text(&self) -> bool {
        let t = self.data_type.to_uppercase();
        ["CHAR", "TEXT", "CLOB", "STRING"]
            .iter()
            .any(|k| t.contains(k))
    }

    /// The profile counted NULLs or blank values in it.
    pub fn has_missing(&self) -> bool {
        self.stats
            .as_ref()
            .is_some_and(|s| s.nulls > 0 || s.empty > 0)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Table {
    pub name: String,
    /// The schema (owner) it lives in, when the source names one (a live catalog does).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    /// The `[[databases]]` entry it was read from, for a table read live.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub database: Option<String>,
    pub description: Option<String>,
    pub rows: Option<u64>,
    pub columns: Vec<Column>,
    /// RFC 0246: other names a semantic model gives the table.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub synonyms: Vec<String>,
    /// RFC 0246: unique keys a semantic model states (a primary key counts as one).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unique_keys: Vec<Vec<String>>,
}

impl Table {
    /// `schema.name` when the schema is known, else `name`: how a query should name it.
    pub fn qualified(&self) -> String {
        match &self.schema {
            Some(s) => format!("{s}.{}", self.name),
            None => self.name.clone(),
        }
    }

    pub fn column(&self, name: &str) -> Option<&Column> {
        self.columns
            .iter()
            .find(|c| c.name.eq_ignore_ascii_case(name))
    }

    /// The single-column primary key, if the table has exactly one.
    pub fn single_pk(&self) -> Option<&Column> {
        let mut pk = self.columns.iter().filter(|c| c.primary_key);
        match (pk.next(), pk.next()) {
            (Some(c), None) => Some(c),
            _ => None,
        }
    }
}

/// `from_table.from_column` references `to_table.to_column`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ForeignKey {
    pub from_table: String,
    pub from_column: String,
    pub to_table: String,
    pub to_column: String,
}

impl ForeignKey {
    pub fn render(&self) -> String {
        format!(
            "{}.{} = {}.{}",
            self.from_table, self.from_column, self.to_table, self.to_column
        )
    }
}

/// One database's schema as the checks see it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Schema {
    pub tables: Vec<Table>,
    pub foreign_keys: Vec<ForeignKey>,
    /// Distinct text values per `(table, column)`, lower-cased keys; only columns small enough to
    /// index. Empty when nothing was indexed.
    pub values: BTreeMap<(String, String), Vec<String>>,
    /// RFC 0246: joins a semantic model declares that no foreign key states, one column pair each.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub declared_joins: Vec<ForeignKey>,
    /// RFC 0246: metrics a semantic model defines, over these tables.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub metrics: Vec<Metric>,
}

/// RFC 0246: a metric a semantic model (Apache Ossie) defines, rewritten over the tables.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Metric {
    pub name: String,
    pub sql: String,
    pub tables: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub synonyms: Vec<String>,
    /// Where it is defined: `ossie:<path>`.
    pub origin: String,
}

impl Metric {
    /// `name = sql -- description (origin)`.
    pub fn render(&self) -> String {
        let mut out = format!("{} = {}", self.name, self.sql);
        let mut notes = Vec::new();
        if let Some(d) = &self.description {
            notes.push(one_line(d));
        }
        if !self.synonyms.is_empty() {
            notes.push(format!("also: {}", self.synonyms.join(", ")));
        }
        notes.push(format!("defined in {}", self.origin));
        out.push_str(&format!(" -- {}", notes.join(" | ")));
        out
    }
}

impl Schema {
    /// `Table` objects and `ForeignKey` edges from a project's ledger — the same data the BIRD
    /// pack is rendered from. No profile: the ledger holds none.
    pub fn from_ledger(objects: &[KirObject], relationships: &[KirRelationship]) -> Schema {
        let mut tables: Vec<Table> = objects
            .iter()
            .filter(|o| o.kind == ObjectKind::Table)
            .map(|o| Table {
                name: unquote(&o.name),
                schema: ["owner", "schema"]
                    .iter()
                    .find_map(|k| o.properties.get(*k).and_then(|v| v.as_str()))
                    .map(str::to_string),
                database: o
                    .properties
                    .get("database")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
                description: o
                    .properties
                    .get("description")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
                rows: None,
                columns: o
                    .properties
                    .get("columns")
                    .and_then(|v| v.as_array())
                    .map(|cols| cols.iter().filter_map(ledger_column).collect())
                    .unwrap_or_default(),
                ..Default::default()
            })
            .collect();
        tables.sort_by(|a, b| {
            (&a.name, &a.schema, &a.database).cmp(&(&b.name, &b.schema, &b.database))
        });
        tables
            .dedup_by(|a, b| a.name == b.name && a.schema == b.schema && a.database == b.database);
        let mut foreign_keys: Vec<ForeignKey> = relationships
            .iter()
            .filter(|r| r.kind == RelationshipKind::ForeignKey)
            .filter_map(|r| r.properties.get("fk_desc").and_then(|v| v.as_str()))
            .filter_map(parse_fk_desc)
            .collect();
        foreign_keys.sort();
        foreign_keys.dedup();
        Schema {
            tables,
            foreign_keys,
            ..Default::default()
        }
    }

    /// Every join the checks know: the declared foreign keys, then the joins a semantic model
    /// declares (RFC 0246).
    pub fn all_joins(&self) -> impl Iterator<Item = &ForeignKey> {
        self.foreign_keys.iter().chain(self.declared_joins.iter())
    }

    /// A join as text, marked when only a semantic model declares it.
    pub fn render_join(&self, fk: &ForeignKey) -> String {
        if self.declared_joins.contains(fk) && !self.foreign_keys.contains(fk) {
            format!("{} (declared in the semantic model)", fk.render())
        } else {
            fk.render()
        }
    }

    pub fn table(&self, name: &str) -> Option<&Table> {
        let name: String = name.split('.').map(unquote).collect::<Vec<_>>().join(".");
        let short = name.rsplit('.').next().unwrap_or(&name);
        self.tables
            .iter()
            .find(|t| t.qualified().eq_ignore_ascii_case(&name))
            .or_else(|| {
                self.tables.iter().find(|t| {
                    t.name.eq_ignore_ascii_case(&name) || t.name.eq_ignore_ascii_case(short)
                })
            })
    }

    /// Only the tables read live from `database`, when there are any (several `[[databases]]`
    /// entries can hold same-named tables); otherwise unchanged.
    pub fn for_database(mut self, database: &str) -> Schema {
        if self
            .tables
            .iter()
            .any(|t| t.database.as_deref() == Some(database))
        {
            self.tables
                .retain(|t| t.database.as_deref() == Some(database));
        }
        self
    }

    /// RFC 0250 amendment: only the named tables (bare or schema-qualified, any case), with only
    /// the joins, indexed values and metrics among them — what a caller allows its users to query.
    /// Returns the narrowed schema and the names that matched no table.
    pub fn retain_tables(mut self, names: &[String]) -> (Schema, Vec<String>) {
        let short = |n: &str| -> String {
            let n = unquote(n.rsplit('.').next().unwrap_or(n));
            n.to_ascii_lowercase()
        };
        let wanted: BTreeSet<String> = names.iter().map(|n| short(n)).collect();
        let present: BTreeSet<String> = self.tables.iter().map(|t| short(&t.name)).collect();
        let unknown = names
            .iter()
            .filter(|n| !present.contains(&short(n)))
            .cloned()
            .collect();
        let keep = |t: &str| wanted.contains(&short(t));
        self.tables.retain(|t| keep(&t.name));
        self.foreign_keys
            .retain(|fk| keep(&fk.from_table) && keep(&fk.to_table));
        self.declared_joins
            .retain(|fk| keep(&fk.from_table) && keep(&fk.to_table));
        self.values.retain(|(table, _), _| keep(table));
        self.metrics.retain(|m| m.tables.iter().all(|t| keep(t)));
        (self, unknown)
    }

    pub fn column(&self, table: &str, column: &str) -> Option<&Column> {
        self.table(table)?.column(column)
    }

    /// The values indexed for a column, if any.
    pub fn column_values(&self, table: &str, column: &str) -> Option<&Vec<String>> {
        let t = self.table(table)?;
        self.values
            .get(&(t.name.to_lowercase(), column.to_lowercase()))
    }

    /// `a.x = b.y` is a declared foreign key, in either direction.
    pub fn is_foreign_key(&self, a: (&str, &str), b: (&str, &str)) -> bool {
        let eq = |x: &str, y: &str| x.eq_ignore_ascii_case(y);
        self.all_joins().any(|fk| {
            (eq(&fk.from_table, a.0)
                && eq(&fk.from_column, a.1)
                && eq(&fk.to_table, b.0)
                && eq(&fk.to_column, b.1))
                || (eq(&fk.from_table, b.0)
                    && eq(&fk.from_column, b.1)
                    && eq(&fk.to_table, a.0)
                    && eq(&fk.to_column, a.1))
        })
    }

    /// The shortest chain of foreign keys from `from` to `to`, as join conditions.
    pub fn join_path(&self, from: &str, to: &str) -> Option<Vec<String>> {
        let (from, to) = (self.table(from)?.name.clone(), self.table(to)?.name.clone());
        if from == to {
            return Some(Vec::new());
        }
        let mut prev: BTreeMap<String, (String, &ForeignKey)> = BTreeMap::new();
        let mut seen: BTreeSet<String> = BTreeSet::from([from.to_lowercase()]);
        let mut queue = VecDeque::from([from.clone()]);
        while let Some(t) = queue.pop_front() {
            for fk in self.all_joins() {
                let next = if fk.from_table.eq_ignore_ascii_case(&t) {
                    &fk.to_table
                } else if fk.to_table.eq_ignore_ascii_case(&t) {
                    &fk.from_table
                } else {
                    continue;
                };
                if seen.insert(next.to_lowercase()) {
                    prev.insert(next.to_lowercase(), (t.clone(), fk));
                    queue.push_back(next.clone());
                }
            }
        }
        let mut path = Vec::new();
        let mut at = to.to_lowercase();
        while at != from.to_lowercase() {
            let (back, fk) = prev.get(&at)?;
            path.push(self.render_join(fk));
            at = back.to_lowercase();
        }
        path.reverse();
        Some(path)
    }

    /// Tables and columns whose name, description or examples mention a word of `text`.
    pub fn search(&self, text: &str) -> Vec<String> {
        let words: Vec<String> = text
            .split(|c: char| !c.is_alphanumeric() && c != '_')
            .filter(|w| w.len() >= 3)
            .map(str::to_lowercase)
            .collect();
        if words.is_empty() {
            return Vec::new();
        }
        let hits = |s: &str| {
            let s = s.to_lowercase();
            words.iter().filter(|w| s.contains(w.as_str())).count()
        };
        let mut scored: Vec<(usize, String)> = Vec::new();
        for m in &self.metrics {
            let n = hits(&m.name) * 2
                + m.description.as_deref().map_or(0, hits)
                + hits(&m.synonyms.join(" ")) * 2;
            if n > 0 {
                scored.push((n, format!("metric {}", m.render())));
            }
        }
        for t in &self.tables {
            let n = hits(&t.name) * 2
                + t.description.as_deref().map_or(0, hits)
                + hits(&t.synonyms.join(" ")) * 2;
            if n > 0 {
                scored.push((
                    n,
                    format!("table {}{}", t.name, note(t.description.as_deref())),
                ));
            }
            for c in &t.columns {
                let examples = c
                    .stats
                    .as_ref()
                    .map(|s| s.examples.join(" "))
                    .unwrap_or_default();
                let n = hits(&c.name) * 2
                    + c.description.as_deref().map_or(0, hits)
                    + hits(&examples)
                    + hits(&c.synonyms.join(" ")) * 2;
                if n > 0 {
                    scored.push((
                        n,
                        format!(
                            "column {}.{} {}{}",
                            t.name,
                            c.name,
                            c.data_type,
                            note(c.description.as_deref())
                        ),
                    ));
                }
            }
        }
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        scored.into_iter().take(MAX_LINES).map(|s| s.1).collect()
    }

    /// One table as text: rows, then a line per column with type, key, description and profile.
    pub fn describe(&self, table: &str) -> Option<String> {
        let t = self.table(table)?;
        let mut out = format!("table {}", t.qualified());
        if let Some(rows) = t.rows {
            out.push_str(&format!(" ({rows} rows)"));
        }
        out.push_str(&note(t.description.as_deref()));
        out.push('\n');
        if !t.synonyms.is_empty() {
            out.push_str(&format!("  also called: {}\n", t.synonyms.join(", ")));
        }
        for c in &t.columns {
            out.push_str(&format!("  {} {}", c.name, c.data_type));
            if c.primary_key {
                out.push_str(" PRIMARY KEY");
            }
            let mut notes: Vec<String> = Vec::new();
            if let Some(d) = &c.description {
                notes.push(one_line(d));
            }
            if c.is_time {
                notes.push("time axis".into());
            }
            if !c.synonyms.is_empty() {
                notes.push(format!("also: {}", c.synonyms.join(", ")));
            }
            if let Some(s) = &c.stats {
                notes.push(describe_stats(s));
            }
            if !notes.is_empty() {
                out.push_str(&format!(" -- {}", notes.join(" | ")));
            }
            out.push('\n');
        }
        let fks: Vec<String> = self
            .all_joins()
            .filter(|fk| {
                fk.from_table.eq_ignore_ascii_case(&t.name)
                    || fk.to_table.eq_ignore_ascii_case(&t.name)
            })
            .map(|fk| self.render_join(fk))
            .collect();
        if !fks.is_empty() {
            out.push_str(&format!("  joins: {}\n", fks.join("; ")));
        }
        if !t.unique_keys.is_empty() {
            let keys: Vec<String> = t
                .unique_keys
                .iter()
                .map(|k| format!("({})", k.join(", ")))
                .collect();
            out.push_str(&format!("  unique: {}\n", keys.join(", ")));
        }
        for m in self
            .metrics
            .iter()
            .filter(|m| m.tables.len() == 1 && m.tables[0].eq_ignore_ascii_case(&t.name))
        {
            out.push_str(&format!("  metric {}\n", m.render()));
        }
        Some(out)
    }

    /// Indexed values containing `text` (case-insensitive), exact matches first; only `column`
    /// (`table.column` or `column`) when given.
    pub fn find_values(&self, text: &str, column: Option<&str>) -> Vec<String> {
        let needle = text.trim().to_lowercase();
        if needle.is_empty() {
            return Vec::new();
        }
        let wanted = column.map(|c| {
            let c = unquote(c).to_lowercase();
            match c.rsplit_once('.') {
                Some((t, c)) => (Some(t.to_string()), c.to_string()),
                None => (None, c),
            }
        });
        let mut hits: Vec<(u8, String)> = Vec::new();
        for ((t, c), values) in &self.values {
            if let Some((wt, wc)) = &wanted
                && (wc != c || wt.as_ref().is_some_and(|wt| wt != t))
            {
                continue;
            }
            let list = self
                .column(t, c)
                .and_then(|col| col.stats.as_ref())
                .is_some_and(|s| s.list_valued);
            let display = self.column(t, c).map_or(c.clone(), |col| col.name.clone());
            let table = self.table(t).map_or(t.clone(), |tb| tb.name.clone());
            for v in values {
                let lv = v.to_lowercase();
                let rank = if v == text.trim() {
                    0
                } else if lv == needle {
                    1
                } else if lv.contains(&needle) {
                    2
                } else {
                    continue;
                };
                let hint = if list {
                    format!(
                        " (list column: use {display} LIKE '%{}%')",
                        text.trim().replace('\'', "''")
                    )
                } else {
                    String::new()
                };
                hits.push((
                    rank,
                    format!("'{}' in {table}.{display}{hint}", v.replace('\'', "''")),
                ));
            }
        }
        hits.sort();
        hits.dedup();
        hits.into_iter().take(MAX_LINES).map(|h| h.1).collect()
    }
}

impl Schema {
    /// The tables a question most likely needs, at most `max`: every table when the schema is
    /// that small, else the tables whose names, columns or descriptions share the question's
    /// words, then their foreign-key neighbours.
    pub fn relevant(&self, question: &str, max: usize) -> Vec<&Table> {
        if self.tables.len() <= max {
            return self.tables.iter().collect();
        }
        let words: Vec<String> = question
            .split(|c: char| !c.is_alphanumeric() && c != '_')
            .filter(|w| w.len() >= 3)
            .map(str::to_lowercase)
            .collect();
        let score = |t: &Table| -> usize {
            let mut text = t.name.to_lowercase();
            for c in &t.columns {
                text.push(' ');
                text.push_str(&c.name.to_lowercase());
                if let Some(d) = &c.description {
                    text.push(' ');
                    text.push_str(&d.to_lowercase());
                }
            }
            if let Some(d) = &t.description {
                text.push(' ');
                text.push_str(&d.to_lowercase());
            }
            for syn in t
                .synonyms
                .iter()
                .chain(t.columns.iter().flat_map(|c| &c.synonyms))
            {
                text.push(' ');
                text.push_str(&syn.to_lowercase());
            }
            // A defined metric names its tables: a question about "total sales" needs them.
            for m in self
                .metrics
                .iter()
                .filter(|m| m.tables.iter().any(|x| x.eq_ignore_ascii_case(&t.name)))
            {
                text.push(' ');
                text.push_str(&m.name.replace('_', " ").to_lowercase());
                for syn in &m.synonyms {
                    text.push(' ');
                    text.push_str(&syn.to_lowercase());
                }
            }
            words.iter().filter(|w| text.contains(w.as_str())).count()
                + words
                    .iter()
                    .filter(|w| t.name.to_lowercase().contains(w.as_str()))
                    .count()
                    * 2
        };
        let mut ranked: Vec<(usize, &Table)> = self
            .tables
            .iter()
            .map(|t| (score(t), t))
            .filter(|(s, _)| *s > 0)
            .collect();
        ranked.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.name.cmp(&b.1.name)));
        let mut picked: Vec<&Table> = ranked.iter().take(max.div_ceil(2)).map(|r| r.1).collect();
        let seeds: Vec<String> = picked.iter().map(|t| t.name.to_lowercase()).collect();
        for fk in self.all_joins() {
            for (from, to) in [
                (&fk.from_table, &fk.to_table),
                (&fk.to_table, &fk.from_table),
            ] {
                if picked.len() < max
                    && seeds.contains(&from.to_lowercase())
                    && let Some(t) = self.table(to)
                    && !picked.iter().any(|p| p.name == t.name)
                {
                    picked.push(t);
                }
            }
        }
        for (_, t) in ranked {
            if picked.len() >= max {
                break;
            }
            if !picked.iter().any(|p| p.name == t.name) {
                picked.push(t);
            }
        }
        picked
    }

    /// Tables as prompt text: [`Schema::describe`] for each, then the foreign keys among them.
    pub fn render(&self, tables: &[&Table]) -> String {
        let mut out = String::new();
        for t in tables {
            if let Some(d) = self.describe(&t.name) {
                out.push_str(&d);
                out.push('\n');
            }
        }
        // RFC 0246: a metric over several tables belongs to none of them; list it once when all
        // its tables are shown (a one-table metric is already under its table).
        let shown = |name: &str| tables.iter().any(|t| t.name.eq_ignore_ascii_case(name));
        let spanning: Vec<String> = self
            .metrics
            .iter()
            .filter(|m| m.tables.len() > 1 && m.tables.iter().all(|t| shown(t)))
            .map(|m| format!("  metric {}", m.render()))
            .collect();
        if !spanning.is_empty() {
            out.push_str(&format!(
                "metrics across tables:\n{}\n\n",
                spanning.join("\n")
            ));
        }
        out
    }
}

/// A profile as one comment: `12 null; 6412 distinct; e.g. 'a', 'b'`.
pub fn describe_stats(s: &ColumnStats) -> String {
    let mut parts = Vec::new();
    if s.nulls > 0 {
        parts.push(format!("{} null", s.nulls));
    }
    if s.empty > 0 {
        parts.push(format!("{} empty strings", s.empty));
    }
    parts.push(if s.unique {
        format!("unique ({} values)", s.distinct)
    } else {
        format!("{} distinct", s.distinct)
    });
    if s.list_valued {
        parts.push("comma-separated list: match one element with LIKE '%x%'".into());
    }
    for (v, n) in &s.sentinels {
        parts.push(format!("{n} rows hold {v} (likely 'unknown')"));
    }
    if !s.examples.is_empty() {
        let ex: Vec<String> = s
            .examples
            .iter()
            .map(|e| format!("'{}'", e.replace('\'', "''")))
            .collect();
        parts.push(format!("e.g. {}", ex.join(", ")));
    }
    parts.join("; ")
}

fn note(d: Option<&str>) -> String {
    d.map(|d| format!(" -- {}", one_line(d)))
        .unwrap_or_default()
}

fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn ledger_column(v: &serde_json::Value) -> Option<Column> {
    Some(Column {
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
        stats: None,
        ..Default::default()
    })
}

/// `"a"."x" → "b"."y"` (a ledger `fk_desc`, optionally schema-qualified) as a foreign key.
pub fn parse_fk_desc(desc: &str) -> Option<ForeignKey> {
    let clean = desc.replace(['"', '`'], "");
    let (from, to) = clean.split_once('→').or_else(|| clean.split_once('='))?;
    let split = |s: &str| {
        let s = s.trim();
        let (t, c) = s.rsplit_once('.')?;
        let t = t.rsplit('.').next().unwrap_or(t);
        Some((t.trim().to_string(), c.trim().to_string()))
    };
    let (from_table, from_column) = split(from)?;
    let (to_table, to_column) = split(to)?;
    Some(ForeignKey {
        from_table,
        from_column,
        to_table,
        to_column,
    })
}

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

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A small schema: `cards` (list-valued `types`), `rulings` → `cards`, a sentinel column.
    pub fn fixture() -> Schema {
        let col = |name: &str, ty: &str, pk: bool| Column {
            name: name.into(),
            data_type: ty.into(),
            primary_key: pk,
            ..Default::default()
        };
        let mut cards = Table {
            name: "cards".into(),
            rows: Some(100),
            columns: vec![
                col("id", "INTEGER", true),
                col("name", "TEXT", false),
                col("types", "TEXT", false),
                col("power", "INTEGER", false),
                col("height_cm", "INTEGER", false),
                col("released", "TEXT", false),
                col("rarity", "TEXT", false),
            ],
            ..Default::default()
        };
        cards.columns[2].stats = Some(ColumnStats {
            distinct: 10,
            list_valued: true,
            examples: vec!["Creature".into(), "Artifact,Creature".into()],
            ..Default::default()
        });
        cards.columns[3].stats = Some(ColumnStats {
            nulls: 7,
            distinct: 12,
            ..Default::default()
        });
        cards.columns[4].stats = Some(ColumnStats {
            distinct: 40,
            sentinels: vec![("0".into(), 9)],
            ..Default::default()
        });
        cards.columns[5].stats = Some(ColumnStats {
            distinct: 90,
            examples: vec!["2017-01-21 00:00:00".into()],
            ..Default::default()
        });
        let rulings = Table {
            name: "rulings".into(),
            rows: Some(300),
            columns: vec![
                col("id", "INTEGER", true),
                col("card_id", "INTEGER", false),
                col("text", "TEXT", false),
            ],
            ..Default::default()
        };
        let sets = Table {
            name: "sets".into(),
            columns: vec![col("code", "TEXT", true), col("name", "TEXT", false)],
            ..Default::default()
        };
        let mut values = BTreeMap::new();
        values.insert(
            ("cards".into(), "rarity".into()),
            vec!["common".into(), "rare".into(), "mythic".into()],
        );
        values.insert(
            ("cards".into(), "types".into()),
            vec!["Creature".into(), "Artifact,Creature".into()],
        );
        values.insert(("sets".into(), "name".into()), vec!["Rare Prints".into()]);
        Schema {
            tables: vec![cards, rulings, sets],
            foreign_keys: vec![ForeignKey {
                from_table: "rulings".into(),
                from_column: "card_id".into(),
                to_table: "cards".into(),
                to_column: "id".into(),
            }],
            values,
            ..Default::default()
        }
    }

    #[test]
    fn fk_desc_parses_quoted_and_schema_qualified() {
        let fk = parse_fk_desc("\"public\".\"rulings\".\"card_id\" → \"cards\".\"id\"").unwrap();
        assert_eq!(fk.render(), "rulings.card_id = cards.id");
        assert!(parse_fk_desc("nonsense").is_none());
    }

    #[test]
    fn schema_qualified_names_and_database_filter() {
        let mut s = fixture();
        s.tables[1].schema = Some("app".into());
        s.tables[1].database = Some("live".into());
        assert_eq!(s.table("app.rulings").unwrap().name, "rulings");
        assert_eq!(s.table("\"app\".\"rulings\"").unwrap().name, "rulings");
        assert_eq!(s.table("rulings").unwrap().qualified(), "app.rulings");
        assert!(
            s.describe("rulings")
                .unwrap()
                .starts_with("table app.rulings")
        );
        let only = s.clone().for_database("live");
        assert_eq!(only.tables.len(), 1);
        assert_eq!(s.for_database("elsewhere").tables.len(), 3);
    }

    #[test]
    fn join_path_walks_foreign_keys_both_ways() {
        let s = fixture();
        assert_eq!(
            s.join_path("cards", "rulings").unwrap(),
            vec!["rulings.card_id = cards.id"]
        );
        assert!(s.join_path("cards", "sets").is_none());
    }

    #[test]
    fn find_values_ranks_exact_first_and_marks_list_columns() {
        let s = fixture();
        let hits = s.find_values("Creature", None);
        assert!(hits[0].starts_with("'Creature' in cards.types (list column"));
        let rare = s.find_values("rare", Some("cards.rarity"));
        assert_eq!(rare, vec!["'rare' in cards.rarity"]);
    }

    #[test]
    fn relevant_picks_named_tables_and_their_neighbours() {
        let s = fixture();
        assert_eq!(
            s.relevant("anything", 10).len(),
            3,
            "a small schema is shown whole"
        );
        let names: Vec<&str> = s
            .relevant("which rulings mention fire", 2)
            .iter()
            .map(|t| t.name.as_str())
            .collect();
        assert_eq!(names, vec!["rulings", "cards"]);
    }

    #[test]
    fn describe_and_search_show_profile_and_names() {
        let s = fixture();
        let d = s.describe("CARDS").unwrap();
        assert!(d.contains("9 rows hold 0 (likely 'unknown')"));
        assert!(d.contains("joins: rulings.card_id = cards.id"));
        assert!(s.search("card rulings")[0].contains("rulings"));
    }

    #[test]
    fn retain_tables_keeps_the_named_tables_and_only_their_joins() {
        let names = vec!["CARDS".to_string(), "main.rulings".into(), "nope".into()];
        let (s, unknown) = fixture().retain_tables(&names);
        let kept: Vec<&str> = s.tables.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(kept, ["cards", "rulings"]);
        assert_eq!(
            s.foreign_keys.len(),
            1,
            "rulings → cards is between kept tables"
        );
        assert!(
            s.values.keys().all(|(t, _)| t != "sets"),
            "values of a dropped table go"
        );
        assert_eq!(unknown, ["nope"]);

        let (alone, _) = fixture().retain_tables(&["rulings".to_string()]);
        assert!(
            alone.foreign_keys.is_empty(),
            "a join to a dropped table goes"
        );
    }
}
