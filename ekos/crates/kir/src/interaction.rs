//! RFC 0256 — the interaction map: who reads and writes each table, declared and observed.
//!
//! Pure: a [`Matrix`] of table × actor × operation, built from what is already in the ledger.
//!
//! **Declared** access comes from code ([`declared`]):
//!
//! | Actor kind | From |
//! |---|---|
//! | `routine` | a `Procedure`, `View` or `Trigger`'s footprint (`reads`, `writes`, `inserts`, `updates`, `deletes`, RFC 0163/0169); a trigger also writes its own table |
//! | `component` | a `JavaMethod`'s embedded SQL (`sql[]`, RFC 0209), grouped by the leading directories of the method's file |
//! | `etl_job` | a Kettle `TransformNode`'s `ReadsFrom` / `WritesTo` edge (RFC 0075), grouped by its `.ktr` / `.kjb` file |
//!
//! **Observed** access is added from `ObservedAccess` facts ([`Matrix::add_observed`], RFC 0256 §3.2),
//! whose actor is a `db_role`. An observation is dated and partial: every cell keeps the window.
//!
//! Only names of relations the ledger knows (`Table`, `View`, `Dataset`) are kept: a footprint also
//! names temporary tables, CTEs and record variables, which are not data stores.

use crate::{KirId, KirObject, KirRelationship, ObjectKind, RelationshipKind};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// What reads or writes (RFC 0256 §3.1). A closed set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorKind {
    Component,
    Routine,
    EtlJob,
    DbRole,
}

impl ActorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Component => "component",
            Self::Routine => "routine",
            Self::EtlJob => "etl_job",
            Self::DbRole => "db_role",
        }
    }
}

/// One actor: its kind and its name (a directory, a routine's name, a job's path, a role).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Actor {
    pub kind: ActorKind,
    pub name: String,
}

impl Actor {
    pub fn new(kind: ActorKind, name: impl Into<String>) -> Self {
        Self {
            kind,
            name: name.into(),
        }
    }
    /// `kind:name`, the form the CLI accepts and prints.
    pub fn key(&self) -> String {
        format!("{}:{}", self.kind.as_str(), self.name)
    }
}

/// An operation on a table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Op {
    Read,
    Insert,
    Update,
    Delete,
    /// A write whose kind the source does not say (a routine's `writes` without the split).
    Write,
}

impl Op {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Insert => "insert",
            Self::Update => "update",
            Self::Delete => "delete",
            Self::Write => "write",
        }
    }
    pub fn is_write(self) -> bool {
        self != Self::Read
    }
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "read" | "select" => Self::Read,
            "insert" => Self::Insert,
            "update" => Self::Update,
            "delete" | "truncate" => Self::Delete,
            "write" => Self::Write,
            _ => return None,
        })
    }
}

/// Observed calls and rows for one operation, over a dated window.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Observed {
    pub calls: u64,
    pub rows: u64,
    /// When the statistics were last reset, if the server says.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_start: Option<String>,
    pub window_end: String,
    /// The view evicted entries: the observation is incomplete.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub truncated: bool,
}

/// What one actor does to one table.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cell {
    /// Operations code states.
    pub declared: BTreeSet<Op>,
    /// Operations a database role was seen running.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub observed: BTreeMap<Op, Observed>,
    /// Where a declared operation is stated: object names (routines, methods, job steps).
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    pub via: BTreeSet<String>,
}

impl Cell {
    pub fn writes(&self) -> bool {
        self.declared.iter().any(|o| o.is_write())
            || self
                .observed
                .keys()
                .any(|o| o.is_write() && self.observed[o].calls > 0)
    }
}

/// Table (lower-case, bare name) → actor → what it does.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Matrix {
    pub tables: BTreeMap<String, BTreeMap<Actor, Cell>>,
}

impl Matrix {
    pub fn declare(&mut self, table: String, actor: &Actor, op: Op, via: &str) {
        let cell = self
            .tables
            .entry(table)
            .or_default()
            .entry(actor.clone())
            .or_default();
        cell.declared.insert(op);
        cell.via.insert(via.to_string());
    }

