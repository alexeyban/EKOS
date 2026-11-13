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
use ekos_kir::predicates::{Clause, NEW_ROW, OLD_ROW, PredicateSite, canonical_text};
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
/// RFC 0170 Phase 2: two concepts that disagree about the same thing.
pub const CONFLICT: &str = "ConceptConflict";

/// Every kind this module writes.
pub const KINDS: [&str; 6] = [CONCEPT, ENUM_MEANING, CONSTRAINT, GAP, CONFLICT, RATIONALE];

/// Relationship kinds: item → the table/view it describes; concept → where it was seen;
/// concept/gap → the commit that explains it.
pub const DESCRIBES: &str = "Describes";
pub const EVIDENCED_BY: &str = "EvidencedBy";
pub const EXPLAINED_BY: &str = "ExplainedBy";
pub const CONFLICTS_WITH: &str = "ConflictsWith";

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
    /// RFC 0170 Phase 4: the user's vocabulary for mapping suggestions (empty: none made).
    pub ontology: crate::ontology::Vocabulary,
    /// RFC 0252: what `ekos profile run` measured (`<state>/profile/current.json`); `None`: nothing.
    pub data: Option<crate::data_usage::DataSignals>,
}

impl Default for SemanticsConfig {
    fn default() -> Self {
        Self {
            min_sites: 2,
            max_enum_values: 12,
            ontology: Default::default(),
            data: None,
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
///
/// An implementation is a raw-content entry point (RFC 0043): it never answers for an excluded
/// path and returns `summary` already redacted, because the summary is written to the ledger.
pub trait RationaleSource {
    /// The commits that last touched `lines` of `path` (a ledger evidence path). Lines it cannot
    /// answer for are simply absent.
    fn blame(&self, path: &str, lines: &[u32]) -> Vec<BlameHit>;

    /// RFC 0252 §3.2(b): the commit that introduced `needle` (a code literal as the ledger writes
    /// it, `'A'` or `7`) at `line` of `path` — following that line back through history to the
    /// oldest version that still names it. `None` when it cannot tell (no history, the line does
    /// not name it, an excluded path). The default knows no history.
    fn introduced(&self, _path: &str, _line: u32, _needle: &str) -> Option<BlameHit> {
        None
    }
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
    pub conflicts: usize,
    pub rationale_links: usize,
    pub mapping_suggestions: usize,
    /// RFC 0252: code values with a measured row count, dead codes, code values no row holds.
    pub data_values_measured: usize,
    pub dead_codes: usize,
    pub never_seen_codes: usize,
    pub data_only_gaps: usize,
    /// RFC 0252 §3.2(b): code values the data held before the code named them, and columns whose
    /// values no code names all appeared after the code last changed ("new codes").
    pub data_before_code: usize,
    pub new_value_columns: usize,
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

/// A table's recovered column entries, as stored (`columns` property).
fn raw_columns(o: &KirObject) -> Vec<&Value> {
    o.properties
        .get("columns")
        .and_then(Value::as_array)
        .map(|a| a.iter().collect())
        .unwrap_or_default()
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
    /// The dbt var that produced this site's single value (`'1200'` ← `acc_ar`): a name hint.
    var_label: Option<String>,
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

fn is_dbt_model(o: &KirObject) -> bool {
    o.kind == ObjectKind::Table
        && o.properties.get("dbt_kind").and_then(Value::as_str) == Some("model")
        && o.properties.contains_key("predicates")
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
    // `NEW.`/`OLD.` in a routine's condition: the table of the trigger(s) running the routine,
    // when they all fire on one table.
    let mut trigger_tables: HashMap<String, BTreeSet<String>> = HashMap::new();
    for t in graph.objects.iter().filter(|o| is_custom(o, "Trigger")) {
        let (Some(f), Some(table)) = (
            t.properties.get("function").and_then(Value::as_str),
            t.properties.get("table").and_then(Value::as_str),
        ) else {
            continue;
        };
        trigger_tables
            .entry(tail(f))
            .or_default()
            .insert(table.to_lowercase());
    }
    let mut out = Vec::new();
    for o in &graph.objects {
        // A dbt model is a view whose WHERE defines it (RFC 0170).
        let carrier_is_view = is_custom(o, "View") || is_dbt_model(o);
        // A Pentaho FilterRows step (RFC 0170 Phase 3) is a `TransformNode` with `predicates`.
        if !(carrier_is_view
            || is_custom(o, "ProcedureStatement")
            || is_custom(o, "Procedure")
            || is_custom(o, "TransformNode")
            || is_custom(o, "PerlSymbol")
            || is_custom(o, "PerlPackage"))
        {
            continue;
        }
        let var_values = o
            .properties
            .get("dbt_var_values")
            .and_then(Value::as_object);
        let Some(Value::Array(preds)) = o.properties.get("predicates") else {
            continue;
        };
        for v in preds {
            let Ok(p) = serde_json::from_value::<PredicateSite>(v.clone()) else {
                continue;
            };
            // RFC 0254: join equalities and casts are how code handles a column, not a meaning.
            if p.clause.is_structural() {
                continue;
            }
            stats.sites += 1;
            let table = match &p.relation {
                Some(r) if r == NEW_ROW || r == OLD_ROW => {
                    let routine = o
                        .properties
                        .get("procedure")
                        .and_then(Value::as_str)
                        .unwrap_or(&o.name);
                    match trigger_tables.get(&tail(routine)) {
                        Some(ts) if ts.len() == 1 => resolve_table(ts.iter().next().unwrap()),
                        _ => None,
                    }
                }
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
            let Some(mut table) = table else {
                continue;
            };
            let mut p = p;
            // A dbt model column that passes a source column through is restated on that column,
            // across models (`stg_orders.is_closed` → `oe.closed`).
            for _ in 0..8 {
                let Some(Value::Object(lineage)) =
                    tables[table].obj.properties.get("column_lineage")
                else {
                    break;
                };
                let Some((r, c)) = lineage.get(&p.column).and_then(|v| {
                    Some((
                        v.get(0)?.as_str()?.to_string(),
                        v.get(1)?.as_str()?.to_string(),
                    ))
                }) else {
                    break;
                };
                let Some(next) = resolve_table(&r) else {
                    break;
                };
                table = next;
                p.column = c.to_lowercase();
                p.relation = Some(r);
            }
            // A column the table does not declare is a computed alias or a typo; not a fact about
            // the table. (A table recovered without columns cannot be checked, and is trusted; a
            // dbt model documents only some of its columns, so it is trusted too.)
            let partial = tables[table].obj.properties.contains_key("dbt_kind");
            if !partial
                && !tables[table].columns.is_empty()
                && !tables[table].columns.contains_key(&p.column)
            {
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
                var_label: match (var_values, p.values.as_slice()) {
                    (Some(m), [v]) => m.get(v).and_then(Value::as_str).map(str::to_string),
                    _ => None,
                },
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
        // "A asset, L liability, Q equity": every part a short upper-case code, a space, a word.
        let parts: Vec<&str> = text
            .trim()
            .trim_end_matches('.')
            .split([',', ';'])
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .collect();
        let pairs: Vec<(String, String)> = parts
            .iter()
            .filter_map(|p| {
                let (code, label) = p.split_once(char::is_whitespace)?;
                let code_ok = !code.is_empty()
                    && code.len() <= 4
                    && code
                        .chars()
                        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit());
                let label = label.trim();
                let label_ok = label.len() <= 60 && label.chars().next()?.is_alphabetic();
                (code_ok && label_ok).then(|| (code.to_string(), label.to_string()))
            })
            .collect();
        if pairs.len() >= 2 && pairs.len() == parts.len() {
            out.extend(pairs);
        }
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

/// One concept in a potential conflict: (id, name, definition).
type ConflictMember = (KirId, String, String);

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

/// One glossary entry, from a `Section` or `Page` carrying `glossary`.
struct GlossEntry {
    term: String,
    definition: String,
    path: String,
    line: u32,
    owner: KirId,
    /// `glossary` (a document) or `ossie` (an Apache Ossie model, RFC 0246).
    source: &'static str,
}

fn glossary_entries(graph: &KirGraph) -> Vec<GlossEntry> {
    let mut out = Vec::new();
    for o in &graph.objects {
        // Documents only. The items this module writes carry `glossary` too (what they matched),
        // and reading those back would re-import every previous run's output.
        if !(is_custom(o, "Section") || is_custom(o, "Page")) {
            continue;
        }
        let Some(Value::Array(entries)) = o.properties.get("glossary") else {
            continue;
        };
        let path = o
            .properties
            .get("source_path")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        for e in entries {
            let (Some(term), Some(definition)) = (e["term"].as_str(), e["definition"].as_str())
            else {
                continue;
            };
            out.push(GlossEntry {
                term: term.to_string(),
                definition: definition.to_string(),
                path: path.clone(),
                line: e["line"].as_u64().unwrap_or(0) as u32,
                owner: o.id,
                source: "glossary",
            });
        }
    }
    out.extend(ossie_gloss(graph));
    // Ledger order is arbitrary; evidence numbering and match order must not be.
    out.sort_by(|a, b| {
        (&a.path, a.line, &a.term, &a.definition, a.owner.0).cmp(&(
            &b.path,
            b.line,
            &b.term,
            &b.definition,
            b.owner.0,
        ))
    });
    out.dedup_by(|a, b| a.path == b.path && a.line == b.line && a.term == b.term);
    out
}

/// Words for matching names across spellings: `OpenOrders`, `open_orders`, "Open orders" all
/// become `open order` (camel case and separators split, lower-cased, a plural `s` dropped).
pub fn words_key(s: &str) -> String {
    let mut words: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut prev_lower = false;
    for c in s.chars() {
        if !c.is_alphanumeric() {
            if !cur.is_empty() {
                words.push(std::mem::take(&mut cur));
            }
            prev_lower = false;
            continue;
        }
        if c.is_uppercase() && prev_lower && !cur.is_empty() {
            words.push(std::mem::take(&mut cur));
        }
        prev_lower = c.is_lowercase() || c.is_ascii_digit();
        cur.extend(c.to_lowercase());
    }
    if !cur.is_empty() {
        words.push(cur);
    }
    words
        .into_iter()
        .map(|w| {
            if w.len() > 3 && w.ends_with('s') && !w.ends_with("ss") {
                w[..w.len() - 1].to_string()
            } else {
                w
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// One application constant: `EC_CUSTOMER => 2` in package `LedgerSMB::Magic`.
#[derive(Debug, Clone)]
struct Constant {
    name: String,
    suffix: String,
    value: String,
    path: String,
    line: u32,
    package: String,
}

/// Constants by prefix (`EC`), from every object carrying `constants` (Perl packages today).
/// Groups of fewer than two constants carry no pattern and are left out.
fn constant_groups(graph: &KirGraph) -> BTreeMap<String, Vec<Constant>> {
    let mut groups: BTreeMap<String, Vec<Constant>> = BTreeMap::new();
    for o in &graph.objects {
        let Some(Value::Array(cs)) = o.properties.get("constants") else {
            continue;
        };
        let path = o
            .properties
            .get("constants_path")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        for c in cs {
            let (Some(name), Some(value)) = (c["name"].as_str(), c["value"].as_str()) else {
                continue;
            };
            // An explicit group (a Python `Enum` class) names its members whole; a Perl constant
            // groups by its prefix (`EC_CUSTOMER` → `EC` / `customer`).
            let (prefix, suffix) = match c["group"].as_str() {
                Some(g) => (g.to_string(), name.to_string()),
                None => match name.split_once('_') {
                    Some((p, s)) => (p.to_string(), s.to_string()),
                    None => continue,
                },
            };
            groups.entry(prefix).or_default().push(Constant {
                name: name.to_string(),
                suffix: suffix.to_lowercase(),
                value: value.to_string(),
                path: path.clone(),
                line: c["line"].as_u64().unwrap_or(0) as u32,
                package: o.name.clone(),
            });
        }
    }
    groups.retain(|_, g| {
        let values: BTreeSet<&str> = g.iter().map(|c| c.value.as_str()).collect();
        g.len() >= 2 && values.len() == g.len()
    });
    groups
}

fn norm(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect()
}

/// The constant group naming `column`'s codes, and how it matched: `labels` (at least two of its
/// names equal the codes' known labels, and none disagree) or `initials` (its prefix is the
/// column's initials — `EC` for `entity_class` — and at least half its values are codes the column
/// is known to take). Ambiguity (two groups) matches nothing.
fn match_constant_group(
    groups: &BTreeMap<String, Vec<Constant>>,
    column: &str,
    values: &BTreeSet<String>,
    labels: &BTreeMap<String, String>,
) -> Option<(String, &'static str)> {
    let mut by_labels = Vec::new();
    let mut by_name = Vec::new();
    let mut by_initials = Vec::new();
    let column_words = words_key(column);
    let initials: String = column
        .split('_')
        .filter(|t| !t.is_empty() && *t != "id")
        .filter_map(|t| t.chars().next())
        .collect::<String>()
        .to_uppercase();
    for (prefix, g) in groups {
        let mut agree = 0;
        let mut disagree = 0;
        for c in g {
            if let Some(l) = labels.get(&c.value) {
                if norm(l) == norm(&c.suffix) {
                    agree += 1;
                } else {
                    disagree += 1;
                }
            }
        }
        if agree >= 2 && disagree == 0 {
            by_labels.push(prefix.clone());
            continue;
        }
        let overlap = g.iter().filter(|c| values.contains(&c.value)).count();
        // `class Status(IntEnum)` for column `status` (or `status_id`).
        let group_words = words_key(prefix);
        if disagree == 0
            && overlap * 2 >= g.len()
            && (group_words == column_words || format!("{group_words} id") == column_words)
        {
            by_name.push(prefix.clone());
            continue;
        }
        if *prefix == initials && initials.len() >= 2 && overlap * 2 >= g.len() && disagree == 0 {
            by_initials.push(prefix.clone());
        }
    }
    match (
        by_labels.as_slice(),
        by_name.as_slice(),
        by_initials.as_slice(),
    ) {
        ([one], _, _) => Some((one.clone(), "labels")),
        ([], [one], _) => Some((one.clone(), "name")),
        ([], [], [one]) => Some((one.clone(), "initials")),
        _ => None,
    }
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
        "is_not_true" => format!("Not{c}OrUnset"),
        "is_not_false" => format!("{c}OrUnset"),
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

/// A `(table index, column)` pair.
type ColKey = (usize, String);
/// Candidate meanings per coded value.
type Meanings = BTreeMap<String, Vec<Meaning>>;
/// Code labels per `(table, column)`: value → best label, for naming concepts.
type Labels = BTreeMap<ColKey, BTreeMap<String, String>>;
/// A concept without a description: (id, key, name, sites).
type Undocumented<'a> = (KirId, String, String, Vec<&'a Site>);
/// A built concept: (id, key, sites).
type BuiltConcept<'a> = (KirId, String, Vec<&'a Site>);
/// Potential conflicts by key: (conflict type, members).
type ConflictGroups = BTreeMap<String, (&'static str, Vec<ConflictMember>)>;

/// What the synthesis steps share: the tables, and the per-column facts read from them and their
/// sites.
struct Facts<'a> {
    tables: &'a [TableInfo<'a>],
    /// (table, column) → value → usage sites.
    coded: BTreeMap<ColKey, BTreeMap<String, Vec<&'a Site>>>,
    /// Declared domains: `CHECK (… IN …)` and dbt `accepted_values`.
    check_domain: BTreeMap<ColKey, BTreeSet<String>>,
    fks: BTreeMap<ColKey, (usize, String)>,
    /// Row keys: a column compared against many distinct literals, or a primary-key/unique column.
    key_like: BTreeSet<ColKey>,
    /// Application constants grouped by prefix: `BC_AP => 1, BC_AR => 2` is the group `BC`.
    groups: BTreeMap<String, Vec<Constant>>,
    /// RFC 0246: Apache Ossie field descriptions per column, with the model file's path.
    ossie_descriptions: BTreeMap<ColKey, Vec<(String, String)>>,
}

impl<'a> Facts<'a> {
    fn new(
        graph: &KirGraph,
        cfg: &SemanticsConfig,
        tables: &'a [TableInfo<'a>],
        sites: &'a [Site],
        index: &NameIndex,
        by_id: &HashMap<Uuid, usize>,
    ) -> Self {
        let mut fks = foreign_keys(graph, by_id);
        add_dbt_references(tables, index, by_id, &mut fks);
        let coded = coded_values(tables, sites);
        let mut check_domain = check_domains(tables);
        add_accepted_values(tables, &mut check_domain);
        let key_like = key_like_columns(tables, &coded, cfg.max_enum_values);
        Facts {
            tables,
            coded,
            check_domain,
            fks,
            key_like,
            groups: constant_groups(graph),
            ossie_descriptions: ossie_descriptions(graph, by_id),
        }
    }

    fn is_key(&self, t: usize, c: &str) -> bool {
        is_key_column(self.tables, t, c)
    }

    /// Every coded column: compared in code, CHECK-constrained, or referencing a seeded lookup.
    fn coded_columns(&self) -> BTreeSet<ColKey> {
        let mut columns: BTreeSet<ColKey> = self.coded.keys().cloned().collect();
        columns.extend(self.check_domain.keys().cloned());
        for ((ti, col), (lt, _)) in &self.fks {
            if self.tables[*lt].obj.properties.contains_key("seed_rows") {
                columns.insert((*ti, col.clone()));
            }
        }
        columns.retain(|k| !self.key_like.contains(k));
        columns
    }
}

/// RFC 0246: the field descriptions the Ossie models in the graph state, per linked table column.
/// The caller passes only current models (the commit step drops a deleted file's objects).
fn ossie_descriptions(
    graph: &KirGraph,
    by_id: &HashMap<Uuid, usize>,
) -> BTreeMap<ColKey, Vec<(String, String)>> {
    let k = ekos_ossie::knowledge::resolve(&graph.objects, &graph.relationships, None);
    let mut out: BTreeMap<ColKey, Vec<(String, String)>> = BTreeMap::new();
    for t in &k.tables {
        let Some(&ti) = by_id.get(&t.table_id.0) else {
            continue;
        };
        let path = t
            .origins
            .first()
            .map(|o| o.trim_start_matches("ossie:").to_string())
            .unwrap_or_default();
        for c in &t.columns {
            if let Some(d) = &c.description {
                out.entry((ti, c.column.to_lowercase()))
                    .or_default()
                    .push((d.clone(), path.clone()));
            }
        }
    }
    out
}

/// RFC 0246: Apache Ossie metric and table descriptions as glossary entries — the term a metric's
/// or a dataset's synonym, the definition its description. They document a concept of the same
/// words; one that matches nothing is not a gap.
fn ossie_gloss(graph: &KirGraph) -> Vec<GlossEntry> {
    use ekos_ossie::kinds::{OSSIE_DATASET, OSSIE_METRIC};
    let mut out = Vec::new();
    for o in &graph.objects {
        if !(is_custom(o, OSSIE_METRIC) || is_custom(o, OSSIE_DATASET)) {
            continue;
        }
        let Some(definition) = o
            .properties
            .get("description")
            .and_then(Value::as_str)
            .filter(|d| !d.trim().is_empty())
        else {
            continue;
        };
        let path = o
            .properties
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let line = o
            .properties
            .get("line")
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32;
        let synonyms = o
            .properties
            .get("synonyms")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str);
        for term in std::iter::once(o.name.as_str()).chain(synonyms) {
            out.push(GlossEntry {
                term: term.to_string(),
                definition: definition.to_string(),
                path: path.clone(),
                line,
                owner: o.id,
                source: "ossie",
            });
        }
    }
    out
}

/// A primary-key or unique column.
fn is_key_column(tables: &[TableInfo], t: usize, c: &str) -> bool {
    tables[t].columns.get(c).is_some_and(|c| c.key)
}

/// Tables by name and by id.
fn index_tables(tables: &[TableInfo]) -> (NameIndex, HashMap<Uuid, usize>) {
    let mut index = NameIndex::default();
    let mut by_id: HashMap<Uuid, usize> = HashMap::new();
    for (i, t) in tables.iter().enumerate() {
        index.add_object(t.obj);
        by_id.insert(t.obj.id.0, i);
    }
    (index, by_id)
}

/// A ledger hands objects back in no particular order. Everything downstream — which sites an
/// item cites when it has more than `MAX_EVIDENCE`, evidence ids by index, signatures — must not
/// depend on it, or every commit rewrites the busiest items.
fn sort_sites(sites: &mut [Site]) {
    sites.sort_by(|a, b| {
        (&a.path, a.p.line, &a.carrier_name, a.carrier.0, &a.p).cmp(&(
            &b.path,
            b.p.line,
            &b.carrier_name,
            b.carrier.0,
            &b.p,
        ))
    });
}

/// A dbt `relationships` test declares the same thing a foreign key does (RFC 0170 Phase 3).
fn add_dbt_references(
    tables: &[TableInfo],
    index: &NameIndex,
    by_id: &HashMap<Uuid, usize>,
    fks: &mut BTreeMap<ColKey, (usize, String)>,
) {
    for (ti, t) in tables.iter().enumerate() {
        for c in raw_columns(t.obj) {
            let (Some(col), Some(r)) = (c["name"].as_str(), c.get("references")) else {
                continue;
            };
            let (Some(rt), Some(rc)) = (r["table"].as_str(), r["column"].as_str()) else {
                continue;
            };
            if let Ok((id, _)) = index.resolve(rt)
                && let Some(&lt) = by_id.get(&id.0)
            {
                fks.entry((ti, col.to_lowercase()))
                    .or_insert((lt, rc.to_lowercase()));
            }
        }
    }
}

/// Coded values per column: the literals of `IN`/`NOT IN` sites, except on quantity and boolean
/// columns.
fn coded_values<'a>(
    tables: &[TableInfo],
    sites: &'a [Site],
) -> BTreeMap<ColKey, BTreeMap<String, Vec<&'a Site>>> {
    let mut coded: BTreeMap<ColKey, BTreeMap<String, Vec<&Site>>> = BTreeMap::new();
    for s in sites {
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
    coded
}

/// `CHECK (kind IN ('A', 'L'))` declares a column's whole domain.
fn check_domains(tables: &[TableInfo]) -> BTreeMap<ColKey, BTreeSet<String>> {
    let mut check_domain: BTreeMap<ColKey, BTreeSet<String>> = BTreeMap::new();
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
    check_domain
}

/// A dbt `accepted_values` test declares a domain the same way.
fn add_accepted_values(
    tables: &[TableInfo],
    check_domain: &mut BTreeMap<ColKey, BTreeSet<String>>,
) {
    for (ti, t) in tables.iter().enumerate() {
        for c in raw_columns(t.obj) {
            let (Some(col), Some(Value::Array(vals))) =
                (c["name"].as_str(), c.get("accepted_values"))
            else {
                continue;
            };
            check_domain
                .entry((ti, col.to_lowercase()))
                .or_default()
                .extend(vals.iter().filter_map(|v| v.as_str().map(str::to_string)));
        }
    }
}

/// Row keys: a column compared against many distinct literals, or a primary-key/unique column.
fn key_like_columns(
    tables: &[TableInfo],
    coded: &BTreeMap<ColKey, BTreeMap<String, Vec<&Site>>>,
    max_enum_values: usize,
) -> BTreeSet<ColKey> {
    coded
        .iter()
        .filter(|((t, c), vals)| vals.len() > max_enum_values || is_key_column(tables, *t, c))
        .map(|(k, _)| k.clone())
        .collect()
}

/// Synthesize every RFC 0170 hypothesis from `graph` (the committed ledger's objects and
/// relationships).
pub fn synthesize(
    graph: &KirGraph,
    cfg: &SemanticsConfig,
    rationale: Option<&dyn RationaleSource>,
) -> SemanticsOutput {
    let mut b = Builder {
        out: SemanticsOutput::default(),
    };

    // Tables, indexed by name. Views are not tables: a coded column is a stored column.
    let tables: Vec<TableInfo> = graph
        .objects
        .iter()
        .filter(|o| o.kind == ObjectKind::Table)
        .map(table_info)
        .collect();
    let (index, by_id) = index_tables(&tables);
    let mut sites = collect_sites(graph, &tables, &index, &by_id, &mut b.out.stats);
    sort_sites(&mut sites);
    let facts = Facts::new(graph, cfg, &tables, &sites, &index, &by_id);
    b.out.stats.key_like_columns = facts.key_like.len();

    let (labels, unexplained) = enum_meanings(&mut b, &facts);

    let mut concepts = view_concepts(graph, &tables, &sites);
    add_recurring_concepts(&mut concepts, cfg, &facts, &sites, &labels);
    let (mut undocumented, built) = emit_concepts(&mut b, &tables, &concepts);
    emit_conflicts(&mut b, &tables, &concepts);

    emit_dbt_constraints(&mut b, &tables);
    emit_check_constraints(&mut b, &tables);

    let glossary = glossary_entries(graph);
    let (glossed, unmatched) = attach_glossary(&mut b, &tables, &glossary);
    undocumented.retain(|(id, ..)| !glossed.contains(&id.0));

    // ── Rationale (git blame) ───────────────────────────────────────────────────────────────
    let mut explained: BTreeSet<Uuid> = BTreeSet::new();
    for (id, key, ss) in &built {
        blame_into(&mut b, rationale, &mut explained, *id, key, ss);
    }

    // ── SemanticGap ─────────────────────────────────────────────────────────────────────────
    let gaps: Vec<GapDraft> = unexplained
        .into_iter()
        .map(|u| value_gap(&tables, u))
        .chain(
            undocumented
                .into_iter()
                .filter(|(id, ..)| !explained.contains(&id.0))
                .map(concept_gap),
        )
        .collect();
    for gap in gaps {
        emit_gap(&mut b, &tables, rationale, &mut explained, gap);
    }
    for g in unmatched {
        emit_term_gap(&mut b, g);
    }

    b.out.stats.rationale_links = b
        .out
        .objects
        .iter()
        .filter(|o| kind_name(o) == Some(RATIONALE))
        .count();
    testimony_meanings(&mut b.out, graph);
    if let Some(d) = &cfg.data {
        let subjects: HashMap<KirId, &KirObject> = graph
            .objects
            .iter()
            .filter(|o| o.kind == ObjectKind::Table)
            .map(|o| (o.id, o))
            .collect();
        let fs = crate::data_usage::fold(&mut b.out, &subjects, d);
        b.out.stats.data_values_measured = fs.values_measured;
        b.out.stats.dead_codes = fs.dead;
        b.out.stats.never_seen_codes = fs.never_seen;
        b.out.stats.data_only_gaps = fs.data_only_gaps;
        b.out.stats.gaps += fs.data_only_gaps;
        if let Some(src) = rationale {
            let ds = crate::data_usage::date_codes(&mut b.out, src);
            b.out.stats.data_before_code = ds.data_before_code;
            b.out.stats.new_value_columns = ds.new_value_columns;
        }
    }
    suggest_mappings(&mut b.out, cfg);
    sign_items(&mut b.out);
    cite_endpoint_evidence(&mut b.out);
    b.out
}

/// RFC 0253: a person's stated meaning of a code value is a meaning source (`testimony`) of its
/// `EnumMeaning`, cited to the testimony. It never replaces a label code gives — a disagreement is
/// a `testimony_mismatch` conflict, recorded when the testimony was saved.
fn testimony_meanings(out: &mut SemanticsOutput, graph: &KirGraph) {
    let mut newest: BTreeMap<String, &KirObject> = BTreeMap::new();
    for t in graph.objects.iter().filter(|o| is_custom(o, "Testimony")) {
        let upd = |o: &KirObject| {
            o.properties
                .get("updated_at")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string()
        };
        let k = t.id.to_string();
        if newest.get(&k).is_none_or(|old| upd(t) >= upd(old)) {
            newest.insert(k, t);
        }
    }
    for t in newest.values() {
        let p = |k: &str| t.properties.get(k).and_then(Value::as_str).unwrap_or("");
        if p("status") != "active" || p("meaning").is_empty() {
            continue;
        }
        let about: BTreeSet<String> = t
            .properties
            .get("about")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        for o in out
            .objects
            .iter_mut()
            .filter(|o| kind_name(o) == Some(ENUM_MEANING) && about.contains(&o.id.to_string()))
        {
            let m = json!({
                "label": p("meaning"), "source": "testimony", "confidence": 0.9,
                "path": format!("testimony://{}", t.id), "line": null, "author": p("author"),
            });
            if let Some(Value::Array(ms)) = o.properties.get_mut("meanings") {
                ms.push(m);
            } else {
                o.properties.insert("meanings".into(), json!([m]));
            }
            if o.properties.get("label").is_none_or(Value::is_null) {
                o.properties.insert("label".into(), json!(p("meaning")));
                o.properties.insert("confidence".into(), json!(0.9));
            }
        }
    }
}

/// Evidence for one usage site: its canonical predicate, carrier and clause.
fn site_evidence(b: &mut Builder, owner: &str, n: usize, s: &Site, tables: &[TableInfo]) -> KirId {
    b.evidence(
        owner,
        n,
        &s.path,
        site_line(s),
        format!(
            "{} ({}, {:?})",
            s.canonical(tables),
            s.carrier_name,
            s.p.clause
        ),
    )
}

// ── EnumMeaning per (table, column, value) ─────────────────────────────────────────────────

/// One `EnumMeaning` per value of every coded column. Returns the code labels found, per column,
/// and the values used in logic that nothing explains.
fn enum_meanings<'a>(b: &mut Builder, f: &Facts<'a>) -> (Labels, Vec<Unexplained<'a>>) {
    let mut labels = Labels::new();
    let mut unexplained = Vec::new();
    for (ti, col) in &f.coded_columns() {
        column_enum_meanings(b, f, *ti, col, &mut labels, &mut unexplained);
    }
    (labels, unexplained)
}

/// The `EnumMeaning`s of one coded column; none when it has no known value.
fn column_enum_meanings<'a>(
    b: &mut Builder,
    f: &Facts<'a>,
    ti: usize,
    col: &str,
    labels: &mut Labels,
    unexplained: &mut Vec<Unexplained<'a>>,
) {
    let (mut meanings, declared) = column_meanings(f, ti, col);
    let key = (ti, col.to_string());
    let mut values: BTreeSet<String> = meanings.keys().cloned().collect();
    if let Some(v) = f.coded.get(&key) {
        values.extend(v.keys().cloned());
    }
    if let Some(v) = f.check_domain.get(&key) {
        values.extend(v.iter().cloned());
    }
    if values.is_empty() {
        return;
    }
    b.out.stats.coded_columns += 1;
    let col_labels = labels.entry(key).or_default();
    for value in values {
        let ms = meanings.remove(&value).unwrap_or_default();
        let declared_in = declared.get(&value).cloned();
        let best = enum_value(b, f, (ti, col), value.clone(), ms, declared_in, unexplained);
        if let Some(label) = best {
            col_labels.insert(value, label);
        }
    }
}

/// Builds the `EnumMeaning` of one value of `(ti, col)`, and records the value as unexplained
/// when logic uses it but no source names it. Returns its best label.
fn enum_value<'a>(
    b: &mut Builder,
    f: &Facts<'a>,
    (ti, col): (usize, &str),
    value: String,
    mut ms: Vec<Meaning>,
    declared_in: Option<String>,
    unexplained: &mut Vec<Unexplained<'a>>,
) -> Option<String> {
    let t = &f.tables[ti];
    let ck = (ti, col.to_string());
    let used: Vec<&Site> = f
        .coded
        .get(&ck)
        .and_then(|m| m.get(&value))
        .cloned()
        .unwrap_or_default();
    ms.sort_by(|a, b| {
        b.confidence
            .total_cmp(&a.confidence)
            .then(a.label.cmp(&b.label))
    });
    ms.dedup_by(|a, b| a.label == b.label && a.source == b.source);
    let key = format!("enum-meaning:{}:{col}:{value}", t.obj.id);
    let name = format!("{}.{col} = {value}", t.name());
    let best = ms.first().cloned();
    if best.is_some() {
        b.out.stats.enum_values_explained += 1;
    }
    b.out.stats.enum_values += 1;
    let in_check = f.check_domain.get(&ck).is_some_and(|d| d.contains(&value));
    let idx = b.object(
        &key,
        name,
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
                json!(
                    ms.iter()
                        .map(|m| json!({
                            "label": m.label, "source": m.source, "confidence": m.confidence,
                            "path": m.path, "line": m.line,
                        }))
                        .collect::<Vec<_>>()
                ),
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
        evs.push(site_evidence(b, &key, 100 + n, s, f.tables));
    }
    let id = b.out.objects[idx].id;
    b.out.objects[idx].evidence = evs;
    b.relate(DESCRIBES, id, t.obj.id);
    if best.is_none() && !used.is_empty() {
        unexplained.push(Unexplained {
            table: ti,
            column: col.to_string(),
            value,
            used,
            declared_in,
        });
    }
    best.map(|m| m.label)
}

/// Every candidate meaning of `(ti, col)`'s values, and the values a lookup table seeds without
/// any label column (declared, not explained).
fn column_meanings(f: &Facts, ti: usize, col: &str) -> (Meanings, BTreeMap<String, String>) {
    let mut meanings = Meanings::new();
    let mut declared = BTreeMap::new();
    comment_meanings(f, ti, col, &mut meanings);
    ossie_meanings(f, ti, col, &mut meanings);
    fk_seed_meanings(f, ti, col, &mut meanings, &mut declared);
    own_seed_meanings(f, ti, col, &mut meanings);
    constant_meanings(f, ti, col, &mut meanings);
    dbt_var_meanings(f, ti, col, &mut meanings);
    case_label_meanings(f, ti, col, &mut meanings);
    (meanings, declared)
}

/// RFC 0246: the legend in an Apache Ossie field description (`Y = preferred, N = not`) — the
/// team's semantic model stating what the codes mean.
fn ossie_meanings(f: &Facts, ti: usize, col: &str, meanings: &mut Meanings) {
    let key = (ti, col.to_string());
    for (text, path) in f.ossie_descriptions.get(&key).into_iter().flatten() {
        for (code, label) in comment_legend(text) {
            let value = match_value(&code, f.coded.get(&key), f.check_domain.get(&key));
            meanings.entry(value).or_default().push(Meaning {
                label,
                source: "ossie",
                confidence: 0.85,
                path: path.clone(),
                line: None,
                detail: format!("Apache Ossie field description: {text}"),
            });
        }
    }
}

/// The column comment's legend: `A=asset,L=liability`.
fn comment_meanings(f: &Facts, ti: usize, col: &str, meanings: &mut Meanings) {
    let t = &f.tables[ti];
    let Some(c) = t.columns.get(col) else {
        return;
    };
    let Some(text) = &c.description else {
        return;
    };
    let key = (ti, col.to_string());
    for (code, label) in comment_legend(text) {
        let value = match_value(&code, f.coded.get(&key), f.check_domain.get(&key));
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

/// Where a seeded row was written.
fn seed_row_location(row: &Value) -> (String, Option<u32>) {
    (
        row.get("path")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        row.get("line").and_then(Value::as_u64).map(|l| l as u32),
    )
}

/// Lookup seed through a foreign key: the referenced table's seeded rows label the codes. A row
/// with no label column only declares its code.
fn fk_seed_meanings(
    f: &Facts,
    ti: usize,
    col: &str,
    meanings: &mut Meanings,
    declared: &mut BTreeMap<String, String>,
) {
    let t = &f.tables[ti];
    let Some((lt, key_col)) = f.fks.get(&(ti, col.to_string())) else {
        return;
    };
    let lookup = &f.tables[*lt];
    let Some(Value::Array(rows)) = lookup.obj.properties.get("seed_rows") else {
        return;
    };
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
            declared.insert(code, lookup.name());
            continue;
        };
        let (path, line) = seed_row_location(row);
        meanings.entry(code).or_default().push(Meaning {
            label,
            source: "lookup_seed",
            confidence: 0.8,
            path,
            line,
            detail: format!(
                "{}.{col} → {}.{key_col}; seeded row {}",
                t.name(),
                lookup.name(),
                Value::Object(values.clone())
            ),
        });
    }
}

/// The lookup table's own key: `oe_class.id = 2` means what `oe_class`'s seeded row 2 says.
fn own_seed_meanings(f: &Facts, ti: usize, col: &str, meanings: &mut Meanings) {
    let t = &f.tables[ti];
    let Some(Value::Array(rows)) = t.obj.properties.get("seed_rows") else {
        return;
    };
    if f.fks.contains_key(&(ti, col.to_string())) {
        return;
    }
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
        let (path, line) = seed_row_location(row);
        meanings.entry(seed_code(key)).or_default().push(Meaning {
            label,
            source: "lookup_seed",
            confidence: 0.8,
            path,
            line,
            detail: format!("{}: seeded row {}", t.name(), Value::Object(values.clone())),
        });
    }
}

/// Application constants (`use constant EC_CUSTOMER => 2`): a group whose names agree with this
/// column's seeded labels, or whose prefix is the column's initials and whose values overlap its
/// codes, names those codes.
fn constant_meanings(f: &Facts, ti: usize, col: &str, meanings: &mut Meanings) {
    let key = (ti, col.to_string());
    let mut known_values: BTreeSet<String> = meanings.keys().cloned().collect();
    if let Some(m) = f.coded.get(&key) {
        known_values.extend(m.keys().cloned());
    }
    if let Some(d) = f.check_domain.get(&key) {
        known_values.extend(d.iter().cloned());
    }
    let labels_now: BTreeMap<String, String> = meanings
        .iter()
        .filter_map(|(v, ms)| ms.first().map(|m| (v.clone(), m.label.clone())))
        .collect();
    let Some((prefix, how)) = match_constant_group(&f.groups, col, &known_values, &labels_now)
    else {
        return;
    };
    let confidence = match how {
        "labels" => 0.6,
        "name" => 0.5,
        _ => 0.4,
    };
    for c in &f.groups[&prefix] {
        meanings.entry(c.value.clone()).or_default().push(Meaning {
            label: c.suffix.clone(),
            source: "app_constant",
            confidence,
            path: c.path.clone(),
            line: Some(c.line),
            detail: format!(
                "use constant {} => {} ({}; matched by {how})",
                c.name, c.value, c.package
            ),
        });
    }
}

/// dbt var names: `account_number = '{{ var("acc_ar") }}'` says '1200' is the AR account.
fn dbt_var_meanings(f: &Facts, ti: usize, col: &str, meanings: &mut Meanings) {
    let Some(vals) = f.coded.get(&(ti, col.to_string())) else {
        return;
    };
    for (v, used) in vals {
        let mut seen = BTreeSet::new();
        for s in used {
            if let Some(var) = &s.var_label
                && seen.insert(var.clone())
            {
                meanings.entry(v.clone()).or_default().push(Meaning {
                    label: var.clone(),
                    source: "dbt_var",
                    confidence: 0.4,
                    path: s.path.clone(),
                    line: site_line(s),
                    detail: format!(
                        "{} — the value of dbt var `{var}` ({})",
                        s.canonical(f.tables),
                        s.carrier_name
                    ),
                });
            }
        }
    }
}

/// CASE branch labels.
fn case_label_meanings(f: &Facts, ti: usize, col: &str, meanings: &mut Meanings) {
    let Some(vals) = f.coded.get(&(ti, col.to_string())) else {
        return;
    };
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
                        s.canonical(f.tables),
                        s.carrier_name
                    ),
                });
            }
        }
    }
}

