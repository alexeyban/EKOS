//! RFC 0211 §3.6 — code ↔ database conflicts.
//!
//! The code's claims about the database (RFC 0209's `ModelTable`s, RFC 0210's routine files)
//! against the live catalog (`origin: live` objects). Each disagreement is an RFC 0172
//! `ConflictingEvidence` citing both sides, so it is reviewed like every other conflict.
//!
//! | Conflict type | Subject | Code says | Database says |
//! |---|---|---|---|
//! | `model_table_missing` | `ModelTable` | the table exists | no such table or view |
//! | `table_without_model` | live `Table` | no model class | the table exists |
//! | `model_column_missing` | live `Table` | the column exists | no such column |
//! | `mandatory_but_nullable` | live `Table` | the column is mandatory | the column is nullable |
//! | `routine_text_differs` | file `Procedure` | its text | a different text (comments ignored) |
//!
//! A conflict the source tree itself accounts for carries an `explanation` and stays open for a
//! person to accept:
//!
//! - `pending_migration` — a migration script creates, adds, drops or tightens exactly this table
//!   or column, and is dated after the database's **migration horizon**: the latest scripted
//!   addition whose result the catalog does contain. Derived from the catalog and the scripts
//!   alone, never from application rows; an older script whose result is missing is drift, not
//!   pending.
//! - `translation_table` — `X_TRL` whose parent `X` has a model class: the iDempiere family reads
//!   translations through the parent's model.
//!
//! Names compare case-insensitively (the dictionary upper-cases, the code does not). A name with
//! more than one candidate on either side is counted as ambiguous and compared no further; among
//! routine files, one defined in a migration script is history, so the definition outside them is
//! the one compared. Nothing is compared unless both sides exist.

use crate::conflicts::{Claim, conflict};
use crate::procedure_lineage::is_live;
use ekos_kir::{KirGraph, KirObject, ObjectKind};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub const MODEL_TABLE_MISSING: &str = "model_table_missing";
pub const TABLE_WITHOUT_MODEL: &str = "table_without_model";
pub const MODEL_COLUMN_MISSING: &str = "model_column_missing";
pub const MANDATORY_BUT_NULLABLE: &str = "mandatory_but_nullable";
pub const ROUTINE_TEXT_DIFFERS: &str = "routine_text_differs";

/// What one comparison found, by conflict type.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct LiveConflictStats {
    pub tables_compared: usize,
    pub routines_compared: usize,
    pub ambiguous: usize,
    pub model_table_missing: usize,
    pub table_without_model: usize,
    pub model_column_missing: usize,
    pub mandatory_but_nullable: usize,
    pub routine_text_differs: usize,
    /// Of the conflicts, those carrying an `explanation`.
    pub explained: usize,
    /// Model column names RFC 0043 redacted at build: not comparable, counted.
    pub redacted_names: usize,
    /// Model columns the generated code marks virtual: not physical, so not compared.
    pub virtual_columns: usize,
    /// The migration horizon as `yyyymmddhhmm`, when one could be derived.
    pub horizon: Option<u64>,
    /// The newest release most of whose scripted additions the catalog contains.
    pub horizon_release: Option<String>,
    /// Live objects whose dictionary status is not `VALID`: a fact on the object, counted here.
    pub invalid_objects: usize,
    /// Of the `mandatory_but_nullable` conflicts: columns whose profile counts nulls in the data
    /// (the code's rule is broken by real rows), columns it counts none in (adding `NOT NULL`
    /// would hold today), and columns without a profile. Exact counts come from a P2 scan, the
    /// rest from optimizer statistics.
    pub nullable_holding_nulls: usize,
    pub nullable_without_nulls: usize,
    pub nullable_unmeasured: usize,
    pub nullable_counted_exactly: usize,
}

impl LiveConflictStats {
    pub fn conflicts(&self) -> usize {
        self.model_table_missing
            + self.table_without_model
            + self.model_column_missing
            + self.mandatory_but_nullable
            + self.routine_text_differs
    }
}

fn custom(o: &KirObject, kind: &str) -> bool {
    matches!(&o.kind, ObjectKind::Custom(k) if k == kind)
}

fn prop<'a>(o: &'a KirObject, key: &str) -> Option<&'a str> {
    o.properties.get(key).and_then(Value::as_str)
}

/// `oracle://<database>/<OWNER>/<kind>/<NAME>`, the observer's target for a live object.
fn live_path(o: &KirObject, kind: &str) -> String {
    format!(
        "oracle://{}/{}/{kind}/{}",
        prop(o, "database").unwrap_or_default(),
        prop(o, "owner").unwrap_or_default(),
        o.name
    )
}