    /// Adds one observed (role, table, operation) aggregate.
    pub fn add_observed(&mut self, table: &str, role: &str, op: Op, seen: Observed) {
        let actor = Actor::new(ActorKind::DbRole, role);
        self.tables
            .entry(table_key(table))
            .or_default()
            .entry(actor)
            .or_default()
            .observed
            .insert(op, seen);
    }

    /// Every distinct actor, with the number of tables it touches.
    pub fn actors(&self) -> BTreeMap<&Actor, usize> {
        let mut out: BTreeMap<&Actor, usize> = BTreeMap::new();
        for row in self.tables.values() {
            for a in row.keys() {
                *out.entry(a).or_default() += 1;
            }
        }
        out
    }

    /// The rows for one table, writers first.
    pub fn row(&self, table: &str) -> Option<&BTreeMap<Actor, Cell>> {
        self.tables.get(&table_key(table))
    }

    /// Keeps only tables `keep` accepts (a restricted reader's scope).
    pub fn retain_tables(&mut self, keep: impl Fn(&str) -> bool) {
        self.tables.retain(|t, _| keep(t));
    }

    /// Keeps only cells whose actor `keep` accepts; drops tables left empty.
    pub fn retain_actors(&mut self, keep: impl Fn(&Actor) -> bool) {
        for row in self.tables.values_mut() {
            row.retain(|a, _| keep(a));
        }
        self.tables.retain(|_, row| !row.is_empty());
    }
}

/// The key a table is filed under: lower case, without schema or quotes.
pub fn table_key(name: &str) -> String {
    let bare = name.rsplit('.').next().unwrap_or(name);
    bare.trim_matches(|c| c == '"' || c == '`' || c == '[' || c == ']')
        .to_lowercase()
}

/// How a method's file is grouped into a component: the first `depth` directories of its path
/// (`org.adempiere.base/src/org/…/MOrder.java` → `org.adempiere.base` at depth 1). A file at the
/// top is its own directory's component, `.`.
pub fn component_of(path: &str, depth: usize) -> String {
    let dirs: Vec<&str> = path.split(['/', '\\']).filter(|s| !s.is_empty()).collect();
    let dirs = &dirs[..dirs.len().saturating_sub(1)];
    if dirs.is_empty() {
        return ".".into();
    }
    dirs[..depth.max(1).min(dirs.len())].join("/")
}

fn custom(o: &KirObject) -> Option<&str> {
    match &o.kind {
        ObjectKind::Custom(k) => Some(k.as_str()),
        _ => None,
    }
}

fn strings(v: Option<&Value>) -> impl Iterator<Item = &str> {
    v.and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
}

const FOOTPRINT: [(&str, Op); 5] = [
    ("reads", Op::Read),
    ("inserts", Op::Insert),
    ("updates", Op::Update),
    ("deletes", Op::Delete),
    ("writes", Op::Write),
];

/// The footprint of one routine, view or statement as (table, op). `writes` is kept only for
/// tables the split (`inserts` / `updates` / `deletes`) does not already name.
fn footprint<'a>(get: impl Fn(&str) -> Option<&'a Value>) -> BTreeSet<(String, Op)> {
    let mut out = BTreeSet::new();
    let mut split = BTreeSet::new();
    for (key, op) in FOOTPRINT {
        for t in strings(get(key)) {
            let t = table_key(t);
            if op == Op::Write && split.contains(&t) {
                continue;
            }
            if matches!(op, Op::Insert | Op::Update | Op::Delete) {
                split.insert(t.clone());
            }
            out.insert((t, op));
        }
    }
    out
}

/// Inputs to [`declared`] beside the objects and relationships.
#[derive(Debug, Clone)]
pub struct DeclaredInput<'a> {
    /// A `JavaMethod`'s source file by id (from its evidence).
    pub method_paths: &'a HashMap<KirId, String>,
    /// Directories that make a component (`[interaction] component-depth`).
    pub component_depth: usize,
}

