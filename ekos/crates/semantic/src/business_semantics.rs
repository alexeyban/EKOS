//! RFC 0170 — business-semantics **hypotheses** from technical traces.
//!
//! Reads what recovery recorded — the normalized column-vs-literal `predicates` on `View`,
//! `ProcedureStatement` and `LANGUAGE sql` `Procedure` objects, and each `Table`'s
//! `check_constraints`, `seed_rows` and column comments — and synthesizes:
//!
//! | Kind | What |
//! |---|---|
//! | `BusinessConcept` | A filter that defines a view, or the same filter recurring in `min_sites`+ places |
//! | `EnumMeaning` | One coded value of one column, with every meaning source found |
//! | `ConstraintCandidate` | One `CHECK` constraint, typed (`range`/`enum`/`pattern`/…) |
//! | `SemanticGap` | A coded value nobody explained; a concept nobody documented |
//! | `RationaleLink` | The commit that last touched a concept's or gap's evidence line (`git blame`) |
//!
//! Pure and deterministic: ids are v5 UUIDs of structural keys, so a re-run on unchanged input
//! yields identical objects, and the one source of outside facts — `git blame` — comes in through
//! [`RationaleSource`], which the CLI implements. Everything is `status: hypothesis`: a human
//! confirms (RFC 0170 Phase 2), never this module. Runs at `ekos commit`, after RFC 0163's
//! `procedure_lineage`, because a predicate in one file names a table created in another.

use crate::procedure_lineage::NameIndex;
use ekos_kir::predicates::{Clause, PredicateSite, canonical_text};
use ekos_kir::{
    KirEvidence, KirGraph, KirId, KirObject, KirRelationship, ObjectKind, RelationshipKind,
    SourceLocation,
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use uuid::Uuid;

pub const CONCEPT: &str = "BusinessConcept";
pub const ENUM_MEANING: &str = "EnumMeaning";
pub const CONSTRAINT: &str = "ConstraintCandidate";
pub const GAP: &str = "SemanticGap";
pub const RATIONALE: &str = "RationaleLink";

/// Every kind this module writes.
pub const KINDS: [&str; 5] = [CONCEPT, ENUM_MEANING, CONSTRAINT, GAP, RATIONALE];

/// Relationship kinds: item → the table/view it describes; concept → where it was seen;
/// concept/gap → the commit that explains it.
pub const DESCRIBES: &str = "Describes";
pub const EVIDENCED_BY: &str = "EvidencedBy";
pub const EXPLAINED_BY: &str = "ExplainedBy";

/// The status every synthesized item starts with.
pub const HYPOTHESIS: &str = "hypothesis";

/// Evidence records kept per item; the count is always recorded in full.
const MAX_EVIDENCE: usize = 12;
/// Lines blamed per item.
const MAX_BLAME_LINES: usize = 6;

/// Tunables (`[semantics]` in `ekos.toml`).
#[derive(Debug, Clone)]
pub struct SemanticsConfig {
    /// Distinct statements/views a filter must recur in to become a concept.
    pub min_sites: usize,
    /// A column compared against more distinct literals than this is a lookup key
    /// (`defaults.setting_key`), not a classification: no concepts, no enum.
    pub max_enum_values: usize,
}

impl Default for SemanticsConfig {
    fn default() -> Self {
        Self {
            min_sites: 2,
            max_enum_values: 12,
        }
    }
}

/// One `git blame` answer: the commit that last touched `line` of a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlameHit {
    pub line: u32,
    pub sha: String,
    pub summary: String,
    pub author: String,
    /// ISO-8601 author date.
    pub date: String,
}

/// Where rationale comes from. The CLI implements it over `git blame`; tests use a fixture.
pub trait RationaleSource {
    /// The commits that last touched `lines` of `path` (a ledger evidence path). Lines it cannot
    /// answer for are simply absent.
    fn blame(&self, path: &str, lines: &[u32]) -> Vec<BlameHit>;
}

/// What one run produced.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SemanticsStats {
    pub sites: usize,
    pub sites_resolved: usize,
    pub concepts: usize,
    pub concepts_from_views: usize,
    pub coded_columns: usize,
    pub enum_values: usize,
    pub enum_values_explained: usize,
    pub key_like_columns: usize,
    pub constraints: usize,
    pub gaps: usize,
    pub rationale_links: usize,
}

/// Everything synthesized, ready to append.
#[derive(Debug, Clone, Default)]
pub struct SemanticsOutput {
    pub objects: Vec<KirObject>,
    pub relationships: Vec<KirRelationship>,
    pub evidence: Vec<KirEvidence>,
    pub stats: SemanticsStats,
}

/// The `ObjectKind` an RFC 0170 kind name is stored as. Concepts use the built-in
/// `ObjectKind::BusinessConcept` ("a named business concept or term"): a `Custom("BusinessConcept")`
/// would serialize to the same string and deserialize back as the built-in variant anyway.
pub fn object_kind(kind: &str) -> ObjectKind {
    if kind == CONCEPT {
        ObjectKind::BusinessConcept
    } else {
        ObjectKind::Custom(kind.into())
    }
}

/// The RFC 0170 kind name of `o`, if it is one of [`KINDS`].
pub fn kind_name(o: &KirObject) -> Option<&'static str> {
    match &o.kind {
        ObjectKind::BusinessConcept => Some(CONCEPT),
        ObjectKind::Custom(k) => KINDS.iter().copied().find(|x| x == k),
        _ => None,
    }
}

fn kid(key: &str) -> KirId {
    KirId(Uuid::new_v5(&Uuid::NAMESPACE_URL, key.as_bytes()))
}

fn is_custom(o: &KirObject, kind: &str) -> bool {
    matches!(&o.kind, ObjectKind::Custom(k) if k == kind)
}

fn tail(n: &str) -> String {
    n.rsplit('.').next().unwrap_or(n).to_lowercase()
}

/// `entity_credit_account` → `EntityCreditAccount`; `'A'` → `A`; `-1` → `Minus1`.
pub fn camel(s: &str) -> String {
    let s = s.trim_matches('\'');
    let s = s
        .strip_prefix('-')
        .map(|r| format!("minus_{r}"))
        .unwrap_or(s.to_string());
    let mut out = String::new();
    let mut upper = true;
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            if upper {
                out.extend(c.to_uppercase());
            } else {
                out.push(c);
            }
            upper = false;
        } else {
            upper = true;
        }
    }
    out
}

fn unquote(v: &str) -> String {
    v.strip_prefix('\'')
        .and_then(|r| r.strip_suffix('\''))
        .map(|r| r.replace("''", "'"))
        .unwrap_or_else(|| v.to_string())
}

// ── Tables ──────────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
struct Column {
    data_type: String,
    /// A single-column primary key or unique column: comparing it to a literal picks one row
    /// (`defaults.setting_key = 'curr'`), it does not classify rows.
    key: bool,
    description: Option<String>,
    description_path: Option<String>,
    description_line: Option<u32>,
}

struct TableInfo<'a> {
    obj: &'a KirObject,
    columns: BTreeMap<String, Column>,
}