/// `path:line` → (path, line).
fn split_source(source: &str) -> (String, Option<u32>) {
    match source.rsplit_once(':') {
        Some((p, l)) if !l.is_empty() && l.chars().all(|c| c.is_ascii_digit()) => {
            (p.to_string(), l.parse().ok())
        }
        _ => (source.to_string(), None),
    }
}

fn claim(value: Value, (path, line): (String, Option<u32>), source: &str) -> Claim {
    Claim {
        value,
        path,
        line,
        source: source.into(),
    }
}

type ByName<'a> = BTreeMap<String, Vec<&'a KirObject>>;

/// Index by lower-cased name.
fn by_name<'a>(objects: impl Iterator<Item = &'a KirObject>) -> ByName<'a> {
    let mut m: ByName = BTreeMap::new();
    for o in objects {
        m.entry(o.name.to_lowercase()).or_default().push(o);
    }
    m
}

/// The model's columns: lower-cased name → (name as written, the column's facts).
fn model_columns(model: &KirObject) -> BTreeMap<String, (&str, &Value)> {
    model
        .properties
        .get("columns")
        .and_then(Value::as_object)
        .map(|m| {
            m.iter()
                .map(|(k, v)| (k.to_lowercase(), (k.as_str(), v)))
                .collect()
        })
        .unwrap_or_default()
}

/// A live relation's columns: lower-cased name → the column.
fn live_columns(table: &KirObject) -> BTreeMap<String, &Value> {
    table
        .properties
        .get("columns")
        .and_then(Value::as_array)
        .map(|cols| {
            cols.iter()
                .filter_map(|c| Some((c.get("name")?.as_str()?.to_lowercase(), c)))
                .collect()
        })
        .unwrap_or_default()
}

/// Where the model states the table: its first column's declaration.
fn model_source(model: &KirObject) -> (String, Option<u32>) {
    model_columns(model)
        .values()
        .filter_map(|(_, c)| c.get("source").and_then(Value::as_str))
        .min()
        .map(split_source)
        .unwrap_or_else(|| (format!("application model: {}", model.name), None))
}

/// `2024-02-02T09:15[:ss]` → `202402020915`.
fn minute_of(applied_at: &str) -> Option<u64> {
    let digits: String = applied_at
        .chars()
        .filter(char::is_ascii_digit)
        .take(12)
        .collect();
    (digits.len() == 12).then(|| digits.parse().ok()).flatten()
}

/// One scripted change, reduced to what an explanation needs.
struct Scripted<'a> {
    operation: &'a str,
    table: String,
    /// The column it adds or changes, the columns a `create_table` declares, or the name a
    /// `rename_column` gives (the name it introduces, which is what a model may be missing).
    columns: Vec<String>,
    not_null: bool,
    at: u64,
    applied_at: &'a str,
    path: &'a str,
    line: Option<u64>,
    release: Option<&'a str>,
    engine: &'a str,
}

/// RFC 0210's `SchemaChange`s, and the database's migration horizon.
struct History<'a> {
    changes: Vec<Scripted<'a>>,
    /// The latest scripted addition the catalog contains.
    horizon: Option<u64>,
    /// The newest release most of whose scripted additions the catalog contains, and every
    /// release's rank (by its latest script). A script is dated when it is written, not when it
    /// ships, so a release can hold scripts older than the time horizon that the database lacks.
    horizon_release: Option<&'a str>,
    rank: BTreeMap<&'a str, u64>,
    /// The engine whose scripts an explanation cites first: the live catalog's dialect.
    engine: Option<&'a str>,
    /// Scripts that are migrations, so a routine defined in one is history.
    scripts: BTreeSet<&'a str>,
}

fn scripted(o: &KirObject) -> Option<Scripted<'_>> {
    let applied_at = prop(o, "applied_at")?;
    let operation = prop(o, "operation")?;
    let column = o.properties.get("column");
    let introduced = if operation == "rename_column" {
        o.properties.get("name")
    } else {
        column.and_then(|c| c.get("name"))
    };
    let mut columns: Vec<String> = introduced
        .and_then(Value::as_str)
        .map(|n| vec![n.to_lowercase()])
        .unwrap_or_default();
    if let Some(cs) = o.properties.get("columns").and_then(Value::as_array) {
        columns.extend(
            cs.iter()
                .filter_map(|c| c.get("name").and_then(Value::as_str))
                .map(str::to_lowercase),
        );
    }
    Some(Scripted {
        operation,
        table: prop(o, "table")?.to_lowercase(),
        columns,
        not_null: column.and_then(|c| c.get("not_null")) == Some(&json!(true)),
        at: minute_of(applied_at)?,
        applied_at,
        path: prop(o, "source_path").unwrap_or_default(),
        line: o.properties.get("line").and_then(Value::as_u64),
        release: prop(o, "release"),
        engine: prop(o, "engine").unwrap_or_default(),
    })
}