/// The declared matrix: every access code states, to relations the ledger knows.
pub fn declared(
    objects: &[KirObject],
    relationships: &[KirRelationship],
    input: &DeclaredInput,
) -> Matrix {
    let known: BTreeSet<String> = objects
        .iter()
        .filter(|o| {
            matches!(o.kind, ObjectKind::Table | ObjectKind::Dataset) || custom(o) == Some("View")
        })
        .map(|o| table_key(&o.name))
        .collect();
    let mut m = Matrix::default();
    let mut put = |table: String, actor: &Actor, op: Op, via: &str| {
        if known.contains(&table) {
            m.declare(table, actor, op, via);
        }
    };
    // RFC 0211: a routine read from a live catalog is used only when no file defines one of the
    // same name — the file's routine is the same code, and its footprint is the one cited.
    let is_live =
        |o: &KirObject| o.properties.get("origin").and_then(Value::as_str) == Some("live");
    let routine_kinds = ["Procedure", "View", "Trigger"];
    let from_files: BTreeSet<(String, String)> = objects
        .iter()
        .filter(|o| !is_live(o))
        .filter_map(|o| {
            custom(o)
                .filter(|k| routine_kinds.contains(k))
                .map(|k| (k, o))
        })
        .map(|(k, o)| (k.to_string(), table_key(&o.name)))
        .collect();
    for o in objects {
        match custom(o) {
            Some(k @ ("Procedure" | "View" | "Trigger"))
                if is_live(o) && from_files.contains(&(k.to_string(), table_key(&o.name))) => {}
            Some(k @ ("Procedure" | "View" | "Trigger")) => {
                let actor = Actor::new(ActorKind::Routine, &o.name);
                let via = format!("{} {}", k.to_lowercase(), o.name);
                for (t, op) in footprint(|k| o.properties.get(k)) {
                    put(t, &actor, op, &via);
                }
                if k == "Trigger"
                    && let Some(t) = o.properties.get("table").and_then(Value::as_str)
                {
                    put(table_key(t), &actor, Op::Write, &via);
                }
            }
            Some("JavaMethod") => {
                let Some(path) = input.method_paths.get(&o.id) else {
                    continue;
                };
                let actor = Actor::new(
                    ActorKind::Component,
                    component_of(path, input.component_depth),
                );
                let via = format!("{path}: {}", o.name);
                let sql = o.properties.get("sql").and_then(Value::as_array);
                for st in sql.into_iter().flatten().filter_map(Value::as_object) {
                    for (t, op) in footprint(|k| st.get(k)) {
                        put(t, &actor, op, &via);
                    }
                }
            }
            _ => {}
        }
    }
    let by_id: HashMap<KirId, &KirObject> = objects.iter().map(|o| (o.id, o)).collect();
    for r in relationships {
        let op = match &r.kind {
            RelationshipKind::Custom(k) if k == "ReadsFrom" => Op::Read,
            RelationshipKind::Custom(k) if k == "WritesTo" => Op::Write,
            _ => continue,
        };
        let (Some(node), Some(table)) = (by_id.get(&r.from), by_id.get(&r.to)) else {
            continue;
        };
        if custom(node) != Some("TransformNode") {
            continue;
        }
        // A node is named `<file>:<index>`; only Kettle files are ETL jobs (a standalone SELECT's
        // nodes are SQL, already covered by its routine or view).
        let Some((file, _)) = node.name.rsplit_once(':') else {
            continue;
        };
        if !(file.ends_with(".ktr") || file.ends_with(".kjb")) {
            continue;
        }
        let actor = Actor::new(ActorKind::EtlJob, file);
        put(table_key(&table.name), &actor, op, &node.name);
    }
    m
}

// ── Links and findings (RFC 0256 §3.3–3.4) ────────────────────────────────────────────────────

/// One database's observation window, as the CLI read it from its `ObservationWindow` fact.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Window {
    pub database: String,
    /// Days between the statistics' reset and the read; `None` when the reset time is unknown.
    pub days: Option<f64>,
    pub truncated: bool,
    /// Table → rows (inserted, updated, deleted) since the table counters' reset.
    pub table_writes: BTreeMap<String, (u64, u64, u64)>,
}

/// A finding type (RFC 0256 §3.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingKind {
    UnexplainedWriter,
    UnattributedWrites,
    MultipleWritingSystems,
    DeclaredNotObserved,
}