impl TableInfo<'_> {
    fn name(&self) -> String {
        tail(&self.obj.name)
    }
}

fn table_info(o: &KirObject) -> TableInfo<'_> {
    let mut columns = BTreeMap::new();
    if let Some(Value::Array(cols)) = o.properties.get("columns") {
        for c in cols {
            let Some(name) = c.get("name").and_then(Value::as_str) else {
                continue;
            };
            columns.insert(
                name.to_lowercase(),
                Column {
                    key: ["primary_key", "unique"]
                        .iter()
                        .any(|k| c.get(*k).and_then(Value::as_bool).unwrap_or(false)),
                    data_type: c
                        .get("data_type")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_lowercase(),
                    description: c
                        .get("description")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    description_path: c
                        .get("description_path")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    description_line: c
                        .get("description_line")
                        .and_then(Value::as_u64)
                        .map(|l| l as u32),
                },
            );
        }
    }
    TableInfo { obj: o, columns }
}

/// A type whose values are quantities or instants, never codes: comparing an amount to `0` is
/// arithmetic, not a classification.
fn is_quantity_type(t: &str) -> bool {
    [
        "numeric", "decimal", "real", "double", "float", "money", "date", "time", "interval",
    ]
    .iter()
    .any(|q| t.contains(q))
}

// ── Sites ───────────────────────────────────────────────────────────────────────────────────

/// A predicate site resolved to a table, with where it was found.
#[derive(Debug, Clone)]
struct Site {
    table: usize,
    column: String,
    p: PredicateSite,
    carrier: KirId,
    carrier_name: String,
    carrier_is_view: bool,
    path: String,
}

impl Site {
    fn canonical(&self, tables: &[TableInfo]) -> String {
        canonical_text(
            &format!("{}.{}", tables[self.table].name(), self.column),
            &self.p.op,
            &self.p.values,
        )
    }
}

fn carrier_path(o: &KirObject) -> String {
    o.properties
        .get("source_path")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// Every site on every carrier, resolved to a table where that is unambiguous.
fn collect_sites(
    graph: &KirGraph,
    tables: &[TableInfo],
    index: &NameIndex,
    by_id: &HashMap<Uuid, usize>,
    stats: &mut SemanticsStats,
) -> Vec<Site> {
    let resolve_table = |name: &str| -> Option<usize> {
        let (id, _) = index.resolve(name).ok()?;
        by_id.get(&id.0).copied()
    };
    let mut out = Vec::new();
    for o in &graph.objects {
        let carrier_is_view = is_custom(o, "View");
        if !(carrier_is_view || is_custom(o, "ProcedureStatement") || is_custom(o, "Procedure")) {
            continue;
        }
        let Some(Value::Array(preds)) = o.properties.get("predicates") else {
            continue;
        };
        for v in preds {
            let Ok(p) = serde_json::from_value::<PredicateSite>(v.clone()) else {
                continue;
            };
            stats.sites += 1;
            let table = match &p.relation {
                Some(r) => resolve_table(r),
                None => {
                    // An unqualified column: the one in-scope table that declares it.
                    let owners: BTreeSet<usize> = p
                        .scope
                        .iter()
                        .filter_map(|r| resolve_table(r))
                        .filter(|&t| tables[t].columns.contains_key(&p.column))
                        .collect();
                    (owners.len() == 1).then(|| *owners.iter().next().unwrap())
                }
            };
            let Some(table) = table else {
                continue;
            };
            // A column the table does not declare is a computed alias or a typo; not a fact about
            // the table. (A table recovered without columns cannot be checked, and is trusted.)
            if !tables[table].columns.is_empty() && !tables[table].columns.contains_key(&p.column) {
                continue;
            }
            stats.sites_resolved += 1;
            out.push(Site {
                table,
                column: p.column.clone(),
                carrier: o.id,
                carrier_name: o.name.clone(),
                carrier_is_view,
                path: carrier_path(o),
                p,
            });
        }
    }
    out
}

// ── Evidence ────────────────────────────────────────────────────────────────────────────────

struct Builder {
    out: SemanticsOutput,
}

impl Builder {
    fn evidence(
        &mut self,
        owner: &str,
        n: usize,
        path: &str,
        line: Option<u32>,
        text: String,
    ) -> KirId {
        let mut ev = KirEvidence::new(
            SourceLocation {
                path: path.to_string(),
                line,
                column: None,
            },
            text,
        );
        ev.id = kid(&format!("semantics-evidence:{owner}:{n}"));
        let id = ev.id;
        self.out.evidence.push(ev);
        id
    }

    fn relate(&mut self, kind: &str, from: KirId, to: KirId) {
        self.out.relationships.push(KirRelationship::deterministic(
            RelationshipKind::Custom(kind.into()),
            from,
            to,
            "",
        ));
    }

    fn object(&mut self, key: &str, name: String, kind: &str, props: Vec<(&str, Value)>) -> usize {
        let mut o = KirObject::new(name, object_kind(kind));
        o.id = kid(key);
        o.properties.insert("status".into(), json!(HYPOTHESIS));
        for (k, v) in props {
            o.properties.insert(k.into(), v);
        }
        self.out.objects.push(o);
        self.out.objects.len() - 1
    }
}

fn site_line(s: &Site) -> Option<u32> {
    (s.p.line > 0).then_some(s.p.line as u32)
}

// ── Meanings ────────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
struct Meaning {
    label: String,
    source: &'static str,
    confidence: f64,
    path: String,
    line: Option<u32>,
    detail: String,
}

/// `A=asset,L=liability, Q=Equity` → `{A: asset, L: liability, Q: Equity}`. Only a comment with at
/// least two `code=label` (or `code: label`) pairs counts; one colon in prose is not a legend.
pub fn comment_legend(text: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for part in text.split([',', ';', '\n']) {
        let Some((code, label)) = part.split_once('=').or_else(|| part.split_once(':')) else {
            continue;
        };
        let code = code.trim().trim_matches(['\'', '"']);
        let label = label.trim().trim_matches(['\'', '"', '.']).trim();
        let code_ok = !code.is_empty()
            && code.len() <= 12
            && code
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
        let label_ok =
            !label.is_empty() && label.len() <= 80 && label.chars().any(char::is_alphabetic);
        if code_ok && label_ok {
            out.insert(code.to_string(), label.to_string());
        }
    }
    if out.len() < 2 {
        out.clear();
    }
    out
}

/// The label column of a lookup seed row: a conventional name if present, else the first
/// string-valued column other than the key.
fn seed_label(values: &serde_json::Map<String, Value>, key_col: &str) -> Option<String> {
    let is_text = |v: &Value| v.as_str().is_some_and(|s| s.starts_with('\''));
    for pref in ["label", "name", "class", "description", "title", "type"] {
        if pref != key_col
            && let Some(v) = values.get(pref)
            && is_text(v)
        {
            return v.as_str().map(unquote);
        }
    }
    values
        .iter()
        .filter(|(k, _)| k.as_str() != key_col)
        .find(|(_, v)| is_text(v))
        .and_then(|(_, v)| v.as_str().map(unquote))
}

/// A coded value used in logic that no source explains.
struct Unexplained<'a> {
    table: usize,
    column: String,
    value: String,
    used: Vec<&'a Site>,
    /// The lookup table that seeds the value without any label column.
    declared_in: Option<String>,
}