impl<'a> History<'a> {
    fn new(objects: &'a [KirObject], live: &ByName<'a>) -> Self {
        let changes: Vec<Scripted> = objects
            .iter()
            .filter(|o| custom(o, "SchemaChange"))
            .filter_map(scripted)
            .collect();
        let scripts = changes.iter().map(|c| c.path).collect();
        // The latest scripted addition the catalog shows: the database has run at least that far.
        let present = |c: &Scripted| {
            let Some([t]) = live.get(&c.table).map(Vec::as_slice) else {
                return false;
            };
            match c.operation {
                "create_table" | "create_view" => true,
                "add_column" => {
                    let cols = live_columns(t);
                    c.columns.iter().all(|n| cols.contains_key(n))
                }
                _ => false,
            }
        };
        let horizon = changes.iter().filter(|c| present(c)).map(|c| c.at).max();
        let mut rank: BTreeMap<&str, u64> = BTreeMap::new();
        let mut additions: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
        for c in &changes {
            let Some(r) = c.release else { continue };
            let latest = rank.entry(r).or_insert(0);
            *latest = (*latest).max(c.at);
            if matches!(c.operation, "create_table" | "create_view" | "add_column") {
                let e = additions.entry(r).or_insert((0, 0));
                e.1 += 1;
                if present(c) {
                    e.0 += 1;
                }
            }
        }
        // A majority, so one coincidentally present column cannot move the database a release on.
        let horizon_release = additions
            .iter()
            .filter(|(_, (present, total))| *present * 2 > *total)
            .max_by_key(|(r, _)| rank.get(*r))
            .map(|(r, _)| *r);
        let engine = live.values().flatten().find_map(|o| prop(o, "dialect"));
        Self {
            changes,
            horizon,
            horizon_release,
            rank,
            engine,
            scripts,
        }
    }

    /// Whether `c` is newer than what the database has run.
    fn is_pending(&self, c: &Scripted) -> bool {
        let Some(horizon) = self.horizon else {
            return false;
        };
        let newer_release = match (c.release, self.horizon_release) {
            (Some(r), Some(h)) => self.rank.get(r) > self.rank.get(h),
            _ => false,
        };
        newer_release || c.at > horizon
    }

    /// The latest change after the horizon doing `operation` to `table` (and `column`).
    fn pending(&self, operations: &[&str], table: &str, column: Option<&str>) -> Option<Value> {
        let c = self
            .changes
            .iter()
            .filter(|c| c.table == table && operations.contains(&c.operation))
            .filter(|c| column.is_none_or(|col| c.columns.iter().any(|n| n == col)))
            .filter(|c| !operations.contains(&"modify_column") || c.not_null)
            .filter(|c| self.is_pending(c))
            .max_by_key(|c| (Some(c.engine) == self.engine, c.at, c.path))?;
        Some(json!({
            "kind": "pending_migration",
            "operation": c.operation,
            "script": c.path,
            "line": c.line,
            "applied_at": c.applied_at,
            "release": c.release,
            "database_release": self.horizon_release,
        }))
    }
}

/// Collects conflicts and counts them.
struct Out<'a> {
    graph: KirGraph,
    stats: LiveConflictStats,
    history: History<'a>,
}

impl Out<'_> {
    fn add(&mut self, part: KirGraph, explanation: Option<Value>) {
        let mut part = part;
        if let Some(e) = explanation {
            for o in &mut part.objects {
                o.properties.insert("explanation".into(), e.clone());
            }
            self.stats.explained += 1;
        }
        self.graph.objects.extend(part.objects);
        self.graph.relationships.extend(part.relationships);
        self.graph.evidence.extend(part.evidence);
    }
}

/// Compare the code's claims with the live catalog in `objects`; the conflicts as a graph to
/// append, and the counts.
pub fn detect(objects: &[KirObject]) -> (KirGraph, LiveConflictStats) {
    let live = by_name(
        objects
            .iter()
            .filter(|o| is_live(o) && (o.kind == ObjectKind::Table || custom(o, "View"))),
    );
    let mut out = Out {
        graph: KirGraph::new(),
        stats: LiveConflictStats {
            invalid_objects: objects
                .iter()
                .filter(|o| is_live(o) && prop(o, "status").is_some_and(|s| s == "INVALID"))
                .count(),
            ..Default::default()
        },
        history: History::new(objects, &live),
    };
    out.stats.horizon = out.history.horizon;
    out.stats.horizon_release = out.history.horizon_release.map(str::to_string);
    let models = by_name(objects.iter().filter(|o| custom(o, "ModelTable")));
    if !live.is_empty() && !models.is_empty() {
        compare_tables(&mut out, &live, &models);
    }
    let live_routines = by_name(
        objects
            .iter()
            .filter(|o| custom(o, "Procedure") && is_live(o)),
    );
    if !live_routines.is_empty() {
        let files = by_name(objects.iter().filter(|o| {
            custom(o, "Procedure") && !is_live(o) && prop(o, "language") == Some("plsql")
        }));
        compare_routines(&mut out, &live_routines, &files);
    }
    (out.graph, out.stats)
}