// ── BusinessConcept ─────────────────────────────────────────────────────────────────────────

/// A `BusinessConcept` before it is built, keyed by its definition text.
struct Concept<'a> {
    origin: &'static str,
    view: Option<&'a KirObject>,
    atoms: BTreeSet<String>,
    sites: Vec<&'a Site>,
    name: String,
    table: usize,
}

/// A clause that filters rows.
fn is_filter_clause(c: Clause) -> bool {
    matches!(
        c,
        Clause::Where | Clause::Having | Clause::JoinOn | Clause::Condition
    )
}

/// Views: the outermost WHERE's resolved top-level conjuncts define "rows of the view".
fn view_concepts<'a>(
    graph: &'a KirGraph,
    tables: &[TableInfo],
    sites: &'a [Site],
) -> BTreeMap<String, Concept<'a>> {
    let mut concepts: BTreeMap<String, Concept> = BTreeMap::new();
    let views: BTreeMap<Uuid, &KirObject> = graph
        .objects
        .iter()
        .filter(|o| is_custom(o, "View") || is_dbt_model(o))
        .map(|o| (o.id.0, o))
        .collect();
    let mut view_sites: BTreeMap<Uuid, Vec<&Site>> = BTreeMap::new();
    for s in sites {
        if s.carrier_is_view && s.p.clause == Clause::Where && s.p.top_level && !s.p.subquery {
            view_sites.entry(s.carrier.0).or_default().push(s);
        }
    }
    for (vid, vs) in &view_sites {
        let atoms: BTreeSet<String> = vs.iter().map(|s| s.canonical(tables)).collect();
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
    concepts
}