/// A `SemanticGap` before it is built.
struct GapDraft<'a> {
    key: String,
    name: String,
    props: Vec<(&'static str, Value)>,
    subject: KirId,
    used: Vec<&'a Site>,
}

/// A seeded key as a code: `'1'` seeded into an integer key is the code `1`.
fn seed_code(key: &str) -> String {
    let bare = unquote(key);
    if key.starts_with('\'') && !bare.is_empty() && bare.chars().all(|c| c.is_ascii_digit()) {
        bare
    } else {
        key.to_string()
    }
}

/// FK `from_table.col → to_table.col` pairs from `ForeignKey` relationships' `fk_desc`.
fn foreign_keys(
    graph: &KirGraph,
    by_id: &HashMap<Uuid, usize>,
) -> BTreeMap<(usize, String), (usize, String)> {
    let mut out = BTreeMap::new();
    for r in &graph.relationships {
        if r.kind != RelationshipKind::ForeignKey {
            continue;
        }
        let (Some(&from), Some(&to)) = (by_id.get(&r.from.0), by_id.get(&r.to.0)) else {
            continue;
        };
        let Some(desc) = r.properties.get("fk_desc").and_then(Value::as_str) else {
            continue;
        };
        let Some((l, rr)) = desc.split_once('→') else {
            continue;
        };
        let col = |s: &str| s.trim().rsplit('.').next().unwrap_or("").to_lowercase();
        let (fc, tc) = (col(l), col(rr));
        // Composite keys are written `a, b`; only a single-column key maps a code.
        if fc.is_empty() || tc.is_empty() || fc.contains(',') || l.contains(',') {
            continue;
        }
        out.insert((from, fc), (to, tc));
    }
    out
}

// ── Names ───────────────────────────────────────────────────────────────────────────────────

/// A readable, deterministic class name for a single-predicate concept, using code labels where
/// known: `entity_class IN (2)` with 2 = Customer → `EntityCreditAccountCustomer`.
fn derived_name(
    table: &str,
    column: &str,
    op: &str,
    values: &[String],
    labels: &BTreeMap<String, String>,
) -> String {
    let t = camel(table);
    let c = camel(column);
    let vals = |vs: &[String]| {
        vs.iter()
            .map(|v| labels.get(v).map(|l| camel(l)).unwrap_or_else(|| camel(v)))
            .collect::<Vec<_>>()
            .join("Or")
    };
    let rest = match op {
        "is_true" => c,
        "is_false" => format!("Not{c}"),
        "is_null" => format!("Without{c}"),
        "is_not_null" => format!("With{c}"),
        "in" if values.iter().all(|v| labels.contains_key(v)) => vals(values),
        "in" => format!("{c}{}", vals(values)),
        "not_in" => format!("{c}Not{}", vals(values)),
        ">" | ">=" if values == ["0"] => format!("Positive{c}"),
        "<" | "<=" if values == ["0"] => format!("Negative{c}"),
        ">" => format!("{c}Above{}", vals(values)),
        ">=" => format!("{c}AtLeast{}", vals(values)),
        "<" => format!("{c}Below{}", vals(values)),
        "<=" => format!("{c}AtMost{}", vals(values)),
        "like" => format!("{c}Like{}", vals(values)),
        "not_like" => format!("{c}NotLike{}", vals(values)),
        other => format!("{c}{}{}", camel(other), vals(values)),
    };
    format!("{t}{rest}")
}

// ── Synthesis ───────────────────────────────────────────────────────────────────────────────