fn compare_tables(out: &mut Out, live: &ByName, models: &ByName) {
    for (name, ms) in models {
        let [model] = ms.as_slice() else {
            out.stats.ambiguous += 1;
            continue;
        };
        match live.get(name).map(Vec::as_slice) {
            None => {
                let mut claims = vec![claim(json!(model.name), model_source(model), "code")];
                claims.extend(
                    live_catalogs(live)
                        .into_iter()
                        .map(|p| claim(Value::Null, (p, None), "database")),
                );
                let why = out
                    .history
                    .pending(&["create_table", "create_view"], name, None);
                out.add(
                    conflict(model, "database_table", MODEL_TABLE_MISSING, claims, None),
                    why,
                );
                out.stats.model_table_missing += 1;
            }
            Some([relation]) => {
                out.stats.tables_compared += 1;
                if relation.kind == ObjectKind::Table {
                    compare_columns(out, model, relation);
                }
            }
            Some(_) => out.stats.ambiguous += 1,
        }
    }
    for (name, relations) in live {
        if models.contains_key(name) {
            continue;
        }
        for table in relations.iter().filter(|r| r.kind == ObjectKind::Table) {
            let claims = vec![
                claim(
                    json!(table.name),
                    (live_path(table, "table"), None),
                    "database",
                ),
                claim(Value::Null, ("application model".into(), None), "code"),
            ];
            let parent = name
                .strip_suffix("_trl")
                .filter(|p| models.contains_key(*p));
            let why = out
                .history
                .pending(&["drop_table"], name, None)
                .or_else(|| parent.map(|p| json!({ "kind": "translation_table", "parent": p })));
            out.add(
                conflict(table, "model_class", TABLE_WITHOUT_MODEL, claims, None),
                why,
            );
            out.stats.table_without_model += 1;
        }
    }
}

/// The `oracle://<database>/<OWNER>` of every schema the live relations came from.
fn live_catalogs(live: &ByName) -> Vec<String> {
    let set: BTreeSet<String> = live
        .values()
        .flatten()
        .map(|t| {
            format!(
                "oracle://{}/{}",
                prop(t, "database").unwrap_or_default(),
                prop(t, "owner").unwrap_or_default()
            )
        })
        .collect();
    set.into_iter().collect()
}

fn compare_columns(out: &mut Out, model: &KirObject, table: &KirObject) {
    let live = live_columns(table);
    let table_lc = table.name.to_lowercase();
    let table_path = live_path(table, "table");
    for (lower, (written, facts)) in model_columns(model) {
        if written.contains("[REDACTED") {
            out.stats.redacted_names += 1;
            continue;
        }
        // Computed by the application (`AD_Column.ColumnSQL`): no physical column to compare.
        if facts.get("virtual_column").and_then(Value::as_bool) == Some(true) {
            out.stats.virtual_columns += 1;
            continue;
        }
        let source = facts
            .get("source")
            .and_then(Value::as_str)
            .map(split_source)
            .unwrap_or_else(|| model_source(model));
        let Some(col) = live.get(&lower) else {
            let claims = vec![
                claim(json!("declared"), source, "code"),
                claim(Value::Null, (table_path.clone(), None), "database"),
            ];
            let why = out.history.pending(
                &["add_column", "create_table", "rename_column"],
                &table_lc,
                Some(&lower),
            );
            let attr = format!("columns.{written}");
            out.add(
                conflict(table, &attr, MODEL_COLUMN_MISSING, claims, None),
                why,
            );
            out.stats.model_column_missing += 1;
            continue;
        };
        let mandatory = facts.get("mandatory").and_then(Value::as_bool) == Some(true);
        let not_null = col.get("not_null").and_then(Value::as_bool) == Some(true);
        if mandatory && !not_null {
            let claims = vec![
                claim(json!(false), source, "code"),
                claim(json!(true), (table_path.clone(), None), "database"),
            ];
            let why = out
                .history
                .pending(&["modify_column"], &table_lc, Some(&lower));
            let attr = format!("columns.{written}.nullable");
            let mut part = conflict(table, &attr, MANDATORY_BUT_NULLABLE, claims, None);
            match null_measurement(table, col) {
                Some(m) => {
                    if m["violating_rows"].as_i64().unwrap_or(0) > 0 {
                        out.stats.nullable_holding_nulls += 1;
                    } else {
                        out.stats.nullable_without_nulls += 1;
                    }
                    if m["exact"] == true {
                        out.stats.nullable_counted_exactly += 1;
                    }
                    for o in &mut part.objects {
                        o.properties.insert("measured".into(), m.clone());
                    }
                }
                None => out.stats.nullable_unmeasured += 1,
            }
            out.add(part, why);
            out.stats.mandatory_but_nullable += 1;
        }
    }
}