impl FindingKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UnexplainedWriter => "unexplained_writer",
            Self::UnattributedWrites => "unattributed_writes",
            Self::MultipleWritingSystems => "multiple_writing_systems",
            Self::DeclaredNotObserved => "declared_not_observed",
        }
    }
}

/// One finding: what, on which table, by whom, with the facts that put it there.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Finding {
    pub kind: FindingKind,
    pub table: String,
    /// The actor it is about (a role, or the declaring actor); `None` for a table-level finding.
    pub actor: Option<Actor>,
    pub operations: BTreeSet<Op>,
    /// The facts in words-ready form: counts, the other writers, the window.
    pub detail: Value,
}

impl Finding {
    /// A stable key: kind, table and actor — never counts, so a re-observation keeps the id.
    pub fn key(&self) -> String {
        format!(
            "{}:{}:{}",
            self.kind.as_str(),
            self.table,
            self.actor.as_ref().map(Actor::key).unwrap_or_default()
        )
    }
}

fn is_system(kind: ActorKind) -> bool {
    matches!(kind, ActorKind::Component | ActorKind::EtlJob)
}

/// Whether `declared` covers `op` (a bare `write` covers every write).
fn covers(declared: &BTreeSet<Op>, op: Op) -> bool {
    declared.contains(&op) || (op.is_write() && declared.contains(&Op::Write))
}

/// Role → the one component or ETL job whose tables contain every table the role touches
/// (RFC 0256 §3.3). A role two actors could explain, or none, has no hypothesis.
pub fn link_hypotheses(m: &Matrix) -> BTreeMap<String, Actor> {
    let mut touched: BTreeMap<&Actor, BTreeSet<&String>> = BTreeMap::new();
    for (t, row) in &m.tables {
        for a in row.keys() {
            touched.entry(a).or_default().insert(t);
        }
    }
    let systems: Vec<(&Actor, &BTreeSet<&String>)> = touched
        .iter()
        .filter(|(a, _)| is_system(a.kind))
        .map(|(a, t)| (*a, t))
        .collect();
    let mut out = BTreeMap::new();
    for (role, tables) in touched.iter().filter(|(a, _)| a.kind == ActorKind::DbRole) {
        let fits: Vec<&Actor> = systems
            .iter()
            .filter(|(_, t)| tables.is_subset(t))
            .map(|(a, _)| *a)
            .collect();
        if let [one] = fits.as_slice() {
            out.insert(role.name.clone(), (*one).clone());
        }
    }
    out
}

/// What [`findings`] reads beside the matrix.
#[derive(Debug, Clone)]
pub struct FindingInput<'a> {
    pub windows: &'a [Window],
    /// Roles a person accounted for (a confirmed review decision on the role, still valid).
    pub accounted: &'a BTreeSet<String>,
    /// Role → the actor it is linked to (hypothesis or confirmed).
    pub links: &'a BTreeMap<String, Actor>,
    /// `[interaction] quiet-after-days`.
    pub quiet_after_days: u32,
}

static NO_ROLES: BTreeSet<String> = BTreeSet::new();
static NO_LINKS: BTreeMap<String, Actor> = BTreeMap::new();

impl Default for FindingInput<'_> {
    fn default() -> Self {
        Self {
            windows: &[],
            accounted: &NO_ROLES,
            links: &NO_LINKS,
            quiet_after_days: 30,
        }
    }
}

/// Whether a role's observed write of `op` on a table is explained: the role was accounted for by
/// a person; or the actor it is linked to declares that write; or, with no link, some code actor
/// declares it (the role runs code EKOS knows — RFC 0256 log, phase 3).
fn explained(row: &BTreeMap<Actor, Cell>, role: &str, op: Op, input: &FindingInput) -> bool {
    if input.accounted.contains(role) {
        return true;
    }
    let declares = |a: &Actor| row.get(a).is_some_and(|c| covers(&c.declared, op));
    match input.links.get(role) {
        Some(actor) => declares(actor),
        None => row
            .iter()
            .any(|(a, c)| a.kind != ActorKind::DbRole && covers(&c.declared, op)),
    }
}

fn observed_writes(c: &Cell) -> impl Iterator<Item = (Op, &Observed)> {
    c.observed
        .iter()
        .filter(|(op, o)| op.is_write() && o.calls > 0)
        .map(|(op, o)| (*op, o))
}