/// Synthesize every RFC 0170 hypothesis from `graph` (the committed ledger's objects and
/// relationships).
pub fn synthesize(
    graph: &KirGraph,
    cfg: &SemanticsConfig,
    rationale: Option<&dyn RationaleSource>,
) -> SemanticsOutput {
    let mut stats = SemanticsStats::default();

    // Tables, indexed by name. Views are not tables: a coded column is a stored column.
    let tables: Vec<TableInfo> = graph
        .objects
        .iter()
        .filter(|o| o.kind == ObjectKind::Table)
        .map(table_info)
        .collect();
    let mut index = NameIndex::default();
    let mut by_id: HashMap<Uuid, usize> = HashMap::new();
    for (i, t) in tables.iter().enumerate() {
        index.add(&t.obj.name, t.obj.id);
        by_id.insert(t.obj.id.0, i);
    }
    let sites = collect_sites(graph, &tables, &index, &by_id, &mut stats);
    let fks = foreign_keys(graph, &by_id);

    let mut b = Builder {
        out: SemanticsOutput::default(),
    };

    // ── Coded values per column ───────────────────────────────────────────────────────────
    // (table, column) → value → usage sites.
    let mut coded: BTreeMap<(usize, String), BTreeMap<String, Vec<&Site>>> = BTreeMap::new();
    for s in &sites {
        if !matches!(s.p.op.as_str(), "in" | "not_in") {
            continue;
        }
        let ty = tables[s.table]
            .columns
            .get(&s.column)
            .map(|c| c.data_type.clone())
            .unwrap_or_default();
        if is_quantity_type(&ty) || ty.contains("bool") {
            continue;
        }
        for v in &s.p.values {
            if matches!(v.as_str(), "true" | "false" | "null") {
                continue;
            }
            coded
                .entry((s.table, s.column.clone()))
                .or_default()
                .entry(v.clone())
                .or_default()
                .push(s);
        }
    }
    // `CHECK (kind IN ('A', 'L'))` declares a column's whole domain.
    let mut check_domain: BTreeMap<(usize, String), BTreeSet<String>> = BTreeMap::new();
    for (ti, t) in tables.iter().enumerate() {
        let Some(Value::Array(checks)) = t.obj.properties.get("check_constraints") else {
            continue;
        };
        for c in checks {
            let Some(Value::Array(preds)) = c.get("predicates") else {
                continue;
            };
            for p in preds
                .iter()
                .filter_map(|p| serde_json::from_value::<PredicateSite>(p.clone()).ok())
            {
                if p.op == "in" && p.top_level {
                    check_domain
                        .entry((ti, p.column.clone()))
                        .or_default()
                        .extend(p.values.iter().filter(|v| v.as_str() != "null").cloned());
                }
            }
        }
    }

    // Row keys: a column compared against many distinct literals, or a primary-key/unique column.
    let is_key = |t: usize, c: &str| tables[t].columns.get(c).is_some_and(|c| c.key);
    let key_like: BTreeSet<(usize, String)> = coded
        .iter()
        .filter(|((t, c), vals)| vals.len() > cfg.max_enum_values || is_key(*t, c))
        .map(|(k, _)| k.clone())
        .collect();
    stats.key_like_columns = key_like.len();

    // Every coded column: compared in code, CHECK-constrained, or referencing a seeded lookup.
    let mut columns: BTreeSet<(usize, String)> = coded.keys().cloned().collect();
    columns.extend(check_domain.keys().cloned());
    for ((ti, col), (lt, _)) in &fks {
        if tables[*lt].obj.properties.contains_key("seed_rows") {
            columns.insert((*ti, col.clone()));
        }
    }
    columns.retain(|k| !key_like.contains(k));

    // ── EnumMeaning per (table, column, value) ─────────────────────────────────────────────
    // Labels per (table, column), for naming concepts.
    let mut labels: BTreeMap<(usize, String), BTreeMap<String, String>> = BTreeMap::new();
    // (table, column, value, usage sites, lookup table that seeds it without a label)
    let mut unexplained: Vec<Unexplained> = Vec::new();
    for (ti, col) in &columns {
        let t = &tables[*ti];
        let mut meanings: BTreeMap<String, Vec<Meaning>> = BTreeMap::new();
        // Values a lookup table seeds without any label column: declared, not explained.
        let mut declared: BTreeMap<String, String> = BTreeMap::new();

        // Column comment legend.
        if let Some(c) = t.columns.get(col)
            && let Some(text) = &c.description
        {
            for (code, label) in comment_legend(text) {
                let value = match_value(
                    &code,
                    coded.get(&(*ti, col.clone())),
                    check_domain.get(&(*ti, col.clone())),
                );
                meanings.entry(value).or_default().push(Meaning {
                    label,
                    source: "column_comment",
                    confidence: 0.9,
                    path: c.description_path.clone().unwrap_or_default(),
                    line: c.description_line,
                    detail: format!("COMMENT ON COLUMN {}.{col} IS {text}", t.name()),
                });
            }
        }
        // Lookup seed through a foreign key.
        if let Some((lt, key_col)) = fks.get(&(*ti, col.clone()))
            && let Some(Value::Array(rows)) = tables[*lt].obj.properties.get("seed_rows")
        {
            for row in rows {
                let Some(values) = row.get("values").and_then(Value::as_object) else {
                    continue;
                };
                let Some(key) = values.get(key_col).and_then(Value::as_str) else {
                    continue;
                };
                let label = seed_label(values, key_col);
                let code = seed_code(key);
                let Some(label) = label else {
                    declared.insert(code, tables[*lt].name());
                    continue;
                };
                meanings.entry(code).or_default().push(Meaning {
                    label,
                    source: "lookup_seed",
                    confidence: 0.8,
                    path: row
                        .get("path")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    line: row.get("line").and_then(Value::as_u64).map(|l| l as u32),
                    detail: format!(
                        "{}.{col} → {}.{key_col}; seeded row {}",
                        t.name(),
                        tables[*lt].name(),
                        Value::Object(values.clone())
                    ),
                });
            }
        }
        // The lookup table's own key: `oe_class.id = 2` means what `oe_class`'s seeded row 2 says.
        if let Some(Value::Array(rows)) = t.obj.properties.get("seed_rows")
            && !fks.contains_key(&(*ti, col.clone()))
        {
            for row in rows {
                let Some(values) = row.get("values").and_then(Value::as_object) else {
                    continue;
                };
                let (Some(key), Some(label)) = (
                    values.get(col).and_then(Value::as_str),
                    seed_label(values, col),
                ) else {
                    continue;
                };
                meanings.entry(seed_code(key)).or_default().push(Meaning {
                    label,
                    source: "lookup_seed",
                    confidence: 0.8,
                    path: row
                        .get("path")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    line: row.get("line").and_then(Value::as_u64).map(|l| l as u32),
                    detail: format!("{}: seeded row {}", t.name(), Value::Object(values.clone())),
                });
            }
        }
        // CASE branch labels.
        if let Some(vals) = coded.get(&(*ti, col.clone())) {
            for (v, used) in vals {
                let mut seen = BTreeSet::new();
                for s in used {
                    if s.p.clause == Clause::Case
                        && s.p.values.len() == 1
                        && let Some(label) = &s.p.label
                        && seen.insert(label.clone())
                    {
                        meanings.entry(v.clone()).or_default().push(Meaning {
                            label: unquote(label),
                            source: "case_label",
                            confidence: 0.5,
                            path: s.path.clone(),
                            line: site_line(s),
                            detail: format!(
                                "CASE WHEN {} THEN {label} ({})",
                                s.canonical(&tables),
                                s.carrier_name
                            ),
                        });
                    }
                }
            }
        }

        let mut values: BTreeSet<String> = meanings.keys().cloned().collect();
        if let Some(v) = coded.get(&(*ti, col.clone())) {
            values.extend(v.keys().cloned());
        }
        if let Some(v) = check_domain.get(&(*ti, col.clone())) {
            values.extend(v.iter().cloned());
        }
        if values.is_empty() {
            continue;
        }
        stats.coded_columns += 1;
        let col_labels = labels.entry((*ti, col.clone())).or_default();
        for value in values {
            let used: Vec<&Site> = coded
                .get(&(*ti, col.clone()))
                .and_then(|m| m.get(&value))
                .cloned()
                .unwrap_or_default();
            let mut ms = meanings.remove(&value).unwrap_or_default();
            ms.sort_by(|a, b| {
                b.confidence
                    .total_cmp(&a.confidence)
                    .then(a.label.cmp(&b.label))
            });
            ms.dedup_by(|a, b| a.label == b.label && a.source == b.source);
            let key = format!("enum-meaning:{}:{col}:{value}", t.obj.id);
            let name = format!("{}.{col} = {value}", t.name());
            let best = ms.first().cloned();
            if let Some(m) = &best {
                col_labels.insert(value.clone(), m.label.clone());
                stats.enum_values_explained += 1;
            }
            stats.enum_values += 1;
            let in_check = check_domain
                .get(&(*ti, col.clone()))
                .is_some_and(|d| d.contains(&value));
            let idx = b.object(
                &key,
                name.clone(),
                ENUM_MEANING,
                vec![
                    ("table", json!(t.name())),
                    ("column", json!(col)),
                    ("value", json!(value)),
                    (
                        "label",
                        best.as_ref().map(|m| json!(m.label)).unwrap_or(Value::Null),
                    ),
                    (
                        "confidence",
                        json!(best.as_ref().map_or(0.0, |m| m.confidence)),
                    ),
                    (
                        "meanings",
                        json!(ms.iter().map(|m| json!({
                            "label": m.label, "source": m.source, "confidence": m.confidence,
                            "path": m.path, "line": m.line,
                        })).collect::<Vec<_>>()),
                    ),
                    ("usage_sites", json!(used.len())),
                    ("in_check_constraint", json!(in_check)),
                ],
            );
            let mut evs = Vec::new();
            for (n, m) in ms.iter().enumerate().take(MAX_EVIDENCE) {
                evs.push(b.evidence(&key, n, &m.path, m.line, m.detail.clone()));
            }
            for (n, s) in used
                .iter()
                .enumerate()
                .take(MAX_EVIDENCE.saturating_sub(evs.len()))
            {
                evs.push(b.evidence(
                    &key,
                    100 + n,
                    &s.path,
                    site_line(s),
                    format!(
                        "{} ({}, {:?})",
                        s.canonical(&tables),
                        s.carrier_name,
                        s.p.clause
                    ),
                ));
            }
            let id = b.out.objects[idx].id;
            b.out.objects[idx].evidence = evs;
            b.relate(DESCRIBES, id, t.obj.id);
            if best.is_none() && !used.is_empty() {
                let lookup = declared.get(&value).cloned();
                unexplained.push(Unexplained {
                    table: *ti,
                    column: col.clone(),
                    value,
                    used,
                    declared_in: lookup,
                });
            }
        }
    }

    // ── BusinessConcept ─────────────────────────────────────────────────────────────────────
    let filter_clause = |c: Clause| matches!(c, Clause::Where | Clause::Having | Clause::JoinOn);
    // definition text → (key, origin, sites, view object)
    struct Concept<'a> {
        origin: &'static str,
        view: Option<&'a KirObject>,
        atoms: BTreeSet<String>,
        sites: Vec<&'a Site>,
        name: String,
        table: usize,
    }
    let mut concepts: BTreeMap<String, Concept> = BTreeMap::new();

    // Views: the outermost WHERE's resolved top-level conjuncts define "rows of the view".
    let views: BTreeMap<Uuid, &KirObject> = graph
        .objects
        .iter()
        .filter(|o| is_custom(o, "View"))
        .map(|o| (o.id.0, o))
        .collect();
    let mut view_sites: BTreeMap<Uuid, Vec<&Site>> = BTreeMap::new();
    for s in &sites {
        if s.carrier_is_view && s.p.clause == Clause::Where && s.p.top_level && !s.p.subquery {
            view_sites.entry(s.carrier.0).or_default().push(s);
        }
    }
    for (vid, vs) in &view_sites {
        let atoms: BTreeSet<String> = vs.iter().map(|s| s.canonical(&tables)).collect();
        let def = atoms.iter().cloned().collect::<Vec<_>>().join(" AND ");
        let view = views[vid];
        concepts.entry(def).or_insert(Concept {
            origin: "view",
            view: Some(view),
            atoms,
            sites: vs.clone(),
            name: camel(&tail(&view.name)),
            table: vs[0].table,
        });
    }

    // Recurring single predicates.
    let mut by_canon: BTreeMap<String, Vec<&Site>> = BTreeMap::new();
    for s in &sites {
        if filter_clause(s.p.clause)
            && !key_like.contains(&(s.table, s.column.clone()))
            && !is_key(s.table, &s.column)
        {
            by_canon.entry(s.canonical(&tables)).or_default().push(s);
        }
    }
    for (canon, ss) in by_canon {
        let carriers: BTreeSet<Uuid> = ss.iter().map(|s| s.carrier.0).collect();
        if let Some(existing) = concepts.get_mut(&canon) {
            // A view defined by exactly this predicate: the same concept, seen more widely.
            for s in ss {
                if !existing
                    .sites
                    .iter()
                    .any(|e| e.carrier == s.carrier && e.p.line == s.p.line)
                {
                    existing.sites.push(s);
                }
            }
            continue;
        }
        if carriers.len() < cfg.min_sites {
            continue;
        }
        let s0 = ss[0];
        let name = derived_name(
            &tables[s0.table].name(),
            &s0.column,
            &s0.p.op,
            &s0.p.values,
            labels
                .get(&(s0.table, s0.column.clone()))
                .unwrap_or(&BTreeMap::new()),
        );
        concepts.insert(
            canon.clone(),
            Concept {
                origin: "recurring",
                view: None,
                atoms: BTreeSet::from([canon]),
                table: s0.table,
                sites: ss,
                name,
            },
        );
    }

    let mut undocumented: Vec<(KirId, String, String, Vec<&Site>)> = Vec::new();
    let mut concept_ids: Vec<(KirId, String, Vec<&Site>)> = Vec::new();
    for (def, c) in &concepts {
        let key = format!("business-concept:{def}");
        let carriers: BTreeSet<Uuid> = c.sites.iter().map(|s| s.carrier.0).collect();
        let description = c
            .view
            .and_then(|v| v.properties.get("description"))
            .and_then(Value::as_str)
            .map(str::to_string);
        let confidence = match c.origin {
            "view" if description.is_some() => 0.6,
            "view" => 0.5,
            _ => (0.3 + 0.05 * carriers.len() as f64).min(0.6),
        };
        let tables_named: BTreeSet<String> =
            c.sites.iter().map(|s| tables[s.table].name()).collect();
        let mut seen_in: Vec<String> = c
            .sites
            .iter()
            .map(|s| s.carrier_name.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        seen_in.truncate(50);
        let idx = b.object(
            &key,
            c.name.clone(),
            CONCEPT,
            vec![
                ("definition", json!(def)),
                ("predicates", json!(c.atoms)),
                ("table", json!(tables[c.table].name())),
                ("tables", json!(tables_named)),
                ("origin", json!(c.origin)),
                (
                    "name_source",
                    json!(if c.origin == "view" {
                        "view"
                    } else {
                        "derived"
                    }),
                ),
                ("view", json!(c.view.map(|v| v.name.clone()))),
                ("description", json!(description)),
                ("sites", json!(c.sites.len())),
                ("carriers", json!(carriers.len())),
                ("seen_in", json!(seen_in)),
                ("confidence", json!(confidence)),
            ],
        );
        let id = b.out.objects[idx].id;
        let mut evs = Vec::new();
        for (n, s) in c.sites.iter().enumerate().take(MAX_EVIDENCE) {
            evs.push(b.evidence(
                &key,
                n,
                &s.path,
                site_line(s),
                format!(
                    "{} ({}, {:?})",
                    s.canonical(&tables),
                    s.carrier_name,
                    s.p.clause
                ),
            ));
        }
        b.out.objects[idx].evidence = evs;
        for t in c.sites.iter().map(|s| s.table).collect::<BTreeSet<_>>() {
            b.relate(DESCRIBES, id, tables[t].obj.id);
        }
        for carrier in &carriers {
            b.relate(EVIDENCED_BY, id, KirId(*carrier));
        }
        stats.concepts += 1;
        if c.origin == "view" {
            stats.concepts_from_views += 1;
        }
        if description.is_none() {
            undocumented.push((id, key.clone(), c.name.clone(), c.sites.clone()));
        }
        concept_ids.push((id, key, c.sites.clone()));
    }

    // ── ConstraintCandidate per CHECK ───────────────────────────────────────────────────────
    for t in &tables {
        let Some(Value::Array(checks)) = t.obj.properties.get("check_constraints") else {
            continue;
        };
        for (n, c) in checks.iter().enumerate() {
            let expression = c
                .get("expression")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let preds: Vec<PredicateSite> = c
                .get("predicates")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|p| serde_json::from_value(p.clone()).ok())
                        .collect()
                })
                .unwrap_or_default();
            let top: Vec<&PredicateSite> = preds.iter().filter(|p| p.top_level).collect();
            let constraint_type = if top.is_empty() {
                "other"
            } else if top
                .iter()
                .all(|p| matches!(p.op.as_str(), "<" | "<=" | ">" | ">=" | "between"))
            {
                "range"
            } else if top.len() == 1 && top[0].op == "in" {
                "enum"
            } else if top.iter().all(|p| p.op == "like") {
                "pattern"
            } else if top.iter().all(|p| p.op == "is_not_null") {
                "not_null"
            } else {
                "other"
            };
            let key = format!("constraint-candidate:{}:{n}:{expression}", t.obj.id);
            let path = c.get("path").and_then(Value::as_str).unwrap_or_default();
            let line = c.get("line").and_then(Value::as_u64).map(|l| l as u32);
            let idx = b.object(
                &key,
                format!("{} CHECK ({expression})", t.name()),
                CONSTRAINT,
                vec![
                    ("table", json!(t.name())),
                    (
                        "constraint_name",
                        c.get("name").cloned().unwrap_or(Value::Null),
                    ),
                    ("expression", json!(expression)),
                    ("constraint_type", json!(constraint_type)),
                    (
                        "columns",
                        json!(
                            top.iter()
                                .map(|p| p.column.clone())
                                .collect::<BTreeSet<_>>()
                        ),
                    ),
                    (
                        "predicates",
                        json!(top.iter().map(|p| p.canonical()).collect::<Vec<_>>()),
                    ),
                    ("structured", json!(preds)),
                    ("confidence", json!(0.95)),
                ],
            );
            let ev = b.evidence(
                &key,
                0,
                path,
                line,
                format!("CHECK ({expression}) on {}", t.name()),
            );
            b.out.objects[idx].evidence.push(ev);
            let id = b.out.objects[idx].id;
            b.relate(DESCRIBES, id, t.obj.id);
            stats.constraints += 1;
        }
    }

    // ── Rationale (git blame) ───────────────────────────────────────────────────────────────
    let mut explained: BTreeSet<Uuid> = BTreeSet::new();
    for (id, key, ss) in &concept_ids {
        blame_into(&mut b, rationale, &mut explained, *id, key, ss);
    }

    // ── SemanticGap ─────────────────────────────────────────────────────────────────────────
    let mut gaps: Vec<GapDraft> = Vec::new();
    for Unexplained {
        table: ti,
        column: col,
        value,
        used,
        declared_in: lookup,
    } in unexplained
    {
        let t = &tables[ti];
        let carriers: BTreeSet<String> = used.iter().map(|s| s.carrier_name.clone()).collect();
        let declared = lookup
            .as_ref()
            .map(|l| format!(" `{l}` seeds it, but with no label or description column."))
            .unwrap_or_default();
        gaps.push(GapDraft {
            key: format!("semantic-gap:value:{}:{col}:{value}", t.obj.id),
            name: format!("{}.{col} = {value}: meaning unknown", t.name()),
            props: vec![
                ("gap_type", json!("unexplained_value")),
                ("table", json!(t.name())),
                ("column", json!(col)),
                ("value", json!(value)),
                ("usage_sites", json!(used.len())),
                ("seen_in", json!(carriers)),
                ("declared_in", json!(lookup)),
                (
                    "question",
                    json!(format!(
                        "What does {}.{col} = {value} mean? It is used in logic ({} site(s)) but no \
                         comment, lookup row or CASE label explains it.{declared}",
                        t.name(),
                        used.len()
                    )),
                ),
            ],
            subject: t.obj.id,
            used,
        });
    }
    for (id, _key, name, ss) in undocumented {
        if explained.contains(&id.0) {
            continue;
        }
        gaps.push(GapDraft {
            key: format!("semantic-gap:concept:{id}"),
            name: format!("{name}: no documented rationale"),
            props: vec![
                ("gap_type", json!("undocumented_concept")),
                ("concept", json!(name)),
                (
                    "question",
                    json!(format!(
                        "Why does the code filter this way ({name})? No comment or commit explains it."
                    )),
                ),
            ],
            subject: id,
            used: ss,
        });
    }
    for GapDraft {
        key,
        name,
        props,
        subject,
        used,
    } in gaps
    {
        let idx = b.object(&key, name, GAP, props);
        let mut evs = Vec::new();
        for (n, s) in used.iter().enumerate().take(MAX_EVIDENCE) {
            evs.push(b.evidence(
                &key,
                n,
                &s.path,
                site_line(s),
                format!(
                    "{} ({}, {:?})",
                    s.canonical(&tables),
                    s.carrier_name,
                    s.p.clause
                ),
            ));
        }
        b.out.objects[idx].evidence = evs;
        let id = b.out.objects[idx].id;
        b.relate(DESCRIBES, id, subject);
        // Who introduced an unexplained value is who to ask.
        if b.out.objects[idx].properties.get("gap_type") == Some(&json!("unexplained_value")) {
            blame_into(&mut b, rationale, &mut explained, id, &key, &used);
        }
        stats.gaps += 1;
    }

    stats.rationale_links = b
        .out
        .objects
        .iter()
        .filter(|o| kind_name(o) == Some(RATIONALE))
        .count();
    b.out.stats = stats;
    b.out
}