/// What the column's profile (RFC 0211 §3.4) says about the rows that break a mandatory rule: in
/// the shape RFC 0172's doc-vs-data measurements use, with the query that re-counts them.
fn null_measurement(table: &KirObject, col: &Value) -> Option<Value> {
    let p = col.get("profile")?;
    let nulls = p.get("nulls")?.as_i64()?;
    let exact = p.get("nulls_exact") == Some(&json!(true));
    let name = col.get("name").and_then(Value::as_str).unwrap_or_default();
    let owner = prop(table, "owner").unwrap_or_default();
    let mut m = json!({
        "violating_rows": nulls,
        "exact": exact,
        "tier": p.get("tier").cloned().unwrap_or(Value::Null),
        "against": "the model's mandatory flag",
        "sql": format!("SELECT COUNT(*) FROM \"{owner}\".\"{}\" WHERE \"{}\" IS NULL", table.name, name.to_uppercase()),
    });
    if let Some(rows) = table
        .properties
        .get("profile")
        .and_then(|t| t.get("row_count"))
    {
        m["rows"] = rows.clone();
    }
    if let Some(at) = p.get("analyzed") {
        m["measured_at"] = at.clone();
    }
    Some(m)
}

/// The definition among same-named routine files: the only one outside a migration script.
fn definition<'a>(files: &[&'a KirObject], scripts: &BTreeSet<&str>) -> Option<&'a KirObject> {
    match files {
        [one] => Some(one),
        many => {
            let outside: Vec<&&KirObject> = many
                .iter()
                .filter(|f| !scripts.contains(prop(f, "source_path").unwrap_or_default()))
                .collect();
            match outside.as_slice() {
                [one] => Some(**one),
                _ => None,
            }
        }
    }
}