/// Recurring single predicates: one that defines a view widens that view's concept; one seen in at
/// least `min_sites` carriers is a concept of its own.
fn add_recurring_concepts<'a>(
    concepts: &mut BTreeMap<String, Concept<'a>>,
    cfg: &SemanticsConfig,
    f: &Facts<'a>,
    sites: &'a [Site],
    labels: &Labels,
) {
    let mut by_canon: BTreeMap<String, Vec<&Site>> = BTreeMap::new();
    for s in sites {
        if is_filter_clause(s.p.clause)
            && !f.key_like.contains(&(s.table, s.column.clone()))
            && !f.is_key(s.table, &s.column)
        {
            by_canon.entry(s.canonical(f.tables)).or_default().push(s);
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
            &f.tables[s0.table].name(),
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
}

/// Builds every concept. Returns the ones with no description, and every one built.
fn emit_concepts<'a>(
    b: &mut Builder,
    tables: &[TableInfo],
    concepts: &BTreeMap<String, Concept<'a>>,
) -> (Vec<Undocumented<'a>>, Vec<BuiltConcept<'a>>) {
    let mut undocumented = Vec::new();
    let mut built = Vec::new();
    for (def, c) in concepts {
        let key = format!("business-concept:{def}");
        let (id, described) = emit_concept(b, tables, &key, def, c);
        if !described {
            undocumented.push((id, key.clone(), c.name.clone(), c.sites.clone()));
        }
        built.push((id, key, c.sites.clone()));
    }
    (undocumented, built)
}

/// Builds one `BusinessConcept`; returns its id and whether its view describes it.
fn emit_concept(
    b: &mut Builder,
    tables: &[TableInfo],
    key: &str,
    def: &str,
    c: &Concept,
) -> (KirId, bool) {
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
    let tables_named: BTreeSet<String> = c.sites.iter().map(|s| tables[s.table].name()).collect();
    let mut seen_in: Vec<String> = c
        .sites
        .iter()
        .map(|s| s.carrier_name.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    seen_in.truncate(50);
    let name_source = if c.origin == "view" {
        "view"
    } else {
        "derived"
    };
    let idx = b.object(
        key,
        c.name.clone(),
        CONCEPT,
        vec![
            ("definition", json!(def)),
            ("predicates", json!(c.atoms)),
            ("table", json!(tables[c.table].name())),
            ("tables", json!(tables_named)),
            ("origin", json!(c.origin)),
            ("name_source", json!(name_source)),
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
        evs.push(site_evidence(b, key, n, s, tables));
    }
    b.out.objects[idx].evidence = evs;
    for t in c.sites.iter().map(|s| s.table).collect::<BTreeSet<_>>() {
        b.relate(DESCRIBES, id, tables[t].obj.id);
    }
    for carrier in &carriers {
        b.relate(EVIDENCED_BY, id, KirId(*carrier));
    }
    b.out.stats.concepts += 1;
    if c.origin == "view" {
        b.out.stats.concepts_from_views += 1;
    }
    (id, description.is_some())
}

// ── ConceptConflict ─────────────────────────────────────────────────────────────────────────

/// Two kinds, both narrow on purpose. `IN` sets on one column that merely differ are usually
/// different concepts (asset vs income accounts), not a disagreement, so they are not flagged.
///  - threshold: one column, the same comparison direction, different literals — "overdue" as
///    `> 90` in one place and `> 60` in another;
///  - name: one name for different definitions.
fn emit_conflicts(b: &mut Builder, tables: &[TableInfo], concepts: &BTreeMap<String, Concept>) {
    for (key, (conflict_type, members)) in conflict_groups(tables, concepts) {
        if members.len() >= 2 {
            emit_conflict(b, &key, conflict_type, &members);
        }
    }
}

/// The direction of a threshold comparison.
fn threshold_direction(op: &str) -> Option<&'static str> {
    match op {
        ">" | ">=" => Some("above"),
        "<" | "<=" => Some("below"),
        _ => None,
    }
}

/// Concepts grouped by what they could conflict over: a threshold's column and direction, a name.
fn conflict_groups(tables: &[TableInfo], concepts: &BTreeMap<String, Concept>) -> ConflictGroups {
    let mut conflicts = ConflictGroups::new();
    for (def, c) in concepts {
        let id = kid(&format!("business-concept:{def}"));
        if c.atoms.len() == 1
            && let Some(s0) = c.sites.first()
            && s0.p.values.len() == 1
            && let Some(dir) = threshold_direction(s0.p.op.as_str())
        {
            conflicts
                .entry(format!(
                    "threshold:{}.{}:{dir}",
                    tables[s0.table].name(),
                    s0.column
                ))
                .or_insert(("threshold", Vec::new()))
                .1
                .push((id, c.name.clone(), def.clone()));
        }
        conflicts
            .entry(format!("name:{}", c.name))
            .or_insert(("name", Vec::new()))
            .1
            .push((id, c.name.clone(), def.clone()));
    }
    conflicts
}

/// Builds one `ConceptConflict` over `members`.
fn emit_conflict(
    b: &mut Builder,
    key: &str,
    conflict_type: &'static str,
    members: &[ConflictMember],
) {
    let names: Vec<String> = members.iter().map(|m| m.1.clone()).collect();
    let defs: Vec<String> = members.iter().map(|m| m.2.clone()).collect();
    let question = match conflict_type {
        "threshold" => format!(
            "Which threshold is the business rule? The code uses {} — one meaning, or several?",
            defs.join(" vs ")
        ),
        _ => format!(
            "`{}` names {} different definitions: {}. Which is it?",
            names[0],
            defs.len(),
            defs.join(" vs ")
        ),
    };
    let okey = format!("concept-conflict:{key}");
    let idx = b.object(
        &okey,
        format!("conflict: {}", defs.join(" vs ")),
        CONFLICT,
        vec![
            ("conflict_type", json!(conflict_type)),
            ("concepts", json!(names)),
            ("definitions", json!(defs)),
            ("question", json!(question)),
            ("confidence", json!(0.5)),
        ],
    );
    let id = b.out.objects[idx].id;
    // Its evidence is its members': each concept's first cited site.
    let evs: Vec<KirId> = members
        .iter()
        .filter_map(|(cid, _, _)| {
            b.out
                .objects
                .iter()
                .find(|o| o.id == *cid)
                .and_then(|o| o.evidence.first().copied())
        })
        .collect();
    b.out.objects[idx].evidence = evs;
    for (cid, _, _) in members {
        b.relate(CONFLICTS_WITH, id, *cid);
    }
    b.out.stats.conflicts += 1;
}

// ── ConstraintCandidate ─────────────────────────────────────────────────────────────────────

/// What a dbt test asserts: (constraint type, expression, structured predicates).
type DbtConstraint = (&'static str, String, Vec<Value>);

/// `ConstraintCandidate` per dbt test (RFC 0170 Phase 3).
fn emit_dbt_constraints(b: &mut Builder, tables: &[TableInfo]) {
    for t in tables {
        for c in raw_columns(t.obj) {
            let Some(col) = c["name"].as_str().map(str::to_lowercase) else {
                continue;
            };
            let tests: Vec<&str> = c["dbt_tests"]
                .as_array()
                .map(|a| a.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            for test in tests {
                if let Some(constraint) = dbt_test_constraint(t, c, &col, test) {
                    emit_dbt_constraint(b, t, c, &col, test, constraint);
                }
            }
        }
    }
}

/// A dbt test's `CHECK`-clause predicate on `col`, as stored.
fn dbt_check_site(t: &TableInfo, col: &str, op: &str, values: Vec<String>) -> Value {
    serde_json::to_value(PredicateSite {
        relation: Some(t.name()),
        scope: Vec::new(),
        column: col.to_string(),
        op: op.into(),
        values,
        clause: Clause::Check,
        top_level: true,
        subquery: false,
        label: None,
        case_else: None,
        null_handled: None,
        line: 0,
    })
    .unwrap_or_default()
}

/// What dbt test `test` on column entry `c` asserts; `None` for a test that is not a constraint.
fn dbt_test_constraint(t: &TableInfo, c: &Value, col: &str, test: &str) -> Option<DbtConstraint> {
    Some(match test {
        "not_null" => (
            "not_null",
            format!("{col} IS NOT NULL"),
            vec![dbt_check_site(t, col, "is_not_null", vec![])],
        ),
        "unique" => ("unique", format!("{col} is unique"), vec![]),
        "accepted_values" => {
            let vals: Vec<String> = c["accepted_values"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            (
                "enum",
                canonical_text(col, "in", &vals),
                vec![dbt_check_site(t, col, "in", vals)],
            )
        }
        "relationships" => (
            "relationship",
            format!(
                "{col} references {}.{}",
                c["references"]["table"].as_str().unwrap_or("?"),
                c["references"]["column"].as_str().unwrap_or("?")
            ),
            vec![],
        ),
        _ => return None,
    })
}

/// Builds the `ConstraintCandidate` of one dbt test.
fn emit_dbt_constraint(
    b: &mut Builder,
    t: &TableInfo,
    c: &Value,
    col: &str,
    test: &str,
    (constraint_type, expression, structured): DbtConstraint,
) {
    let key = format!("constraint-candidate:dbt:{}:{col}:{test}", t.obj.id);
    let path = c["dbt_path"].as_str().unwrap_or_default().to_string();
    let line = c["dbt_line"].as_u64().map(|l| l as u32);
    let preds: Vec<String> = structured
        .iter()
        .filter_map(|v| serde_json::from_value::<PredicateSite>(v.clone()).ok())
        .map(|p| p.canonical())
        .collect();
    let idx = b.object(
        &key,
        format!("{} dbt {test} ({col})", t.name()),
        CONSTRAINT,
        vec![
            ("table", json!(t.name())),
            ("constraint_name", json!(format!("dbt:{test}"))),
            ("expression", json!(expression)),
            ("constraint_type", json!(constraint_type)),
            ("source", json!("dbt_test")),
            ("columns", json!([col])),
            ("predicates", json!(preds)),
            ("structured", json!(structured)),
            ("confidence", json!(0.9)),
        ],
    );
    let ev = b.evidence(
        &key,
        0,
        &path,
        line,
        format!("dbt test `{test}` on {}.{col}", t.name()),
    );
    b.out.objects[idx].evidence.push(ev);
    let id = b.out.objects[idx].id;
    b.relate(DESCRIBES, id, t.obj.id);
    b.out.stats.constraints += 1;
}

/// `ConstraintCandidate` per CHECK.
fn emit_check_constraints(b: &mut Builder, tables: &[TableInfo]) {
    for t in tables {
        let Some(Value::Array(checks)) = t.obj.properties.get("check_constraints") else {
            continue;
        };
        for (n, c) in checks.iter().enumerate() {
            emit_check_constraint(b, t, n, c);
        }
    }
}

/// What a CHECK's top-level predicates constrain: a range, an enum, a pattern, non-null, or other.
fn check_constraint_type(top: &[&PredicateSite]) -> &'static str {
    if top.is_empty() {
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
    }
}

/// Builds the `ConstraintCandidate` of the `n`th CHECK of `t`.
fn emit_check_constraint(b: &mut Builder, t: &TableInfo, n: usize, c: &Value) {
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
    let constraint_type = check_constraint_type(&top);
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
    b.out.stats.constraints += 1;
}

// ── Glossary (RFC 0170 Phase 3) ─────────────────────────────────────────────────────────────

/// A glossary term attaches to a concept or a code label with exactly the same words; one that
/// matches nothing recovered — no concept, code or table — is a reverse gap: written down, but
/// no trace in code. Returns the ids of the items documented and the entries that matched nothing.
fn attach_glossary<'g>(
    b: &mut Builder,
    tables: &[TableInfo],
    glossary: &'g [GlossEntry],
) -> (BTreeSet<Uuid>, Vec<&'g GlossEntry>) {
    let table_words: BTreeSet<String> = tables.iter().map(|t| words_key(&t.name())).collect();
    let mut glossed: BTreeSet<Uuid> = BTreeSet::new();
    let mut unmatched: Vec<&GlossEntry> = Vec::new();
    for (gi, g) in glossary.iter().enumerate() {
        let key = words_key(&g.term);
        if key.is_empty() {
            continue;
        }
        let documented = gloss_items(b, gi, g, &key, &mut glossed);
        // An Ossie model names metrics and tables, not a written promise of a concept: matching
        // nothing is no gap.
        if !documented && !table_words.contains(&key) && g.source == "glossary" {
            unmatched.push(g);
        }
    }
    (glossed, unmatched)
}

/// The words a glossary term must equal to document an item: a concept's name, a code's label.
fn gloss_target(o: &KirObject) -> Option<String> {
    match kind_name(o) {
        Some(k) if k == CONCEPT => Some(words_key(&o.name)),
        Some(k) if k == ENUM_MEANING => Some(
            o.properties
                .get("label")
                .and_then(Value::as_str)
                .map(words_key)
                .unwrap_or_default(),
        ),
        _ => None,
    }
}

/// Attaches glossary entry `gi` to every item whose words are `key`; true when one matched.
fn gloss_items(
    b: &mut Builder,
    gi: usize,
    g: &GlossEntry,
    key: &str,
    glossed: &mut BTreeSet<Uuid>,
) -> bool {
    let mut hit = false;
    for idx in 0..b.out.objects.len() {
        let o = &b.out.objects[idx];
        if gloss_target(o).as_deref() != Some(key) {
            continue;
        }
        hit = true;
        let owner_key = format!("glossary:{}:{gi}", o.id);
        let ev = b.evidence(
            &owner_key,
            0,
            &g.path,
            (g.line > 0).then_some(g.line),
            format!("glossary: {} — {}", g.term, g.definition),
        );
        let o = &mut b.out.objects[idx];
        o.evidence.push(ev);
        add_gloss(o, g);
        glossed.insert(o.id.0);
    }
    hit
}

/// Records a glossary entry on an item; a concept with no description takes its definition.
fn add_gloss(o: &mut KirObject, g: &GlossEntry) {
    let entry = json!({"term": g.term, "definition": g.definition, "path": g.path, "line": g.line});
    match o.properties.get_mut("glossary") {
        Some(Value::Array(a)) => a.push(entry),
        _ => {
            o.properties.insert("glossary".into(), json!([entry]));
        }
    }
    if kind_name(o) == Some(CONCEPT) && o.properties.get("description").is_none_or(Value::is_null) {
        o.properties
            .insert("description".into(), json!(g.definition));
        o.properties
            .insert("description_source".into(), json!(g.source));
    }
}

// ── SemanticGap ─────────────────────────────────────────────────────────────────────────────

/// The gap of a value used in logic that nothing explains.
fn value_gap<'a>(tables: &[TableInfo], u: Unexplained<'a>) -> GapDraft<'a> {
    let Unexplained {
        table: ti,
        column: col,
        value,
        used,
        declared_in: lookup,
    } = u;
    let t = &tables[ti];
    let carriers: BTreeSet<String> = used.iter().map(|s| s.carrier_name.clone()).collect();
    let declared = lookup
        .as_ref()
        .map(|l| format!(" `{l}` seeds it, but with no label or description column."))
        .unwrap_or_default();
    GapDraft {
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
    }
}

/// The gap of a concept no comment or commit explains.
fn concept_gap((id, _key, name, ss): Undocumented<'_>) -> GapDraft<'_> {
    GapDraft {
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
    }
}

/// Builds one `SemanticGap`.
fn emit_gap(
    b: &mut Builder,
    tables: &[TableInfo],
    rationale: Option<&dyn RationaleSource>,
    explained: &mut BTreeSet<Uuid>,
    gap: GapDraft,
) {
    let GapDraft {
        key,
        name,
        props,
        subject,
        used,
    } = gap;
    let idx = b.object(&key, name, GAP, props);
    let mut evs = Vec::new();
    for (n, s) in used.iter().enumerate().take(MAX_EVIDENCE) {
        evs.push(site_evidence(b, &key, n, s, tables));
    }
    b.out.objects[idx].evidence = evs;
    let id = b.out.objects[idx].id;
    b.relate(DESCRIBES, id, subject);
    // Who introduced an unexplained value is who to ask.
    if b.out.objects[idx].properties.get("gap_type") == Some(&json!("unexplained_value")) {
        blame_into(b, rationale, explained, id, &key, &used);
    }
    b.out.stats.gaps += 1;
}

/// The gap of a glossary term no recovered concept, code or table matches.
fn emit_term_gap(b: &mut Builder, g: &GlossEntry) {
    let key = format!("semantic-gap:term:{}:{}", g.path, words_key(&g.term));
    let idx = b.object(
        &key,
        format!("glossary term “{}”: no trace in code", g.term),
        GAP,
        vec![
            ("gap_type", json!("unmapped_term")),
            ("term", json!(g.term)),
            ("definition", json!(g.definition)),
            (
                "question",
                json!(format!(
                    "The glossary defines “{}” ({}), but no recovered concept, code or table \
                     matches it. Which code implements it — or is the definition only on paper?",
                    g.term, g.definition
                )),
            ),
        ],
    );
    let ev = b.evidence(
        &key,
        0,
        &g.path,
        (g.line > 0).then_some(g.line),
        format!("glossary: {} — {}", g.term, g.definition),
    );
    b.out.objects[idx].evidence.push(ev);
    let (id, owner) = (b.out.objects[idx].id, g.owner);
    b.relate(DESCRIBES, id, owner);
    b.out.stats.gaps += 1;
}

// ── Finishing ───────────────────────────────────────────────────────────────────────────────

/// The name an ontology term is matched against: a concept's name, a code's label.
fn mapping_name(o: &KirObject) -> Option<String> {
    match kind_name(o) {
        Some(k) if k == CONCEPT => Some(o.name.clone()),
        Some(k) if k == ENUM_MEANING => o
            .properties
            .get("label")
            .and_then(Value::as_str)
            .map(str::to_string),
        _ => None,
    }
}

/// RFC 0170 Phase 4: ontology mapping suggestions — a concept by its name, a code by its label.
fn suggest_mappings(out: &mut SemanticsOutput, cfg: &SemanticsConfig) {
    if cfg.ontology.terms.is_empty() {
        return;
    }
    for o in &mut out.objects {
        let Some(name) = mapping_name(o) else {
            continue;
        };
        let suggestions = cfg.ontology.suggest(&name);
        if !suggestions.is_empty() {
            o.properties
                .insert("mapping_suggestions".into(), json!(suggestions));
            out.stats.mapping_suggestions += suggestions.len();
        }
    }
}

/// RFC 0170 Phase 2: what each item asserts and rests on, for the review lifecycle.
fn sign_items(out: &mut SemanticsOutput) {
    let ev_by_id: HashMap<Uuid, &KirEvidence> = out.evidence.iter().map(|e| (e.id.0, e)).collect();
    let sigs: Vec<String> = out
        .objects
        .iter()
        .map(|o| {
            let evs: Vec<&KirEvidence> = o
                .evidence
                .iter()
                .filter_map(|id| ev_by_id.get(&id.0).copied())
                .collect();
            crate::semantics_review::signature(o, &evs)
        })
        .collect();
    for (o, sig) in out.objects.iter_mut().zip(sigs) {
        o.properties.insert("signature".into(), json!(sig));
    }
}

/// Every link this layer makes joins one of its own items to something else, and rests on what
/// that item rests on: copy the item's evidence onto the link (issue #12). The `from` end wins
/// when both ends are items (`ConflictsWith`).
fn cite_endpoint_evidence(out: &mut SemanticsOutput) {
    let evidence: HashMap<KirId, &Vec<KirId>> =
        out.objects.iter().map(|o| (o.id, &o.evidence)).collect();
    for r in out
        .relationships
        .iter_mut()
        .filter(|r| r.evidence.is_empty())
    {
        if let Some(ev) = evidence
            .get(&r.from)
            .filter(|e| !e.is_empty())
            .or_else(|| evidence.get(&r.to))
        {
            r.evidence = (*ev).clone();
        }
    }
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

    /// Issue #12: every link this layer makes cites evidence (its item's).
    #[test]
    fn every_link_cites_evidence() {
        let out = synthesize(&fixture(), &SemanticsConfig::default(), Some(&Fixed));
        assert!(!out.relationships.is_empty());
        for r in &out.relationships {
            assert!(!r.evidence.is_empty(), "{} has no evidence", r.kind);
        }
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

    #[test]
    fn the_same_column_with_different_thresholds_is_a_conflict_not_two_facts() {
        let mut g = fixture();
        g.objects.retain(|o| o.name != "parts");
        g.add_object(table(
            "parts",
            json!([{"name": "obsolete", "data_type": "BOOLEAN"}, {"name": "age", "data_type": "INT"}]),
            vec![],
        ));
        for (n, v) in [("a", "90"), ("b", "90"), ("c", "60"), ("d", "60")] {
            g.add_object(carrier(
                "ProcedureStatement",
                &format!("{n}#1"),
                json!([site("parts", "age", ">", &[v], "where", 1)]),
            ));
        }
        let out = synthesize(&g, &SemanticsConfig::default(), None);
        let c = of(&out, CONFLICT);
        assert_eq!(
            c.len(),
            1,
            "{:?}",
            c.iter().map(|x| &x.name).collect::<Vec<_>>()
        );
        assert_eq!(prop(c[0], "conflict_type"), &json!("threshold"));
        assert_eq!(
            out.relationships
                .iter()
                .filter(|r| r.kind == RelationshipKind::Custom(CONFLICTS_WITH.into()))
                .count(),
            2
        );
        assert!(
            out.objects
                .iter()
                .all(|o| o.properties.contains_key("signature"))
        );
    }

    #[test]
    fn a_trigger_condition_on_new_resolves_to_the_trigger_table() {
        let mut g = fixture();
        let mut trig = KirObject::new("trg_ar", ObjectKind::Custom("Trigger".into()));
        trig.properties
            .insert("function".into(), json!("public.check_ar"));
        trig.properties
            .insert("table".into(), json!("entity_credit_account"));
        g.add_object(trig);
        for n in 1..=2 {
            let mut c = carrier(
                "ProcedureStatement",
                &format!("check_ar#{n}"),
                json!([site("$new", "entity_class", "in", &["2"], "condition", 5)]),
            );
            c.properties.insert("procedure".into(), json!("check_ar"));
            g.add_object(c);
        }
        let out = synthesize(&g, &SemanticsConfig::default(), None);
        let e = of(&out, ENUM_MEANING)
            .into_iter()
            .find(|o| o.name == "entity_credit_account.entity_class = 2")
            .unwrap();
        // 2 WHERE/JOIN sites from the fixture + 2 trigger conditions.
        assert_eq!(prop(e, "usage_sites"), &json!(4));
    }

    #[test]
    fn dbt_tests_declare_domains_references_and_constraints() {
        let mut g = fixture();
        g.add_object(table(
            "orders",
            json!([
                {"name": "status", "description": "1=open, 2=closed", "description_path": "m/schema.yml",
                 "description_line": 7, "accepted_values": ["1", "2"], "dbt_tests": ["accepted_values", "not_null"],
                 "not_null": true, "dbt_path": "m/schema.yml", "dbt_line": 7},
                {"name": "kind_id", "references": {"table": "entity_class", "column": "id"},
                 "dbt_tests": ["relationships"], "dbt_path": "m/schema.yml", "dbt_line": 12}
            ]),
            vec![],
        ));
        let out = synthesize(&g, &SemanticsConfig::default(), None);
        let label = |n: &str| {
            of(&out, ENUM_MEANING)
                .into_iter()
                .find(|o| o.name == n)
                .map(|o| prop(o, "label").clone())
        };
        assert_eq!(label("orders.status = 1"), Some(json!("open")));
        assert_eq!(
            label("orders.kind_id = 2"),
            Some(json!("Customer")),
            "relationships → seeded lookup"
        );
        let types: BTreeSet<String> = of(&out, CONSTRAINT)
            .iter()
            .filter(|c| prop(c, "source") == &json!("dbt_test"))
            .map(|c| prop(c, "constraint_type").as_str().unwrap().to_string())
            .collect();
        assert_eq!(
            types,
            BTreeSet::from(["enum".into(), "not_null".into(), "relationship".into()])
        );
    }

    #[test]
    fn a_dbt_model_defines_a_concept_and_its_vars_label_codes() {
        let mut g = fixture();
        g.add_object(table(
            "accounts",
            json!([{"name": "account_number", "data_type": "TEXT"}]),
            vec![],
        ));
        let mut m = KirObject::new("mart_ar_aging", ObjectKind::Table);
        m.id = kid("m:ar");
        for (k, v) in [
            ("dbt_kind", json!("model")),
            ("source_path", json!("models/mart_ar_aging.sql")),
            ("description", json!("Receivables by age bucket.")),
            ("dbt_var_values", json!({"'1200'": "acc_ar"})),
            (
                "predicates",
                json!([site(
                    "accounts",
                    "account_number",
                    "in",
                    &["'1200'"],
                    "where",
                    9
                )]),
            ),
        ] {
            m.properties.insert(k.into(), v);
        }
        g.add_object(m);
        let out = synthesize(&g, &SemanticsConfig::default(), None);
        let c = of(&out, CONCEPT)
            .into_iter()
            .find(|c| c.name == "MartArAging")
            .expect("the model's filter is a concept named after it");
        assert_eq!(prop(c, "origin"), &json!("view"));
        assert_eq!(prop(c, "description"), &json!("Receivables by age bucket."));
        let e = of(&out, ENUM_MEANING)
            .into_iter()
            .find(|o| o.name == "accounts.account_number = '1200'")
            .unwrap();
        assert_eq!(prop(e, "label"), &json!("acc_ar"));
    }

    #[test]
    fn application_constants_name_codes_by_label_agreement_or_initials() {
        let mut g = fixture();
        let mut magic =
            KirObject::new("LedgerSMB::Magic", ObjectKind::Custom("PerlPackage".into()));
        magic
            .properties
            .insert("constants_path".into(), json!("lib/LedgerSMB/Magic.pm"));
        magic.properties.insert(
            "constants".into(),
            json!([
                {"name": "EC_VENDOR", "value": "1", "line": 10},
                {"name": "EC_CUSTOMER", "value": "2", "line": 11},
                {"name": "EC_EMPLOYEE", "value": "3", "line": 12},
                {"name": "XY_ONE", "value": "1", "line": 20},
                {"name": "XY_TWO", "value": "2", "line": 21}
            ]),
        );
        g.add_object(magic);
        let out = synthesize(&g, &SemanticsConfig::default(), None);
        let e = of(&out, ENUM_MEANING)
            .into_iter()
            .find(|o| o.name == "entity_credit_account.entity_class = 2")
            .unwrap();
        let sources: Vec<&str> = prop(e, "meanings")
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["source"].as_str().unwrap())
            .collect();
        // Seeds say 1=Vendor, 2=Customer; EC_VENDOR/EC_CUSTOMER agree — the group matches by labels.
        assert!(sources.contains(&"app_constant"), "{sources:?}");
        assert!(sources.contains(&"lookup_seed"));
        // XY_* agrees with nothing and is not the column's initials: no match anywhere.
        assert!(of(&out, ENUM_MEANING).iter().all(|o| {
            prop(o, "meanings")
                .as_array()
                .unwrap()
                .iter()
                .all(|m| m["label"] != json!("one"))
        }));
    }

    #[test]
    fn glossary_terms_document_matching_items_and_unmatched_terms_are_gaps() {
        assert_eq!(words_key("OpenOrders"), "open order");
        assert_eq!(words_key("open_orders"), "open order");
        assert_eq!(words_key("Open orders"), "open order");
        let mut g = fixture();
        let mut sec = KirObject::new(
            "docs/glossary.md § Terms",
            ObjectKind::Custom("Section".into()),
        );
        sec.properties
            .insert("source_path".into(), json!("docs/glossary.md"));
        sec.properties.insert(
            "glossary".into(),
            json!([
                {"term": "Active parts", "definition": "Parts we still sell.", "line": 4},
                {"term": "Customer", "definition": "An entity that buys from us.", "line": 5},
                {"term": "Dunning level", "definition": "How many reminders were sent.", "line": 6}
            ]),
        );
        g.add_object(sec);
        let out = synthesize(&g, &SemanticsConfig::default(), None);
        // The view concept "ActiveParts" already had its view comment; the term attaches anyway.
        let c = of(&out, CONCEPT)
            .into_iter()
            .find(|c| c.name == "ActiveParts")
            .unwrap();
        assert_eq!(prop(c, "glossary")[0]["term"], json!("Active parts"));
        // "Customer" is the label of entity_class = 2.
        let e = of(&out, ENUM_MEANING)
            .into_iter()
            .find(|o| o.name == "entity_credit_account.entity_class = 2")
            .unwrap();
        assert_eq!(
            prop(e, "glossary")[0]["definition"],
            json!("An entity that buys from us.")
        );
        let gaps: Vec<String> = of(&out, GAP)
            .iter()
            .filter(|g| prop(g, "gap_type") == &json!("unmapped_term"))
            .map(|g| prop(g, "term").as_str().unwrap().to_string())
            .collect();
        assert_eq!(gaps, vec!["Dunning level"]);

        // A second run over a ledger that now also holds the first run's items (which carry
        // `glossary`) must produce exactly the same items — no re-import of our own output.
        let mut again = g.clone();
        again.objects.extend(out.objects.iter().cloned());
        let out2 = synthesize(&again, &SemanticsConfig::default(), None);
        let e2 = of(&out2, ENUM_MEANING)
            .into_iter()
            .find(|o| o.name == "entity_credit_account.entity_class = 2")
            .unwrap();
        assert_eq!(prop(e2, "glossary").as_array().unwrap().len(), 1);
    }

    /// RFC 0246: an Apache Ossie model is a meaning source — a field description's legend gives
    /// code meanings, a metric's description documents the concept of the same words, and a
    /// metric that matches nothing is no gap.
    #[test]
    fn rfc0246_ossie_descriptions_are_a_meaning_source() {
        let mut g = fixture();
        // The account table loses its comment; the semantic model states the legend instead.
        let account = g.objects.iter_mut().find(|o| o.name == "account").unwrap();
        account.properties.insert(
            "columns".into(),
            json!([{"name": "category", "data_type": "CHAR(1)"}]),
        );
        let account_id = account.id;
        let mut ds = KirObject::new("account", ObjectKind::Custom("OssieDataset".into()));
        ds.properties
            .insert("path".into(), json!("semantic/finance.ossie.yaml"));
        ds.properties.insert("model".into(), json!("finance"));
        ds.properties.insert(
            "fields".into(),
            json!([{"name": "category", "sql": "category",
                    "description": "A = asset, L = liability, Q = equity"}]),
        );
        let mut link = KirRelationship::new(RelationshipKind::DependsOn, ds.id, account_id);
        link.properties.insert("link".into(), json!("ossie_source"));
        let mut metric = KirObject::new("active_parts", ObjectKind::Custom("OssieMetric".into()));
        metric
            .properties
            .insert("path".into(), json!("semantic/finance.ossie.yaml"));
        metric
            .properties
            .insert("description".into(), json!("Parts still on sale."));
        let mut other = KirObject::new("revenue", ObjectKind::Custom("OssieMetric".into()));
        other
            .properties
            .insert("path".into(), json!("semantic/finance.ossie.yaml"));
        other
            .properties
            .insert("description".into(), json!("Money in."));
        g.add_object(ds);
        g.add_object(metric);
        g.add_object(other);
        g.add_relationship(link);
        let out = synthesize(&g, &SemanticsConfig::default(), None);

        let a = of(&out, ENUM_MEANING)
            .into_iter()
            .find(|o| o.name == "account.category = 'A'")
            .unwrap_or_else(|| panic!("{:#?}", of(&out, ENUM_MEANING)));
        assert_eq!(prop(a, "label"), &json!("asset"));
        assert_eq!(prop(a, "meanings")[0]["source"], json!("ossie"));
        assert_eq!(
            prop(a, "meanings")[0]["path"],
            json!("semantic/finance.ossie.yaml")
        );
        let c = of(&out, CONCEPT)
            .into_iter()
            .find(|c| c.name == "ActiveParts")
            .unwrap();
        assert_eq!(
            prop(c, "glossary")[0]["definition"],
            json!("Parts still on sale.")
        );
        assert!(
            of(&out, GAP)
                .iter()
                .all(|g| prop(g, "term") != &json!("revenue")),
            "an Ossie metric that matches nothing is no gap"
        );
    }

    #[test]
    fn ontology_suggestions_come_only_from_the_users_vocabulary() {
        let none = synthesize(&fixture(), &SemanticsConfig::default(), None);
        assert!(
            none.objects
                .iter()
                .all(|o| !o.properties.contains_key("mapping_suggestions"))
        );
        let cfg = SemanticsConfig {
            ontology: crate::ontology::Vocabulary {
                prefixes: Default::default(),
                terms: vec![crate::ontology::OntologyTerm {
                    id: "schema:Customer".into(),
                    label: "Customer".into(),
                    synonyms: vec![],
                }],
            },
            ..Default::default()
        };
        let out = synthesize(&fixture(), &cfg, None);
        let e = of(&out, ENUM_MEANING)
            .into_iter()
            .find(|o| o.name == "entity_credit_account.entity_class = 2")
            .unwrap();
        assert_eq!(
            prop(e, "mapping_suggestions")[0]["id"],
            json!("schema:Customer")
        );
        assert_eq!(out.stats.mapping_suggestions, 1);
    }

    #[test]
    fn a_python_enum_named_like_the_column_names_its_codes() {
        let mut g = fixture();
        g.add_object(table(
            "orders",
            json!([{"name": "status_id", "data_type": "INT"}]),
            vec![],
        ));
        g.add_object(carrier(
            "ProcedureStatement",
            "order_report#1",
            json!([site("orders", "status_id", "in", &["1", "3"], "where", 4)]),
        ));
        let mut cls = KirObject::new("Status", ObjectKind::Custom("PythonSymbol".into()));
        cls.properties
            .insert("constants_path".into(), json!("app/models.py"));
        cls.properties.insert(
            "constants".into(),
            json!([
                {"name": "OPEN", "value": "1", "line": 7, "group": "Status"},
                {"name": "SHIPPED", "value": "3", "line": 8, "group": "Status"}
            ]),
        );
        g.add_object(cls);
        let out = synthesize(&g, &SemanticsConfig::default(), None);
        let label = |n: &str| {
            of(&out, ENUM_MEANING)
                .into_iter()
                .find(|o| o.name == n)
                .map(|o| prop(o, "label").clone())
        };
        assert_eq!(label("orders.status_id = 3"), Some(json!("shipped")));
    }

    #[test]
    fn a_filter_on_a_dbt_model_column_lands_on_the_source_column() {
        let mut g = fixture();
        let mut stg = KirObject::new("stg_orders", ObjectKind::Table);
        stg.id = kid("t:stg");
        stg.properties.insert("dbt_kind".into(), json!("model"));
        stg.properties
            .insert("columns".into(), json!([{"name": "order_id"}]));
        stg.properties.insert(
            "column_lineage".into(),
            json!({"is_obsolete": ["parts", "obsolete"]}),
        );
        g.add_object(stg);
        for n in ["mart_a", "mart_b"] {
            g.add_object(carrier(
                "ProcedureStatement",
                &format!("{n}#1"),
                json!([site(
                    "stg_orders",
                    "is_obsolete",
                    "is_false",
                    &[],
                    "where",
                    3
                )]),
            ));
        }
        let out = synthesize(&g, &SemanticsConfig::default(), None);
        let c = of(&out, CONCEPT)
            .into_iter()
            .find(|c| c.name == "ActiveParts")
            .unwrap();
        assert_eq!(
            prop(c, "sites"),
            &json!(4),
            "the view, the routine, and two marts via lineage"
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

    /// A ledger returns objects in any order; the output must not change with it.
    #[test]
    fn synthesis_does_not_depend_on_object_order() {
        let a = synthesize(&fixture(), &SemanticsConfig::default(), Some(&Fixed));
        let mut g = fixture();
        g.objects.reverse();
        g.relationships.reverse();
        let b = synthesize(&g, &SemanticsConfig::default(), Some(&Fixed));
        let norm = |o: &SemanticsOutput| {
            let mut v: Vec<_> = o
                .objects
                .iter()
                .map(|x| {
                    (
                        x.id.0,
                        x.evidence.clone(),
                        serde_json::to_string(&x.properties.iter().collect::<BTreeMap<_, _>>())
                            .unwrap(),
                    )
                })
                .collect();
            v.sort_by_key(|x| x.0);
            v
        };
        assert_eq!(norm(&a), norm(&b));
        let ev = |o: &SemanticsOutput| {
            let mut v: Vec<_> = o
                .evidence
                .iter()
                .map(|e| (e.id.0, e.fragment.clone()))
                .collect();
            v.sort();
            v
        };
        assert_eq!(ev(&a), ev(&b));
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
            comment_legend("A asset, L liability, Q equity, I income, E expense."),
            BTreeMap::from([
                ("A".into(), "asset".into()),
                ("E".into(), "expense".into()),
                ("I".into(), "income".into()),
                ("L".into(), "liability".into()),
                ("Q".into(), "equity".into()),
            ])
        );
        // Prose with a short capitalised word is not a legend.
        assert_eq!(
            comment_legend("The account number from the chart (e.g. 1200 Accounts).").len(),
            0
        );
        assert_eq!(comment_legend("AR and AP, plus other ledgers").len(), 0);
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