/// `git blame` an item's evidence lines (at most [`MAX_BLAME_LINES`]) and link each distinct commit
/// as a `RationaleLink`.
fn blame_into(
    b: &mut Builder,
    rationale: Option<&dyn RationaleSource>,
    explained: &mut BTreeSet<Uuid>,
    owner: KirId,
    owner_key: &str,
    sites: &[&Site],
) {
    let Some(src) = rationale else { return };
    let mut by_path: BTreeMap<&str, BTreeSet<u32>> = BTreeMap::new();
    for s in sites.iter().filter(|s| !s.path.is_empty()) {
        if let Some(l) = site_line(s) {
            by_path.entry(&s.path).or_default().insert(l);
        }
    }
    let mut commits: BTreeMap<String, (BlameHit, String, BTreeSet<u32>)> = BTreeMap::new();
    let mut budget = MAX_BLAME_LINES;
    for (path, lines) in by_path {
        let lines: Vec<u32> = lines.into_iter().take(budget).collect();
        if lines.is_empty() {
            break;
        }
        budget -= lines.len();
        for hit in src.blame(path, &lines) {
            commits
                .entry(hit.sha.clone())
                .or_insert_with(|| (hit.clone(), path.to_string(), BTreeSet::new()))
                .2
                .insert(hit.line);
        }
    }
    for (sha, (hit, path, lines)) in commits {
        let key = format!("rationale:{owner_key}:{sha}");
        let short = &sha[..sha.len().min(10)];
        let idx = b.object(
            &key,
            format!("{short} {}", hit.summary),
            RATIONALE,
            vec![
                ("sha", json!(sha)),
                ("summary", json!(hit.summary)),
                ("author", json!(hit.author)),
                ("date", json!(hit.date)),
                ("path", json!(path)),
                ("lines", json!(lines)),
                ("confidence", json!(0.4)),
            ],
        );
        let first = lines.iter().next().copied();
        let ev = b.evidence(
            &key,
            0,
            &path,
            first,
            format!("git blame: {short} {}", hit.summary),
        );
        b.out.objects[idx].evidence.push(ev);
        let id = b.out.objects[idx].id;
        b.relate(EXPLAINED_BY, owner, id);
        explained.insert(owner.0);
    }
}