/// The findings of one table's row: unexplained writers, and several writing systems.
fn row_findings(
    table: &str,
    row: &BTreeMap<Actor, Cell>,
    input: &FindingInput,
    out: &mut Vec<Finding>,
) {
    let mut unexplained_roles = Vec::new();
    for (a, c) in row.iter().filter(|(a, _)| a.kind == ActorKind::DbRole) {
        let ops: BTreeSet<Op> = observed_writes(c)
            .filter(|(op, _)| !explained(row, &a.name, *op, input))
            .map(|(op, _)| op)
            .collect();
        if ops.is_empty() {
            continue;
        }
        let calls: u64 = ops.iter().map(|op| c.observed[op].calls).sum();
        let window = c.observed.values().next();
        out.push(Finding {
            kind: FindingKind::UnexplainedWriter,
            table: table.to_string(),
            actor: Some(a.clone()),
            operations: ops,
            detail: serde_json::json!({
                "calls": calls,
                "window_start": window.and_then(|w| w.window_start.clone()),
                "window_end": window.map(|w| w.window_end.clone()),
                "truncated": window.is_some_and(|w| w.truncated),
            }),
        });
        unexplained_roles.push(a.clone());
    }
    let mut systems: Vec<String> = row
        .iter()
        .filter(|(a, c)| is_system(a.kind) && c.declared.iter().any(|o| o.is_write()))
        .map(|(a, _)| a.key())
        .chain(unexplained_roles.iter().map(Actor::key))
        .collect();
    systems.sort();
    systems.dedup();
    if systems.len() >= 2 {
        out.push(Finding {
            kind: FindingKind::MultipleWritingSystems,
            table: table.to_string(),
            actor: None,
            operations: BTreeSet::new(),
            detail: serde_json::json!({ "writers": systems }),
        });
    }
}

/// Writes a table's counters show that no observed statement in the window explains.
fn unattributed(window: &Window, m: &Matrix, out: &mut Vec<Finding>) {
    for (table, &(ins, upd, del)) in &window.table_writes {
        let observed: BTreeSet<Op> = m
            .row(table)
            .into_iter()
            .flat_map(BTreeMap::values)
            .flat_map(|c| observed_writes(c).map(|(op, _)| op))
            .collect();
        let missing: BTreeMap<Op, u64> = [(Op::Insert, ins), (Op::Update, upd), (Op::Delete, del)]
            .into_iter()
            .filter(|(op, n)| *n > 0 && !covers(&observed, *op))
            .collect();
        if missing.is_empty() {
            continue;
        }
        out.push(Finding {
            kind: FindingKind::UnattributedWrites,
            table: table.clone(),
            actor: None,
            operations: missing.keys().copied().collect(),
            detail: serde_json::json!({
                "database": window.database,
                "rows": missing.iter().map(|(op, n)| (op.as_str(), n)).collect::<BTreeMap<_, _>>(),
                "note": "the table counters and the statement statistics may have been reset at different times",
            }),
        });
    }
}

/// Declared writes no role was seen running, on a table of a database observed long enough
/// (`quiet_after_days`) and not truncated. Low confidence by construction.
fn not_observed(window: &Window, m: &Matrix, quiet: u32, out: &mut Vec<Finding>) {
    if window.truncated || window.days.is_none_or(|d| d <= f64::from(quiet)) {
        return;
    }
    for table in window.table_writes.keys() {
        let Some(row) = m.row(table) else {
            continue;
        };
        let observed: BTreeSet<Op> = row
            .values()
            .flat_map(|c| observed_writes(c).map(|(op, _)| op))
            .collect();
        for (a, c) in row.iter().filter(|(a, _)| a.kind != ActorKind::DbRole) {
            let ops: BTreeSet<Op> = c
                .declared
                .iter()
                .copied()
                .filter(|op| op.is_write())
                .filter(|op| !(observed.contains(op) || (*op == Op::Write && !observed.is_empty())))
                .collect();
            if !ops.is_empty() {
                out.push(Finding {
                    kind: FindingKind::DeclaredNotObserved,
                    table: table.clone(),
                    actor: Some(a.clone()),
                    operations: ops,
                    detail: serde_json::json!({ "database": window.database, "window_days": window.days }),
                });
            }
        }
    }
}