fn compare_routines(out: &mut Out, live: &ByName, files: &ByName) {
    for (name, fs) in files {
        let Some(ls) = live.get(name) else {
            continue;
        };
        let (Some(file), [db]) = (definition(fs, &out.history.scripts), ls.as_slice()) else {
            out.stats.ambiguous += 1;
            continue;
        };
        let (Some(a), Some(b)) = (prop(file, "text_hash"), prop(db, "text_hash")) else {
            continue;
        };
        out.stats.routines_compared += 1;
        if a == b {
            continue;
        }
        let file_at = (
            prop(file, "source_path").unwrap_or_default().to_string(),
            file.properties
                .get("line")
                .and_then(Value::as_u64)
                .and_then(|l| u32::try_from(l).ok()),
        );
        let db_at = (
            prop(db, "source_path").unwrap_or_default().to_string(),
            None,
        );
        let claims = vec![
            claim(json!(a), file_at, "code"),
            claim(json!(b), db_at, "database"),
        ];
        out.add(
            conflict(file, "text", ROUTINE_TEXT_DIFFERS, claims, None),
            None,
        );
        out.stats.routine_text_differs += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn live_table(name: &str, columns: Value) -> KirObject {
        KirObject::new(name, ObjectKind::Table)
            .with_property("origin", json!("live"))
            .with_property("database", json!("erp"))
            .with_property("owner", json!("ADEMPIERE"))
            .with_property("columns", columns)
    }

    fn model(name: &str, columns: Value) -> KirObject {
        KirObject::new(name, ObjectKind::Custom("ModelTable".into()))
            .with_property("columns", columns)
    }

    fn routine(name: &str, path: &str, hash: &str) -> KirObject {
        let mut o = KirObject::new(name, ObjectKind::Custom("Procedure".into()))
            .with_property("language", json!("plsql"))
            .with_property("text_hash", json!(hash))
            .with_property("source_path", json!(path))
            .with_property("line", json!(1));
        if path.starts_with("oracle://") {
            o.properties.insert("origin".into(), json!("live"));
        }
        o
    }

    fn change(op: &str, table: &str, column: Option<&str>, at: &str, path: &str) -> KirObject {
        let mut o = KirObject::new(
            format!("{op} {table}"),
            ObjectKind::Custom("SchemaChange".into()),
        )
        .with_property("operation", json!(op))
        .with_property("table", json!(table))
        .with_property("applied_at", json!(at))
        .with_property("source_path", json!(path))
        .with_property("release", json!(path.split('/').nth(1)))
        .with_property("engine", json!(path.split('/').nth(2)))
        .with_property("line", json!(3));
        if let Some(c) = column {
            o.properties
                .insert("column".into(), json!({ "name": c, "not_null": true }));
        }
        o
    }

    fn types(g: &KirGraph) -> Vec<(String, String)> {
        let mut v: Vec<(String, String)> = g
            .objects
            .iter()
            .map(|o| {
                (
                    o.properties["conflict_type"].as_str().unwrap().to_string(),
                    o.properties["attribute"].as_str().unwrap().to_string(),
                )
            })
            .collect();
        v.sort();
        v
    }

    fn explanation<'a>(g: &'a KirGraph, attribute: &str) -> Option<&'a Value> {
        g.objects
            .iter()
            .find(|o| o.properties["attribute"] == attribute)
            .and_then(|o| o.properties.get("explanation"))
    }

    #[test]
    fn each_kind_of_disagreement_becomes_one_cited_conflict() {
        let objects = vec![
            live_table(
                "C_INVOICE",
                json!([
                    { "name": "C_INVOICE_ID", "data_type": "NUMBER(10,0)", "not_null": true },
                    { "name": "DOCUMENTNO", "data_type": "VARCHAR2(30 CHAR)" },
                ]),
            ),
            live_table("T_LEFTOVER", json!([])),
            model(
                "C_Invoice",
                json!({
                    "C_Invoice_ID": { "mandatory": true, "source": "base/src/org/compiere/model/X_C_Invoice.java:40" },
                    "DocumentNo": { "mandatory": true, "source": "base/src/org/compiere/model/X_C_Invoice.java:41" },
                    "GrandTotal": { "source": "base/src/org/compiere/model/X_C_Invoice.java:42" },
                    "[REDACTED:generic-assigned-secret]": { "source": "X_C_Invoice.java:43" },
                    "OpenAmt": { "virtual_column": true, "source": "X_C_Invoice.java:44" },
                }),
            ),
            model(
                "C_Gone",
                json!({ "C_Gone_ID": { "source": "X_C_Gone.java:9" } }),
            ),
            routine("invoiceOpen", "db/oracle/functions/invoiceOpen.sql", "aaa"),
            routine(
                "INVOICEOPEN",
                "oracle://erp/ADEMPIERE/function/INVOICEOPEN",
                "bbb",
            ),
            routine("same", "db/oracle/functions/same.sql", "ccc"),
            routine("SAME", "oracle://erp/ADEMPIERE/function/SAME", "ccc"),
        ];
        let (g, stats) = detect(&objects);
        assert_eq!(
            types(&g),
            [
                (
                    MANDATORY_BUT_NULLABLE.into(),
                    "columns.DocumentNo.nullable".into()
                ),
                (MODEL_COLUMN_MISSING.into(), "columns.GrandTotal".into()),
                (MODEL_TABLE_MISSING.into(), "database_table".into()),
                (ROUTINE_TEXT_DIFFERS.into(), "text".into()),
                (TABLE_WITHOUT_MODEL.into(), "model_class".into()),
            ]
        );
        assert_eq!((stats.tables_compared, stats.routines_compared), (1, 2));
        assert_eq!(
            (stats.conflicts(), stats.explained, stats.redacted_names),
            (5, 0, 1)
        );
        let nullable = g
            .objects
            .iter()
            .find(|o| o.properties["conflict_type"] == MANDATORY_BUT_NULLABLE)
            .unwrap();
        let claims = nullable.properties["claims"].as_array().unwrap();
        assert!(claims.iter().any(
            |c| c["path"] == "base/src/org/compiere/model/X_C_Invoice.java" && c["line"] == 41
        ));
        assert!(
            claims
                .iter()
                .any(|c| c["path"] == "oracle://erp/ADEMPIERE/table/C_INVOICE")
        );
        assert_eq!(g.relationships.len(), 5, "one Disputes link per conflict");
    }

    #[test]
    fn a_migration_newer_than_the_horizon_explains_and_an_older_one_does_not() {
        let objects = vec![
            live_table(
                "C_INVOICE",
                json!([{ "name": "C_INVOICE_ID", "not_null": true }, { "name": "OLDCOL" }]),
            ),
            live_table("C_INVOICE_TRL", json!([])),
            live_table("CM_LEFTOVER", json!([])),
            model(
                "C_Invoice",
                json!({
                    "C_Invoice_ID": { "source": "X.java:1" },
                    "OldCol": { "mandatory": true, "source": "X.java:2" },
                    "NewCol": { "source": "X.java:3" },
                    "DriftCol": { "source": "X.java:4" },
                }),
            ),
            model("C_New", json!({ "C_New_ID": { "source": "Y.java:1" } })),
            // Present in the catalog: sets the horizon at 2024-12-01.
            change(
                "add_column",
                "C_Invoice",
                Some("OldCol"),
                "2024-12-01T10:00",
                "migration/iD12/oracle/a.sql",
            ),
            // Older than the horizon and missing: drift, unexplained.
            change(
                "add_column",
                "C_Invoice",
                Some("DriftCol"),
                "2023-05-01T10:00",
                "migration/iD11/oracle/b.sql",
            ),
            // Newer: pending.
            change(
                "add_column",
                "C_Invoice",
                Some("NewCol"),
                "2025-03-01T10:00",
                "migration/iD13/oracle/c.sql",
            ),
            change(
                "create_table",
                "C_New",
                None,
                "2025-03-02T10:00",
                "migration/iD13/oracle/d.sql",
            ),
            change(
                "modify_column",
                "C_Invoice",
                Some("OldCol"),
                "2025-03-03T10:00",
                "migration/iD13/oracle/e.sql",
            ),
            change(
                "drop_table",
                "CM_Leftover",
                None,
                "2025-03-04T10:00",
                "migration/iD13/oracle/f.sql",
            ),
        ];
        let (g, stats) = detect(&objects);
        assert_eq!(stats.horizon, Some(202412011000));
        assert_eq!(stats.horizon_release.as_deref(), Some("iD12"));
        assert_eq!(
            explanation(&g, "columns.NewCol").unwrap()["script"],
            "migration/iD13/oracle/c.sql"
        );
        assert!(
            explanation(&g, "columns.DriftCol").is_none(),
            "older than the horizon is drift"
        );
        assert_eq!(
            explanation(&g, "database_table").unwrap()["operation"],
            "create_table"
        );
        assert_eq!(
            explanation(&g, "columns.OldCol.nullable").unwrap()["operation"],
            "modify_column"
        );
        let without: Vec<&Value> = g
            .objects
            .iter()
            .filter(|o| o.properties["conflict_type"] == TABLE_WITHOUT_MODEL)
            .map(|o| &o.properties["explanation"])
            .collect();
        assert!(
            without
                .iter()
                .any(|e| e["kind"] == "translation_table" && e["parent"] == "c_invoice")
        );
        assert!(without.iter().any(|e| e["operation"] == "drop_table"));
        assert_eq!((stats.conflicts(), stats.explained), (6, 5));
    }

    /// A script dated before the time horizon but shipped in a newer release is still pending, and
    /// the database's own engine's copy is the one cited.
    #[test]
    fn a_release_newer_than_the_database_explains_an_early_dated_script() {
        let mut table = live_table("C_INVOICE", json!([{ "name": "OLDCOL" }]));
        table.properties.insert("dialect".into(), json!("oracle"));
        let objects = vec![
            table,
            model(
                "C_Invoice",
                json!({ "OldCol": { "source": "X.java:1" }, "C_CostCenter_ID": { "source": "X.java:2" } }),
            ),
            change(
                "add_column",
                "C_Invoice",
                Some("OldCol"),
                "2024-12-01T10:00",
                "migration/iD12/oracle/a.sql",
            ),
            change(
                "add_column",
                "C_Invoice",
                Some("C_CostCenter_ID"),
                "2024-05-14T13:59",
                "migration/iD13/postgresql/b.sql",
            ),
            change(
                "add_column",
                "C_Invoice",
                Some("C_CostCenter_ID"),
                "2024-05-14T13:59",
                "migration/iD13/oracle/b.sql",
            ),
            change(
                "create_table",
                "Later",
                None,
                "2026-03-01T10:00",
                "migration/iD13/oracle/c.sql",
            ),
        ];
        let (g, stats) = detect(&objects);
        assert_eq!(stats.horizon_release.as_deref(), Some("iD12"));
        let e = explanation(&g, "columns.C_CostCenter_ID").unwrap();
        assert_eq!(e["script"], "migration/iD13/oracle/b.sql");
        assert_eq!(e["release"], "iD13");
    }

    /// A profile turns "mandatory but nullable" into a measurement: rows that break the rule,
    /// none, or not measured — with the query that re-counts them.
    #[test]
    fn a_profile_measures_the_rows_that_break_a_mandatory_rule() {
        let table = live_table(
            "C_ORDER",
            json!([
                { "name": "DATEPROMISED", "profile": { "tier": "p2", "nulls": 3, "nulls_exact": true } },
                { "name": "DESCRIPTION", "profile": { "tier": "p0", "nulls": 0, "nulls_exact": false, "analyzed": "2024-12-01T10:00:00" } },
                { "name": "POREFERENCE" },
            ]),
        )
        .with_property(
            "profile",
            json!({ "tier": "p2", "row_count": 10, "row_count_exact": true }),
        );
        let objects = vec![
            table,
            model(
                "C_Order",
                json!({
                    "DatePromised": { "mandatory": true, "source": "X.java:1" },
                    "Description": { "mandatory": true, "source": "X.java:2" },
                    "POReference": { "mandatory": true, "source": "X.java:3" },
                }),
            ),
        ];
        let (g, stats) = detect(&objects);
        assert_eq!(stats.mandatory_but_nullable, 3);
        assert_eq!(
            (
                stats.nullable_holding_nulls,
                stats.nullable_without_nulls,
                stats.nullable_unmeasured,
                stats.nullable_counted_exactly
            ),
            (1, 1, 1, 1)
        );
        let measured = |attr: &str| {
            g.objects
                .iter()
                .find(|o| o.properties["attribute"] == attr)
                .and_then(|o| o.properties.get("measured"))
                .cloned()
        };
        let m = measured("columns.DatePromised.nullable").unwrap();
        assert_eq!(
            (m["violating_rows"].clone(), m["rows"].clone()),
            (json!(3), json!(10))
        );
        assert_eq!(
            m["sql"],
            "SELECT COUNT(*) FROM \"ADEMPIERE\".\"C_ORDER\" WHERE \"DATEPROMISED\" IS NULL"
        );
        assert_eq!(
            measured("columns.Description.nullable").unwrap()["measured_at"],
            "2024-12-01T10:00:00"
        );
        assert!(measured("columns.POReference.nullable").is_none());
    }

    /// A pending rename explains the new name a model already uses (iD13's
    /// `AD_Sequence_No RENAME COLUMN CalendarYearMonth TO SequenceKey`).
    #[test]
    fn a_pending_rename_explains_the_new_column_name() {
        let mut rename = change(
            "rename_column",
            "AD_Sequence_No",
            Some("CalendarYearMonth"),
            "2025-06-12T10:38",
            "migration/iD13/oracle/r.sql",
        );
        rename
            .properties
            .insert("name".into(), json!("SequenceKey"));
        let objects = vec![
            live_table("AD_SEQUENCE_NO", json!([{ "name": "CALENDARYEARMONTH" }])),
            model(
                "AD_Sequence_No",
                json!({ "SequenceKey": { "source": "X.java:1" } }),
            ),
            change(
                "add_column",
                "AD_Sequence_No",
                Some("CalendarYearMonth"),
                "2024-12-01T10:00",
                "migration/iD12/oracle/a.sql",
            ),
            rename,
        ];
        let (g, _) = detect(&objects);
        let e = explanation(&g, "columns.SequenceKey").unwrap();
        assert_eq!(e["operation"], "rename_column");
        assert_eq!(e["script"], "migration/iD13/oracle/r.sql");
    }

    #[test]
    fn a_model_of_a_view_matches_the_live_view() {
        let view = KirObject::new("AD_ALLCLIENTS_V", ObjectKind::Custom("View".into()))
            .with_property("origin", json!("live"));
        let objects = vec![
            view,
            model("AD_AllClients_V", json!({ "X": { "mandatory": true } })),
        ];
        let (g, stats) = detect(&objects);
        assert!(g.objects.is_empty());
        assert_eq!(stats.tables_compared, 1);
    }

    #[test]
    fn a_routine_recreated_by_migrations_is_compared_by_its_definition() {
        let objects = vec![
            change(
                "add_column",
                "T",
                Some("C"),
                "2020-01-01T00:00",
                "migration/i7/oracle/x.sql",
            ),
            routine("f", "db/oracle/functions/f.sql", "aaa"),
            routine("f", "migration/i7/oracle/x.sql", "zzz"),
            routine("F", "oracle://erp/ADEMPIERE/function/F", "aaa"),
        ];
        let (g, stats) = detect(&objects);
        assert_eq!((stats.routines_compared, stats.ambiguous), (1, 0));
        assert!(g.objects.is_empty());
    }

    #[test]
    fn nothing_is_compared_without_both_sides() {
        let only_live = vec![live_table("C_INVOICE", json!([]))];
        assert_eq!(detect(&only_live).1.conflicts(), 0);
        let only_model = vec![model("C_Invoice", json!({}))];
        assert_eq!(detect(&only_model).1.conflicts(), 0);
    }

    #[test]
    fn an_ambiguous_name_is_counted_not_compared() {
        let mut other_owner = live_table("C_INVOICE", json!([]));
        other_owner
            .properties
            .insert("owner".into(), json!("OTHER"));
        let objects = vec![
            live_table("C_INVOICE", json!([])),
            other_owner,
            model("C_Invoice", json!({ "X": { "mandatory": true } })),
        ];
        let (g, stats) = detect(&objects);
        assert_eq!(stats.ambiguous, 1);
        assert!(g.objects.is_empty());
    }
}
