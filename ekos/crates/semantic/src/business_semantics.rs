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
}

impl Default for SemanticsConfig {
    fn default() -> Self {
        Self {
            min_sites: 2,
            max_enum_values: 12,
            ontology: Default::default(),
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
    pub conflicts: usize,
    pub rationale_links: usize,
    pub mapping_suggestions: usize,
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
            });
        }
    }
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
    let mut sites = collect_sites(graph, &tables, &index, &by_id, &mut stats);
    // A ledger hands objects back in no particular order. Everything downstream — which sites an
    // item cites when it has more than `MAX_EVIDENCE`, evidence ids by index, signatures — must not
    // depend on it, or every commit rewrites the busiest items.
    sites.sort_by(|a, b| {
        (&a.path, a.p.line, &a.carrier_name, a.carrier.0, &a.p).cmp(&(
            &b.path,
            b.p.line,
            &b.carrier_name,
            b.carrier.0,
            &b.p,
        ))
    });
    let mut fks = foreign_keys(graph, &by_id);
    // A dbt `relationships` test declares the same thing a foreign key does (RFC 0170 Phase 3).
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

    // A dbt `accepted_values` test declares a domain the same way.
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

    // Application constants grouped by prefix: `BC_AP => 1, BC_AR => 2` is the group `BC`.
    let groups = constant_groups(graph);

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
        // Application constants (`use constant EC_CUSTOMER => 2`): a group whose names agree with
        // this column's seeded labels, or whose prefix is the column's initials and whose values
        // overlap its codes, names those codes.
        {
            let mut known_values: BTreeSet<String> = meanings.keys().cloned().collect();
            if let Some(m) = coded.get(&(*ti, col.clone())) {
                known_values.extend(m.keys().cloned());
            }
            if let Some(d) = check_domain.get(&(*ti, col.clone())) {
                known_values.extend(d.iter().cloned());
            }
            let labels_now: BTreeMap<String, String> = meanings
                .iter()
                .filter_map(|(v, ms)| ms.first().map(|m| (v.clone(), m.label.clone())))
                .collect();
            if let Some((prefix, how)) =
                match_constant_group(&groups, col, &known_values, &labels_now)
            {
                for c in &groups[&prefix] {
                    meanings.entry(c.value.clone()).or_default().push(Meaning {
                        label: c.suffix.clone(),
                        source: "app_constant",
                        confidence: match how {
                            "labels" => 0.6,
                            "name" => 0.5,
                            _ => 0.4,
                        },
                        path: c.path.clone(),
                        line: Some(c.line),
                        detail: format!(
                            "use constant {} => {} ({}; matched by {how})",
                            c.name, c.value, c.package
                        ),
                    });
                }
            }
        }
        // dbt var names: `account_number = '{{ var("acc_ar") }}'` says '1200' is the AR account.
        if let Some(vals) = coded.get(&(*ti, col.clone())) {
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
                                s.canonical(&tables),
                                s.carrier_name
                            ),
                        });
                    }
                }
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
    let filter_clause = |c: Clause| {
        matches!(
            c,
            Clause::Where | Clause::Having | Clause::JoinOn | Clause::Condition
        )
    };
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
        .filter(|o| is_custom(o, "View") || is_dbt_model(o))
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

    // ── ConceptConflict ─────────────────────────────────────────────────────────────────────
    // Two kinds, both narrow on purpose. `IN` sets on one column that merely differ are usually
    // different concepts (asset vs income accounts), not a disagreement, so they are not flagged.
    //  - threshold: one column, the same comparison direction, different literals — "overdue" as
    //    `> 90` in one place and `> 60` in another;
    //  - name: one name for different definitions.
    let mut conflicts: BTreeMap<String, (&'static str, Vec<ConflictMember>)> = BTreeMap::new();
    for (def, c) in &concepts {
        let id = kid(&format!("business-concept:{def}"));
        if c.atoms.len() == 1
            && let Some(s0) = c.sites.first()
            && s0.p.values.len() == 1
        {
            let dir = match s0.p.op.as_str() {
                ">" | ">=" => Some("above"),
                "<" | "<=" => Some("below"),
                _ => None,
            };
            if let Some(dir) = dir {
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
        }
        conflicts
            .entry(format!("name:{}", c.name))
            .or_insert(("name", Vec::new()))
            .1
            .push((id, c.name.clone(), def.clone()));
    }
    for (key, (conflict_type, members)) in conflicts {
        if members.len() < 2 {
            continue;
        }
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
        for (cid, _, _) in &members {
            b.relate(CONFLICTS_WITH, id, *cid);
        }
        stats.conflicts += 1;
    }

    // ── ConstraintCandidate per dbt test (RFC 0170 Phase 3) ────────────────────────────────
    for t in &tables {
        for c in raw_columns(t.obj) {
            let Some(col) = c["name"].as_str().map(str::to_lowercase) else {
                continue;
            };
            let tests: Vec<&str> = c["dbt_tests"]
                .as_array()
                .map(|a| a.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            for test in tests {
                let site = |op: &str, values: Vec<String>| {
                    serde_json::to_value(PredicateSite {
                        relation: Some(t.name()),
                        scope: Vec::new(),
                        column: col.clone(),
                        op: op.into(),
                        values,
                        clause: Clause::Check,
                        top_level: true,
                        subquery: false,
                        label: None,
                        line: 0,
                    })
                    .unwrap_or_default()
                };
                let (constraint_type, expression, structured) = match test {
                    "not_null" => (
                        "not_null",
                        format!("{col} IS NOT NULL"),
                        vec![site("is_not_null", vec![])],
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
                            canonical_text(&col, "in", &vals),
                            vec![site("in", vals)],
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
                    _ => continue,
                };
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
                        ("columns", json!([col.clone()])),
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
                stats.constraints += 1;
            }
        }
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

    // ── Glossary (RFC 0170 Phase 3) ─────────────────────────────────────────────────────────
    // A glossary term attaches to a concept or a code label with exactly the same words; one that
    // matches nothing recovered — no concept, code or table — is a reverse gap: written down, but
    // no trace in code.
    let glossary = glossary_entries(graph);
    let table_words: BTreeSet<String> = tables.iter().map(|t| words_key(&t.name())).collect();
    let mut glossed: BTreeSet<Uuid> = BTreeSet::new();
    let mut unmatched: Vec<&GlossEntry> = Vec::new();
    for (gi, g) in glossary.iter().enumerate() {
        let key = words_key(&g.term);
        if key.is_empty() {
            continue;
        }
        let mut hit = table_words.contains(&key);
        for idx in 0..b.out.objects.len() {
            let o = &b.out.objects[idx];
            let target = match kind_name(o) {
                Some(k) if k == CONCEPT => words_key(&o.name),
                Some(k) if k == ENUM_MEANING => o
                    .properties
                    .get("label")
                    .and_then(Value::as_str)
                    .map(words_key)
                    .unwrap_or_default(),
                _ => continue,
            };
            if target != key {
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
            let entry =
                json!({"term": g.term, "definition": g.definition, "path": g.path, "line": g.line});
            match o.properties.get_mut("glossary") {
                Some(Value::Array(a)) => a.push(entry),
                _ => {
                    o.properties.insert("glossary".into(), json!([entry]));
                }
            }
            if kind_name(o) == Some(CONCEPT)
                && o.properties.get("description").is_none_or(Value::is_null)
            {
                o.properties
                    .insert("description".into(), json!(g.definition));
                o.properties
                    .insert("description_source".into(), json!("glossary"));
            }
            glossed.insert(o.id.0);
        }
        if !hit {
            unmatched.push(g);
        }
    }
    undocumented.retain(|(id, ..)| !glossed.contains(&id.0));

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

    for g in unmatched {
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
        stats.gaps += 1;
    }

    stats.rationale_links = b
        .out
        .objects
        .iter()
        .filter(|o| kind_name(o) == Some(RATIONALE))
        .count();
    // RFC 0170 Phase 4: ontology mapping suggestions — a concept by its name, a code by its label.
    if !cfg.ontology.terms.is_empty() {
        for o in &mut b.out.objects {
            let name = match kind_name(o) {
                Some(k) if k == CONCEPT => o.name.clone(),
                Some(k) if k == ENUM_MEANING => {
                    match o.properties.get("label").and_then(Value::as_str) {
                        Some(l) => l.to_string(),
                        None => continue,
                    }
                }
                _ => continue,
            };
            let suggestions = cfg.ontology.suggest(&name);
            if !suggestions.is_empty() {
                o.properties
                    .insert("mapping_suggestions".into(), json!(suggestions));
                stats.mapping_suggestions += suggestions.len();
            }
        }
    }

    // RFC 0170 Phase 2: what each item asserts and rests on, for the review lifecycle.
    let ev_by_id: HashMap<Uuid, &KirEvidence> =
        b.out.evidence.iter().map(|e| (e.id.0, e)).collect();
    let sigs: Vec<String> = b
        .out
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
    for (o, sig) in b.out.objects.iter_mut().zip(sigs) {
        o.properties.insert("signature".into(), json!(sig));
    }
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