/// Every finding, sorted by key. Pure: the same inputs give the same findings in the same order.
/// Nothing is derived from absence when no window was observed.
pub fn findings(m: &Matrix, input: &FindingInput) -> Vec<Finding> {
    let mut out = Vec::new();
    if input.windows.is_empty() {
        return out;
    }
    for (table, row) in &m.tables {
        row_findings(table, row, input, &mut out);
    }
    for w in input.windows {
        unattributed(w, m, &mut out);
        not_observed(w, m, input.quiet_after_days, &mut out);
    }
    out.sort_by_key(Finding::key);
    out.dedup_by(|a, b| a.key() == b.key());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn obj(name: &str, kind: &str, props: Value) -> KirObject {
        let mut o = KirObject::new(name, ObjectKind::Custom(kind.into()));
        o.properties = props.as_object().unwrap().clone().into_iter().collect();
        o
    }

    fn table(name: &str) -> KirObject {
        KirObject::new(name, ObjectKind::Table)
    }

    #[test]
    fn routines_methods_and_jobs_become_actors_on_known_tables_only() {
        let proc = obj(
            "post_invoice",
            "Procedure",
            json!({"reads": ["public.ar", "tmp_x"], "writes": ["gl", "acc_trans"], "updates": ["acc_trans"]}),
        );
        let trig = obj(
            "audit_gl",
            "Trigger",
            json!({"table": "gl", "inserts": ["audit"]}),
        );
        let method = obj(
            "MOrder.purge",
            "JavaMethod",
            json!({"sql": [{"text": "DELETE FROM c_order WHERE x", "deletes": ["C_Order"]}]}),
        );
        let node = obj(
            "jobs/load.ktr:3",
            "TransformNode",
            json!({"node_type": "Sink"}),
        );
        let objects = vec![
            table("ar"),
            table("gl"),
            table("acc_trans"),
            table("audit"),
            table("c_order"),
            proc,
            trig,
            method.clone(),
            node.clone(),
        ];
        let rel = KirRelationship::new(
            RelationshipKind::Custom("WritesTo".into()),
            node.id,
            objects[1].id,
        );
        let paths = HashMap::from([(
            method.id,
            "org.adempiere.base/src/org/compiere/model/MOrder.java".to_string(),
        )]);
        let m = declared(
            &objects,
            &[rel],
            &DeclaredInput {
                method_paths: &paths,
                component_depth: 1,
            },
        );
        assert!(m.row("tmp_x").is_none(), "an unknown name is not a table");
        let gl = m.row("gl").unwrap();
        let kinds: BTreeSet<ActorKind> = gl.keys().map(|a| a.kind).collect();
        assert_eq!(
            kinds,
            BTreeSet::from([ActorKind::Routine, ActorKind::EtlJob])
        );
        // `writes` is dropped where the split names the table.
        let acc = &m.row("acc_trans").unwrap()[&Actor::new(ActorKind::Routine, "post_invoice")];
        assert_eq!(acc.declared, BTreeSet::from([Op::Update]));
        let order = m.row("C_ORDER").unwrap();
        let comp = Actor::new(ActorKind::Component, "org.adempiere.base");
        assert_eq!(order[&comp].declared, BTreeSet::from([Op::Delete]));
        assert!(order[&comp].writes());
        assert_eq!(m.row("ar").unwrap().len(), 1);
    }

    #[test]
    fn a_live_routine_defined_in_a_file_counts_once() {
        let file = obj("post", "Procedure", json!({"writes": ["gl"]}));
        let live = obj(
            "public.post",
            "Procedure",
            json!({"origin": "live", "writes": ["gl"]}),
        );
        let only_live = obj(
            "public.other",
            "Procedure",
            json!({"origin": "live", "writes": ["gl"]}),
        );
        let paths = HashMap::new();
        let input = DeclaredInput {
            method_paths: &paths,
            component_depth: 1,
        };
        let m = declared(&[table("gl"), file, live, only_live], &[], &input);
        let names: Vec<&str> = m
            .row("gl")
            .unwrap()
            .keys()
            .map(|a| a.name.as_str())
            .collect();
        assert_eq!(names, ["post", "public.other"]);
    }

    #[test]
    fn a_sql_transform_node_is_not_an_etl_job() {
        let node = obj("q.sql:0", "TransformNode", json!({"node_type": "Source"}));
        let t = table("ar");
        let rel = KirRelationship::new(RelationshipKind::Custom("ReadsFrom".into()), node.id, t.id);
        let paths = HashMap::new();
        let input = DeclaredInput {
            method_paths: &paths,
            component_depth: 1,
        };
        let m = declared(&[t, node], &[rel], &input);
        assert!(m.tables.is_empty());
    }

    #[test]
    fn components_group_by_leading_directories() {
        assert_eq!(component_of("a/b/c/X.java", 1), "a");
        assert_eq!(component_of("a/b/c/X.java", 2), "a/b");
        assert_eq!(component_of("a/X.java", 3), "a");
        assert_eq!(component_of("X.java", 1), ".");
        assert_eq!(table_key("public.\"AR\""), "ar");
    }

    #[test]
    fn observed_cells_sit_beside_declared_ones() {
        let mut m = Matrix::default();
        m.add_observed(
            "public.gl",
            "etl_nightly",
            Op::Update,
            Observed {
                calls: 3,
                rows: 30,
                window_end: "2026-11-11T00:00:00Z".into(),
                ..Default::default()
            },
        );
        let row = m.row("gl").unwrap();
        let cell = &row[&Actor::new(ActorKind::DbRole, "etl_nightly")];
        assert!(cell.writes() && cell.declared.is_empty());
        m.retain_tables(|t| t != "gl");
        assert!(m.tables.is_empty());
    }

    fn observed(calls: u64) -> Observed {
        Observed {
            calls,
            rows: calls,
            window_start: Some("2026-11-01".into()),
            window_end: "2026-11-11".into(),
            truncated: false,
        }
    }

    /// The fixture of the phase 2 exit: `app` runs the declared writes, `etl` updates a table no
    /// code writes, `bi` reads.
    fn fixture() -> Matrix {
        let mut m = Matrix::default();
        let routine = Actor::new(ActorKind::Routine, "place_order");
        m.declare("orders".into(), &routine, Op::Insert, "place_order");
        m.declare("orders".into(), &routine, Op::Update, "pay_order");
        m.add_observed("orders", "app", Op::Insert, observed(20));
        m.add_observed("orders", "app", Op::Update, observed(5));
        m.add_observed("customers", "etl", Op::Update, observed(3));
        m.add_observed("customers", "bi", Op::Read, observed(7));
        m
    }

    fn window(days: f64, writes: &[(&str, (u64, u64, u64))]) -> Window {
        Window {
            database: "shop".into(),
            days: Some(days),
            truncated: false,
            table_writes: writes.iter().map(|(t, w)| (t.to_string(), *w)).collect(),
        }
    }

    #[test]
    fn a_role_writing_where_no_code_writes_is_an_unexplained_writer() {
        let m = fixture();
        let windows = [window(
            3.0,
            &[("orders", (20, 5, 0)), ("customers", (0, 3, 0))],
        )];
        let none = BTreeSet::new();
        let links = BTreeMap::new();
        let input = FindingInput {
            windows: &windows,
            accounted: &none,
            links: &links,
            quiet_after_days: 30,
        };
        let f = findings(&m, &input);
        let kinds: Vec<(FindingKind, &str)> =
            f.iter().map(|x| (x.kind, x.table.as_str())).collect();
        assert_eq!(
            kinds,
            [(FindingKind::UnexplainedWriter, "customers")],
            "{f:?}"
        );
        assert_eq!(f[0].actor.as_ref().unwrap().name, "etl");
        // A person accounts for the role: the finding goes.
        let accounted = BTreeSet::from(["etl".to_string()]);
        let input = FindingInput {
            accounted: &accounted,
            ..input
        };
        assert!(findings(&m, &input).is_empty());
    }

    #[test]
    fn nothing_is_derived_from_absence_without_an_observation() {
        let m = fixture();
        assert!(findings(&m, &FindingInput::default()).is_empty());
    }

    #[test]
    fn counters_without_an_observed_statement_are_unattributed() {
        let m = fixture();
        let windows = [window(3.0, &[("customers", (50, 3, 0))])];
        let acc = BTreeSet::from(["etl".to_string()]);
        let links = BTreeMap::new();
        let f = findings(
            &m,
            &FindingInput {
                windows: &windows,
                accounted: &acc,
                links: &links,
                quiet_after_days: 30,
            },
        );
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, FindingKind::UnattributedWrites);
        assert_eq!(f[0].operations, BTreeSet::from([Op::Insert]));
    }

    #[test]
    fn declared_not_observed_needs_a_long_untruncated_window() {
        let mut m = fixture();
        m.declare(
            "orders".into(),
            &Actor::new(ActorKind::Routine, "purge"),
            Op::Delete,
            "purge",
        );
        let acc = BTreeSet::from(["etl".to_string()]);
        let links = BTreeMap::new();
        let short = [window(3.0, &[("orders", (20, 5, 0))])];
        let input = FindingInput {
            windows: &short,
            accounted: &acc,
            links: &links,
            quiet_after_days: 30,
        };
        assert!(findings(&m, &input).is_empty());
        let long = [window(45.0, &[("orders", (20, 5, 0))])];
        let f = findings(
            &m,
            &FindingInput {
                windows: &long,
                ..input.clone()
            },
        );
        assert_eq!(f.len(), 1, "{f:?}");
        assert_eq!(f[0].kind, FindingKind::DeclaredNotObserved);
        assert_eq!(f[0].actor.as_ref().unwrap().name, "purge");
        let mut truncated = long.clone();
        truncated[0].truncated = true;
        assert!(
            findings(
                &m,
                &FindingInput {
                    windows: &truncated,
                    ..input
                }
            )
            .is_empty()
        );
    }

    #[test]
    fn a_role_is_linked_only_to_the_one_system_covering_its_tables() {
        let mut m = fixture();
        let erp = Actor::new(ActorKind::Component, "erp");
        let crm = Actor::new(ActorKind::Component, "crm");
        m.declare("customers".into(), &erp, Op::Update, "x");
        m.declare("orders".into(), &crm, Op::Read, "y");
        // `etl` touches customers only: erp alone covers it.
        assert_eq!(link_hypotheses(&m).get("etl"), Some(&erp));
        // A second system touching customers: no hypothesis for either role.
        m.declare("customers".into(), &crm, Op::Read, "y");
        let links = link_hypotheses(&m);
        assert!(!links.contains_key("etl") && !links.contains_key("bi"));
        // A linked role is explained only by its own actor's declared writes.
        let windows = [window(3.0, &[])];
        let none = BTreeSet::new();
        let to_crm = BTreeMap::from([("etl".to_string(), crm.clone())]);
        let f = findings(
            &m,
            &FindingInput {
                windows: &windows,
                accounted: &none,
                links: &to_crm,
                quiet_after_days: 30,
            },
        );
        assert!(
            f.iter()
                .any(|x| x.kind == FindingKind::UnexplainedWriter && x.table == "customers")
        );
        let to_erp = BTreeMap::from([("etl".to_string(), erp)]);
        let f = findings(
            &m,
            &FindingInput {
                windows: &windows,
                accounted: &none,
                links: &to_erp,
                quiet_after_days: 30,
            },
        );
        assert!(!f.iter().any(|x| x.kind == FindingKind::UnexplainedWriter));
    }

    #[test]
    fn two_writing_systems_on_one_table_are_reported() {
        let mut m = Matrix::default();
        m.declare(
            "gl".into(),
            &Actor::new(ActorKind::Component, "erp"),
            Op::Insert,
            "a",
        );
        m.declare(
            "gl".into(),
            &Actor::new(ActorKind::EtlJob, "load.ktr"),
            Op::Write,
            "b",
        );
        let windows = [window(1.0, &[])];
        let none = BTreeSet::new();
        let links = BTreeMap::new();
        let f = findings(
            &m,
            &FindingInput {
                windows: &windows,
                accounted: &none,
                links: &links,
                quiet_after_days: 30,
            },
        );
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, FindingKind::MultipleWritingSystems);
    }
}