/// The value a comment code names: `A` → `'A'` when the code compares a string, `1` stays `1`.
fn match_value(
    code: &str,
    used: Option<&BTreeMap<String, Vec<&Site>>>,
    check: Option<&BTreeSet<String>>,
) -> String {
    let quoted = format!("'{}'", code.replace('\'', "''"));
    let known =
        |v: &str| used.is_some_and(|m| m.contains_key(v)) || check.is_some_and(|c| c.contains(v));
    if known(code) {
        code.to_string()
    } else if known(&quoted) || !code.chars().all(|c| c.is_ascii_digit() || c == '-') {
        quoted
    } else {
        code.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(name: &str, cols: Value, extra: Vec<(&str, Value)>) -> KirObject {
        let mut o = KirObject::new(name, ObjectKind::Table);
        o.id = kid(&format!("t:{name}"));
        o.properties.insert("columns".into(), cols);
        for (k, v) in extra {
            o.properties.insert(k.into(), v);
        }
        o
    }

    fn carrier(kind: &str, name: &str, preds: Value) -> KirObject {
        let mut o = KirObject::new(name, ObjectKind::Custom(kind.into()));
        o.id = kid(&format!("c:{name}"));
        o.properties.insert("predicates".into(), preds);
        o.properties
            .insert("source_path".into(), json!("sql/x.sql"));
        if kind == "View" {
            o.properties
                .insert("description".into(), json!("Parts still sold"));
        }
        o
    }

    fn site(rel: &str, col: &str, op: &str, values: &[&str], clause: &str, line: u64) -> Value {
        json!({"relation": rel, "column": col, "op": op, "values": values,
               "clause": clause, "top_level": true, "line": line})
    }

    fn fixture() -> KirGraph {
        let mut g = KirGraph::new();
        let parts = table(
            "parts",
            json!([{"name": "id", "data_type": "INT", "primary_key": true},
                   {"name": "obsolete", "data_type": "BOOLEAN"},
                   {"name": "price", "data_type": "NUMERIC"}]),
            vec![(
                "check_constraints",
                json!([{"name": null, "path": "sql/schema.sql", "expression": "price >= 0", "line": 9,
                        "predicates": [site("parts", "price", ">=", &["0"], "check", 9)]}]),
            )],
        );
        let account = table(
            "account",
            json!([{"name": "category", "data_type": "CHAR(1)",
                    "description": "A=asset,L=liability,Q=Equity", "description_path": "sql/schema.sql",
                    "description_line": 3}]),
            vec![],
        );
        let eca = table(
            "entity_credit_account",
            json!([{"name": "entity_class", "data_type": "INT"}]),
            vec![],
        );
        let ec = table(
            "entity_class",
            json!([{"name": "id", "data_type": "INT"}, {"name": "class", "data_type": "TEXT"}]),
            vec![(
                "seed_rows",
                json!([{"path": "sql/schema.sql", "line": 20, "values": {"id": "1", "class": "'Vendor'"}},
                       {"path": "sql/schema.sql", "line": 20, "values": {"id": "2", "class": "'Customer'"}}]),
            )],
        );
        let fk = {
            let mut r = KirRelationship::new(RelationshipKind::ForeignKey, eca.id, ec.id);
            r.properties.insert(
                "fk_desc".into(),
                json!("entity_credit_account.entity_class → entity_class.id"),
            );
            r
        };
        for t in [parts, account, eca, ec] {
            g.add_object(t);
        }
        g.add_relationship(fk);
        g.add_object(carrier(
            "View",
            "active_parts",
            json!([site("parts", "obsolete", "is_false", &[], "where", 5)]),
        ));
        g.add_object(carrier(
            "ProcedureStatement",
            "parts__list#2",
            json!([site("parts", "obsolete", "is_false", &[], "where", 40)]),
        ));
        g.add_object(carrier(
            "ProcedureStatement",
            "customer__list#1",
            json!([
                site(
                    "entity_credit_account",
                    "entity_class",
                    "in",
                    &["2"],
                    "where",
                    50
                ),
                site("account", "category", "in", &["'A'", "'Z'"], "where", 51)
            ]),
        ));
        g.add_object(carrier(
            "ProcedureStatement",
            "customer__search#4",
            json!([site("entity_credit_account", "entity_class", "in", &["2"], "join_on", 70),
                   {"scope": ["account", "parts"], "column": "category", "op": "in",
                    "values": ["'A'", "'Z'"], "clause": "where", "top_level": true, "line": 71}]),
        ));
        g
    }

    fn of<'a>(out: &'a SemanticsOutput, kind: &str) -> Vec<&'a KirObject> {
        out.objects
            .iter()
            .filter(|o| kind_name(o) == Some(kind))
            .collect()
    }

    fn prop<'a>(o: &'a KirObject, k: &str) -> &'a Value {
        o.properties.get(k).unwrap_or(&Value::Null)
    }

    #[test]
    fn view_filters_and_recurring_filters_become_concepts_named_by_meaning() {
        let out = synthesize(&fixture(), &SemanticsConfig::default(), None);
        let mut names: Vec<(String, String)> = of(&out, CONCEPT)
            .iter()
            .map(|c| {
                (
                    c.name.clone(),
                    prop(c, "definition").as_str().unwrap().to_string(),
                )
            })
            .collect();
        names.sort();
        assert_eq!(
            names,
            vec![
                (
                    "AccountCategoryAssetOrZ".into(),
                    "account.category IN ('A', 'Z')".into()
                ),
                ("ActiveParts".into(), "parts.obsolete IS FALSE".into()),
                (
                    "EntityCreditAccountCustomer".into(),
                    "entity_credit_account.entity_class IN (2)".into()
                ),
            ]
        );
        let ap = of(&out, CONCEPT)
            .into_iter()
            .find(|c| c.name == "ActiveParts")
            .unwrap();
        // The view's filter also recurs in a routine: one concept, both sites.
        assert_eq!(prop(ap, "sites"), &json!(2));
        assert_eq!(prop(ap, "origin"), &json!("view"));
        assert_eq!(prop(ap, "description"), &json!("Parts still sold"));
        assert!(
            of(&out, CONCEPT)
                .iter()
                .all(|c| prop(c, "status") == &json!(HYPOTHESIS))
        );
        assert_eq!(out.stats.concepts_from_views, 1);
    }

    #[test]
    fn code_meanings_come_from_comments_and_lookup_seeds_and_gaps_from_neither() {
        let out = synthesize(&fixture(), &SemanticsConfig::default(), None);
        let label = |name: &str| {
            of(&out, ENUM_MEANING)
                .into_iter()
                .find(|o| o.name == name)
                .map(|o| prop(o, "label").clone())
        };
        assert_eq!(label("account.category = 'A'"), Some(json!("asset")));
        assert_eq!(label("account.category = 'Q'"), Some(json!("Equity")));
        assert_eq!(label("account.category = 'Z'"), Some(Value::Null));
        // Every seeded value, compared in code or not.
        assert_eq!(
            label("entity_credit_account.entity_class = 2"),
            Some(json!("Customer"))
        );
        assert_eq!(
            label("entity_credit_account.entity_class = 1"),
            Some(json!("Vendor"))
        );

        let gaps: Vec<String> = of(&out, GAP).iter().map(|g| g.name.clone()).collect();
        assert!(
            gaps.contains(&"account.category = 'Z': meaning unknown".to_string()),
            "{gaps:?}"
        );
        // The unqualified `category` in scope {account, parts} resolved to account (parts has no
        // such column), so both statements count.
        let z = of(&out, GAP)
            .into_iter()
            .find(|g| g.name.starts_with("account.category = 'Z'"))
            .unwrap();
        assert_eq!(prop(z, "usage_sites"), &json!(2));
        assert_eq!(z.evidence.len(), 2);
    }

    #[test]
    fn a_lookup_tables_own_key_means_its_seeded_row() {
        let mut g = fixture();
        g.add_object(carrier(
            "ProcedureStatement",
            "entity_class__list#1",
            json!([site("entity_class", "id", "in", &["3"], "where", 80)]),
        ));
        let mut ec = g
            .objects
            .iter()
            .find(|o| o.name == "entity_class")
            .unwrap()
            .clone();
        // Not a key column here, so the comparison is a coded use.
        ec.properties.insert(
            "columns".into(),
            json!([{"name": "id", "data_type": "INT"}, {"name": "class", "data_type": "TEXT"}]),
        );
        let rows = ec.properties["seed_rows"].as_array().unwrap().clone();
        let mut rows = rows;
        rows.push(json!({"path": "sql/schema.sql", "line": 21, "values": {"id": "3", "class": "'Employee'"}}));
        ec.properties.insert("seed_rows".into(), json!(rows));
        g.objects.retain(|o| o.name != "entity_class");
        g.add_object(ec);
        let out = synthesize(&g, &SemanticsConfig::default(), None);
        let e = of(&out, ENUM_MEANING)
            .into_iter()
            .find(|o| o.name == "entity_class.id = 3")
            .unwrap();
        assert_eq!(prop(e, "label"), &json!("Employee"));
    }

    #[test]
    fn check_constraints_become_typed_candidates() {
        let out = synthesize(&fixture(), &SemanticsConfig::default(), None);
        let c = of(&out, CONSTRAINT);
        assert_eq!(c.len(), 1);
        assert_eq!(prop(c[0], "constraint_type"), &json!("range"));
        assert_eq!(prop(c[0], "predicates"), &json!(["parts.price >= 0"]));
    }

    struct Fixed;
    impl RationaleSource for Fixed {
        fn blame(&self, path: &str, lines: &[u32]) -> Vec<BlameHit> {
            lines
                .iter()
                .map(|&line| BlameHit {
                    line,
                    sha: if line < 50 {
                        "aaa111".into()
                    } else {
                        "bbb222".into()
                    },
                    summary: format!("touch {path}"),
                    author: "dev".into(),
                    date: "2020-01-01T00:00:00Z".into(),
                })
                .collect()
        }
    }

    #[test]
    fn rationale_links_concepts_to_commits_and_closes_the_undocumented_gap() {
        let without = synthesize(&fixture(), &SemanticsConfig::default(), None);
        assert!(
            of(&without, GAP)
                .iter()
                .any(|g| prop(g, "gap_type") == &json!("undocumented_concept"))
        );

        let with = synthesize(&fixture(), &SemanticsConfig::default(), Some(&Fixed));
        assert!(with.stats.rationale_links > 0);
        assert!(
            !of(&with, GAP)
                .iter()
                .any(|g| prop(g, "gap_type") == &json!("undocumented_concept"))
        );
        let explained_by = with
            .relationships
            .iter()
            .filter(|r| r.kind == RelationshipKind::Custom(EXPLAINED_BY.into()))
            .count();
        assert_eq!(explained_by, with.stats.rationale_links);
    }

    #[test]
    fn a_column_compared_against_many_values_is_a_key_not_a_classification() {
        let mut g = fixture();
        let preds: Vec<Value> = (0..20)
            .map(|i| {
                site(
                    "account",
                    "category",
                    "in",
                    &[&format!("'k{i}'")],
                    "where",
                    100 + i,
                )
            })
            .collect();
        g.add_object(carrier("ProcedureStatement", "settings#1", json!(preds)));
        let out = synthesize(&g, &SemanticsConfig::default(), None);
        assert_eq!(out.stats.key_like_columns, 1);
        assert!(
            of(&out, ENUM_MEANING)
                .iter()
                .all(|o| prop(o, "column") != &json!("category"))
        );
    }

    #[test]
    fn a_primary_key_compared_to_a_literal_picks_a_row_not_a_class() {
        let mut g = fixture();
        g.add_object(table(
            "defaults",
            json!([{"name": "setting_key", "data_type": "TEXT", "primary_key": true}]),
            vec![],
        ));
        g.add_object(carrier(
            "ProcedureStatement",
            "setting_get#1",
            json!([
                site("defaults", "setting_key", "in", &["'curr'"], "where", 9),
                site("defaults", "setting_key", "in", &["'curr'"], "where", 19)
            ]),
        ));
        let out = synthesize(&g, &SemanticsConfig::default(), None);
        assert!(
            of(&out, ENUM_MEANING)
                .iter()
                .all(|o| prop(o, "table") != &json!("defaults"))
        );
        assert!(
            of(&out, GAP)
                .iter()
                .all(|o| prop(o, "table") != &json!("defaults"))
        );
        assert!(
            of(&out, CONCEPT)
                .iter()
                .all(|o| prop(o, "table") != &json!("defaults"))
        );
    }

    /// The stored kind must survive a serde round trip, or a ledger read loses every concept.
    #[test]
    fn every_kind_round_trips_through_serde() {
        let out = synthesize(&fixture(), &SemanticsConfig::default(), Some(&Fixed));
        for o in &out.objects {
            let back: KirObject = serde_json::from_value(serde_json::to_value(o).unwrap()).unwrap();
            assert_eq!(kind_name(&back), kind_name(o), "{}", o.name);
            assert!(kind_name(o).is_some());
        }
        assert!(
            out.objects
                .iter()
                .any(|o| o.kind == ObjectKind::BusinessConcept)
        );
    }

    #[test]
    fn synthesis_is_deterministic() {
        let a = synthesize(&fixture(), &SemanticsConfig::default(), Some(&Fixed));
        let b = synthesize(&fixture(), &SemanticsConfig::default(), Some(&Fixed));
        let ids = |o: &SemanticsOutput| {
            o.objects
                .iter()
                .map(|x| (x.id, x.properties.clone()))
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(&a), ids(&b));
        let evs = |o: &SemanticsOutput| o.evidence.iter().map(|e| e.id).collect::<Vec<_>>();
        assert_eq!(evs(&a), evs(&b));
    }

    #[test]
    fn a_legend_needs_two_pairs() {
        assert_eq!(comment_legend("Note: this is free text").len(), 0);
        assert_eq!(
            comment_legend(" A=asset,L=liability,Q=Equity,I=Income,E=expense "),
            BTreeMap::from([
                ("A".into(), "asset".into()),
                ("E".into(), "expense".into()),
                ("I".into(), "Income".into()),
                ("L".into(), "liability".into()),
                ("Q".into(), "Equity".into()),
            ])
        );
    }
}
