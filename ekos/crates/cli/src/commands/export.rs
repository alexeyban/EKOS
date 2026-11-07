//! `ekos export linkml` — RFC 0170: the business-semantics hypotheses as a draft LinkML schema.
//!
//! EKOS feeds LinkML; it does not replace it. This writes one schema file and stops: JSON Schema,
//! DDL, Pydantic, RDF/OWL/SHACL and data validation are LinkML tooling's job.
//!
//! | EKOS | LinkML |
//! |---|---|
//! | `Table` touched by an exported item | `class` with `attributes` (range from the SQL type, `required` from NOT NULL, `identifier` from a single-column primary key) |
//! | `BusinessConcept` | `class`, `is_a` its table's class, `description` = the predicate |
//! | `EnumMeaning`s of one column | `enum`, `permissible_values` keyed by the code |
//! | `ConstraintCandidate` | `slot_usage` (`minimum_value`/`maximum_value`/`pattern`/`range`) |
//! | status, confidence, evidence, rationale, gaps | `annotations.ekos_*` + `comments` |
//!
//! Hypotheses are never exported as facts by accident: the default `--status confirmed` leaves them
//! out, and every element that is exported carries its `ekos_status`.

use super::semantics::current_items;
use anyhow::{Context, Result};
use ekos_compiler_core::EkosConfig;
use ekos_kir::{KirObject, ObjectKind, RelationshipKind};
use ekos_ledger::KnowledgeStore;
use ekos_semantic::business_semantics::{
    CONCEPT, CONFLICT, CONSTRAINT, ENUM_MEANING, EXPLAINED_BY, GAP, RATIONALE, camel, kind_name,
};
use serde_json::Value as Json;
use serde_yaml::{Mapping, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

/// Which statuses an export includes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum StatusFilter {
    /// Confirmed by a human (RFC 0170 Phase 2). The default: nothing unreviewed by accident.
    Confirmed,
    /// Unreviewed hypotheses only.
    Hypothesis,
    /// Everything, each element annotated with its status.
    All,
}

impl StatusFilter {
    /// Whether an item with this review status is exported.
    pub fn admits(self, status: &str) -> bool {
        match self {
            StatusFilter::Confirmed => status == "confirmed",
            StatusFilter::Hypothesis => status == "hypothesis",
            StatusFilter::All => status != "rejected",
        }
    }

    /// The filter's name as written on the command line.
    pub fn as_str(self) -> &'static str {
        match self {
            StatusFilter::Confirmed => "confirmed",
            StatusFilter::Hypothesis => "hypothesis",
            StatusFilter::All => "all",
        }
    }

    /// The filter a name (as [`as_str`](Self::as_str) writes it) stands for.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "confirmed" => Some(StatusFilter::Confirmed),
            "hypothesis" => Some(StatusFilter::Hypothesis),
            "all" => Some(StatusFilter::All),
            _ => None,
        }
    }
}

fn kind(o: &KirObject) -> &'static str {
    kind_name(o).unwrap_or_default()
}

fn prop<'a>(o: &'a KirObject, k: &str) -> &'a Json {
    o.properties.get(k).unwrap_or(&Json::Null)
}

fn text(o: &KirObject, k: &str) -> String {
    match prop(o, k) {
        Json::String(s) => s.clone(),
        Json::Null => String::new(),
        v => v.to_string(),
    }
}

/// `ekos_suggested_exact_mappings` / `ekos_suggested_close_mappings` from suggestions — never
/// LinkML's own `exact_mappings`: a suggestion is not a decision.
fn suggestion_annotations(suggestions: &[(String, String)]) -> Vec<(&'static str, Value)> {
    let of = |t: &str| {
        let ids: Vec<&str> = suggestions
            .iter()
            .filter(|(_, m)| m == t)
            .map(|(id, _)| id.as_str())
            .collect();
        if ids.is_empty() {
            Value::Null
        } else {
            s(ids.join(" "))
        }
    };
    vec![
        ("ekos_suggested_exact_mappings", of("exact")),
        ("ekos_suggested_close_mappings", of("close")),
    ]
}

fn item_suggestions(o: &KirObject) -> Vec<(String, String)> {
    prop(o, "mapping_suggestions")
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| {
                    Some((
                        x["id"].as_str()?.to_string(),
                        x["match_type"].as_str()?.to_string(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// A string property as a YAML value, or nothing when absent.
fn opt(o: &KirObject, k: &str) -> Value {
    match text(o, k) {
        t if t.is_empty() => Value::Null,
        t => Value::String(t),
    }
}

fn s(v: impl Into<String>) -> Value {
    Value::String(v.into())
}

fn map(pairs: Vec<(&str, Value)>) -> Value {
    let mut m = Mapping::new();
    for (k, v) in pairs {
        if !matches!(v, Value::Null) {
            m.insert(s(k), v);
        }
    }
    Value::Mapping(m)
}

/// The LinkML type of a SQL type.
pub fn linkml_range(sql_type: &str) -> &'static str {
    let t = sql_type.to_ascii_lowercase();
    if t.starts_with("bool") {
        "boolean"
    } else if t.contains("int") || t.contains("serial") {
        "integer"
    } else if t.contains("numeric") || t.contains("decimal") || t.contains("money") {
        "decimal"
    } else if t.contains("double") || t.contains("real") || t.contains("float") {
        "float"
    } else if t.contains("timestamp") || t.contains("datetime") {
        "datetime"
    } else if t == "date" || t.starts_with("date ") {
        "date"
    } else if t.starts_with("time") {
        "time"
    } else {
        "string"
    }
}

/// A SQL `LIKE` pattern as an anchored regular expression.
pub fn like_to_regex(pattern: &str) -> String {
    let mut out = String::from("^");
    for c in pattern.chars() {
        match c {
            '%' => out.push_str(".*"),
            '_' => out.push('.'),
            c if "\\.^$|?*+()[]{}".contains(c) => {
                out.push('\\');
                out.push(c);
            }
            c => out.push(c),
        }
    }
    out.push('$');
    out
}

fn unquote(v: &str) -> String {
    v.strip_prefix('\'')
        .and_then(|r| r.strip_suffix('\''))
        .map(|r| r.replace("''", "'"))
        .unwrap_or_else(|| v.to_string())
}

/// A name not yet used, by suffixing.
fn unique(name: String, used: &mut BTreeSet<String>, suffix: &str) -> String {
    let name = if name.is_empty() {
        "Unnamed".into()
    } else {
        name
    };
    // LinkML element names must not start with a digit.
    let name = if name.starts_with(|c: char| c.is_ascii_digit()) {
        format!("N{name}")
    } else {
        name
    };
    if used.insert(name.clone()) {
        return name;
    }
    let with = format!("{name}{suffix}");
    if used.insert(with.clone()) {
        return with;
    }
    (2..)
        .map(|n| format!("{with}{n}"))
        .find(|c| used.insert(c.clone()))
        .unwrap()
}

/// Compact `path:line` evidence references for an item, de-duplicated, at most `max`.
fn evidence_refs(ledger: &dyn KnowledgeStore, o: &KirObject, max: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for id in &o.evidence {
        if let Ok(Some(e)) = ledger.get_evidence(id) {
            let r = match e.location.line {
                Some(l) => format!("{}:{l}", e.location.path),
                None => e.location.path.clone(),
            };
            if !r.is_empty() && !out.contains(&r) {
                out.push(r);
            }
        }
        if out.len() >= max {
            break;
        }
    }
    out
}

/// Options for one export.
pub struct LinkmlOptions {
    pub status: StatusFilter,
    pub out: Option<PathBuf>,
    pub name: Option<String>,
    /// Print the schema as JSON instead of YAML (for the web console's viewer).
    pub json: bool,
}

/// The schema name when none is given: the workspace directory's name, as an identifier.
pub fn default_schema_name(cwd: &Path) -> String {
    let base = cwd
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "workspace".into());
    base.to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

/// `ekos export linkml`.
pub fn linkml(config: &EkosConfig, cwd: &Path, opts: &LinkmlOptions) -> Result<()> {
    let (ledger, items) = current_items(config, cwd)?;
    if items.is_empty() {
        anyhow::bail!(
            "no business-semantics items in the ledger — set `[semantics] enabled = true` in \
             ekos.toml and re-run `ekos commit` (RFC 0170)"
        );
    }
    let name = opts
        .name
        .clone()
        .unwrap_or_else(|| default_schema_name(cwd));
    let vocab = super::semantics::load_vocabulary(config, cwd)?;
    let schema = build_schema(&*ledger, &items, opts.status, &name, &vocab)?;
    let Some(schema) = schema else {
        let counts: BTreeMap<String, usize> = items.iter().fold(BTreeMap::new(), |mut m, o| {
            *m.entry(text(o, "status")).or_default() += 1;
            m
        });
        anyhow::bail!(
            "nothing to export with --status {:?}: the ledger holds {} — RFC 0170 Phase 1 only \
             produces hypotheses. Pass `--status hypothesis` (or `all`) for a draft for review.",
            opts.status,
            counts
                .iter()
                .map(|(k, v)| format!("{v} {k}"))
                .collect::<Vec<_>>()
                .join(", ")
        );
    };
    if opts.json && opts.out.is_none() {
        println!("{}", serde_json::to_string(&schema)?);
        return Ok(());
    }
    let yaml = format!(
        "# Draft LinkML schema generated by EKOS (RFC 0170) from technical traces.\n\
         # Every element carries `ekos_status`; a `hypothesis` is NOT a confirmed definition.\n\
         # Validate with `linkml-lint`; generate artifacts with LinkML's own generators.\n{}",
        serde_yaml::to_string(&schema)?
    );
    match &opts.out {
        Some(path) => {
            std::fs::write(path, &yaml).with_context(|| format!("writing {}", path.display()))?;
            let classes = schema["classes"].as_mapping().map_or(0, Mapping::len);
            let enums = schema["enums"].as_mapping().map_or(0, Mapping::len);
            println!(
                "Wrote {} — {classes} class(es), {enums} enum(s), status filter {:?}.",
                path.display(),
                opts.status
            );
        }
        None => print!("{yaml}"),
    }
    Ok(())
}

/// What the schema says beside its definitions: open gaps and open disagreements.
struct Annotations {
    /// Gap question per `(table, column, value)`.
    value_gaps: BTreeMap<(String, String, String), String>,
    /// Gap question per concept name.
    concept_gaps: BTreeMap<String, String>,
    /// Open disagreements between concepts, per concept name.
    conflicts: BTreeMap<String, Vec<String>>,
}

fn collect_annotations(items: &[KirObject]) -> Annotations {
    let mut value_gaps: BTreeMap<(String, String, String), String> = BTreeMap::new();
    let mut concept_gaps: BTreeMap<String, String> = BTreeMap::new();
    // Open disagreements between concepts, per concept name.
    let mut conflicts: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for c in items
        .iter()
        .filter(|o| kind(o) == CONFLICT && text(o, "status") != "rejected")
    {
        for name in prop(c, "concepts").as_array().into_iter().flatten() {
            if let Some(n) = name.as_str() {
                conflicts
                    .entry(n.to_string())
                    .or_default()
                    .push(text(c, "question"));
            }
        }
    }
    for g in items
        .iter()
        .filter(|o| kind(o) == GAP && text(o, "status") != "rejected")
    {
        match text(g, "gap_type").as_str() {
            "unexplained_value" => {
                value_gaps.insert(
                    (text(g, "table"), text(g, "column"), text(g, "value")),
                    text(g, "question"),
                );
            }
            "undocumented_concept" => {
                concept_gaps.insert(text(g, "concept"), text(g, "question"));
            }
            _ => {}
        }
    }
    Annotations {
        value_gaps,
        concept_gaps,
        conflicts,
    }
}

type EnumNames = BTreeMap<(String, String), String>;

/// A table class's attributes, one per recovered column. A composite primary key is not marked:
/// LinkML allows one identifier per class.
fn table_attributes(obj: &KirObject, t: &str, enum_name: &EnumNames) -> Mapping {
    let mut attrs = Mapping::new();
    if let Some(Json::Array(cols)) = obj.properties.get("columns") {
        for c in cols {
            let Some(col) = c["name"].as_str() else {
                continue;
            };
            let col_l = col.to_lowercase();
            let sql_type = c["data_type"].as_str().unwrap_or_default();
            let flag = |k: &str| c[k].as_bool().unwrap_or(false);
            let mut annotations = vec![("ekos_sql_type", s(sql_type))];
            if let Some(e) = enum_name.get(&(t.to_string(), col_l.clone())) {
                annotations.push(("ekos_enum", s(e.clone())));
            }
            attrs.insert(
                s(col_l.clone()),
                map(vec![
                    ("range", s(linkml_range(sql_type))),
                    (
                        "required",
                        if flag("not_null") {
                            Value::Bool(true)
                        } else {
                            Value::Null
                        },
                    ),
                    (
                        "identifier",
                        if flag("primary_key") {
                            Value::Bool(true)
                        } else {
                            Value::Null
                        },
                    ),
                    (
                        "description",
                        c["description"]
                            .as_str()
                            .map(|d| s(d.trim()))
                            .unwrap_or(Value::Null),
                    ),
                    ("annotations", map(annotations)),
                ]),
            );
        }
    }
    // Single-column primary key only: LinkML allows one identifier per class.
    let ids = attrs
        .values()
        .filter(|v| v.get("identifier").is_some())
        .count();
    if ids > 1 {
        for v in attrs.values_mut() {
            if let Some(m) = v.as_mapping_mut() {
                m.remove(s("identifier"));
            }
        }
    }
    attrs
}

/// What one top-level predicate of a CHECK constraint says about its column, as slot usage.
fn predicate_usage(
    p: &Json,
    t: &str,
    col: &str,
    values: &[String],
    enum_name: &EnumNames,
) -> Vec<(&'static str, Value)> {
    let num = |v: &str| {
        v.parse::<f64>().ok().map(|f| {
            if f.fract() == 0.0 {
                Value::Number((f as i64).into())
            } else {
                Value::Number(f.into())
            }
        })
    };
    let mut usage = vec![];
    match (p["op"].as_str().unwrap_or_default(), values) {
        (">=", [v]) => usage.push(("minimum_value", num(v).unwrap_or(Value::Null))),
        ("<=", [v]) => usage.push(("maximum_value", num(v).unwrap_or(Value::Null))),
        ("between", [a, b]) => {
            usage.push(("minimum_value", num(a).unwrap_or(Value::Null)));
            usage.push(("maximum_value", num(b).unwrap_or(Value::Null)));
        }
        ("like", [v]) => usage.push(("pattern", s(like_to_regex(&unquote(v))))),
        ("in", _) => {
            if let Some(e) = enum_name.get(&(t.to_string(), col.to_string())) {
                usage.push(("range", s(e.clone())));
            }
        }
        ("is_not_null", _) => usage.push(("required", Value::Bool(true))),
        _ => {}
    }
    if usage.iter().all(|(_, v)| matches!(v, Value::Null)) {
        usage.clear();
    }
    usage
}

/// A table class's `slot_usage`: the bounds, patterns and ranges its admitted CHECK constraints
/// state, each annotated with the constraint it came from.
fn table_slot_usage(
    ledger: &dyn KnowledgeStore,
    t: &str,
    constraints: Option<&Vec<&KirObject>>,
    attrs: &Mapping,
    enum_name: &EnumNames,
) -> Mapping {
    let mut slot_usage = Mapping::new();
    for c in constraints.into_iter().flatten() {
        let structured: Vec<Json> = prop(c, "structured")
            .as_array()
            .cloned()
            .unwrap_or_default();
        for p in structured
            .iter()
            .filter(|p| p["top_level"].as_bool() == Some(true))
        {
            let col = p["column"].as_str().unwrap_or_default().to_lowercase();
            if !attrs.contains_key(s(col.clone())) {
                continue;
            }
            let values: Vec<String> = p["values"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            let mut usage = predicate_usage(p, t, &col, &values, enum_name);
            let refs = evidence_refs(ledger, c, 3);
            usage.push((
                "annotations",
                map(vec![
                    ("ekos_id", s(c.id.to_string())),
                    ("ekos_status", s(text(c, "status"))),
                    ("ekos_constraint", s(text(c, "expression"))),
                    ("ekos_evidence", s(refs.join("; "))),
                ]),
            ));
            // Several CHECKs on one column: the later one's bounds are kept, all are annotated.
            let entry = slot_usage
                .entry(s(col.clone()))
                .or_insert_with(|| Value::Mapping(Mapping::new()));
            if let Some(m) = entry.as_mapping_mut() {
                for (k, v) in usage {
                    if !matches!(v, Value::Null) {
                        m.insert(s(k), v);
                    }
                }
            }
        }
    }
    slot_usage
}

/// The open gap and disagreements recorded against one concept.
struct ConceptNotes<'a> {
    gap: Option<&'a String>,
    conflicts: &'a [String],
}

/// The class for one admitted concept, a subclass of its table's class.
fn concept_class(
    ledger: &dyn KnowledgeStore,
    o: &KirObject,
    parent: &str,
    rationale: &HashMap<String, &KirObject>,
    notes: &ConceptNotes<'_>,
) -> Result<Value> {
    let subject = text(o, "table");
    let expert = text(o, "expert_name");
    let refs = evidence_refs(ledger, o, 6);
    let mut commits: Vec<String> = Vec::new();
    for r in ledger.relationships_for(&o.id)? {
        if r.from == o.id
            && r.kind == RelationshipKind::Custom(EXPLAINED_BY.into())
            && let Some(link) = rationale.get(&r.to.to_string())
        {
            let sha = text(link, "sha");
            commits.push(format!(
                "commit {} \"{}\" ({})",
                &sha[..sha.len().min(10)],
                text(link, "summary"),
                text(link, "date").split('T').next().unwrap_or_default()
            ));
        }
    }
    commits.sort();
    // An expert's description replaces the generated one outright (the predicate stays in
    // `ekos_definition`), so a round trip through `ekos import linkml` never grows it.
    let author = o.properties.get("description").and_then(Json::as_str);
    let description = match o
        .properties
        .get("expert_description")
        .and_then(Json::as_str)
    {
        Some(expert) => expert.trim().to_string(),
        None => format!(
            "{}Rows of `{subject}` where {}.",
            author
                .map(|d| format!("{}\n", d.trim()))
                .unwrap_or_default(),
            text(o, "definition")
        ),
    };
    let mut comments: Vec<Value> = vec![s(format!(
        "EKOS {} recovered from {} site(s); name {}.",
        text(o, "status"),
        text(o, "sites"),
        if !expert.is_empty() {
            format!(
                "given by {} (recovered as `{}`)",
                text(o, "reviewed_by"),
                o.name
            )
        } else if text(o, "name_source") == "view" {
            format!("taken from view `{}`", text(o, "view"))
        } else {
            "derived from the predicate, not from any author".to_string()
        }
    ))];
    if let Some(q) = notes.gap {
        comments.push(s(format!("GAP: {q}")));
    }
    for q in notes.conflicts {
        comments.push(s(format!("CONFLICT: {q}")));
    }
    Ok(map(vec![
        ("is_a", s(parent)),
        ("description", s(description)),
        ("comments", Value::Sequence(comments)),
        (
            "annotations",
            map(vec![
                ("ekos_id", s(o.id.to_string())),
                ("ekos_status", s(text(o, "status"))),
                ("ekos_confidence", s(text(o, "confidence"))),
                ("ekos_definition", s(text(o, "definition"))),
                ("ekos_origin", s(text(o, "origin"))),
                ("ekos_evidence", s(refs.join("; "))),
                (
                    "ekos_rationale",
                    if commits.is_empty() {
                        Value::Null
                    } else {
                        s(commits.join("; "))
                    },
                ),
                (
                    "ekos_gap",
                    notes.gap.map(|q| s(q.clone())).unwrap_or(Value::Null),
                ),
            ]),
        ),
    ]))
}

/// The permissible values of one coded column, in value order, each with its label, sources and
/// any open gap.
fn permissible_values(
    ledger: &dyn KnowledgeStore,
    t: &str,
    c: &str,
    values: &[&KirObject],
    value_gaps: &BTreeMap<(String, String, String), String>,
) -> Mapping {
    let mut pvs = Mapping::new();
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| {
        let (va, vb) = (text(a, "value"), text(b, "value"));
        match (va.parse::<i64>(), vb.parse::<i64>()) {
            (Ok(x), Ok(y)) => x.cmp(&y),
            _ => va.cmp(&vb),
        }
    });
    for v in sorted {
        let value = text(v, "value");
        let label = match text(v, "expert_label") {
            e if e.is_empty() => text(v, "label"),
            e => e,
        };
        let sources: Vec<String> = prop(v, "meanings")
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|m| m["source"].as_str().map(str::to_string))
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect()
            })
            .unwrap_or_default();
        let gap = value_gaps.get(&(t.to_string(), c.to_string(), value.clone()));
        pvs.insert(
            s(unquote(&value)),
            map(vec![
                (
                    "description",
                    if label.is_empty() {
                        Value::Null
                    } else {
                        s(label)
                    },
                ),
                (
                    "annotations",
                    map(vec![
                        ("ekos_id", s(v.id.to_string())),
                        ("ekos_status", s(text(v, "status"))),
                        ("ekos_confidence", s(text(v, "confidence"))),
                        ("ekos_reviewed_by", opt(v, "reviewed_by")),
                        (
                            "ekos_suggested_exact_mappings",
                            suggestion_annotations(&item_suggestions(v))[0].1.clone(),
                        ),
                        (
                            "ekos_suggested_close_mappings",
                            suggestion_annotations(&item_suggestions(v))[1].1.clone(),
                        ),
                        (
                            "ekos_source",
                            if sources.is_empty() {
                                Value::Null
                            } else {
                                s(sources.join(", "))
                            },
                        ),
                        ("ekos_usage_sites", s(text(v, "usage_sites"))),
                        ("ekos_evidence", s(evidence_refs(ledger, v, 4).join("; "))),
                        ("ekos_gap", gap.map(|q| s(q.clone())).unwrap_or(Value::Null)),
                    ]),
                ),
            ]),
        );
    }
    pvs
}

/// The schema, or `None` when the status filter admits nothing.
pub fn build_schema(
    ledger: &dyn KnowledgeStore,
    items: &[KirObject],
    status: StatusFilter,
    name: &str,
    vocab: &ekos_semantic::ontology::Vocabulary,
) -> Result<Option<Value>> {
    let admitted: Vec<&KirObject> = items
        .iter()
        .filter(|o| matches!(kind(o), k if k == CONCEPT || k == ENUM_MEANING || k == CONSTRAINT))
        .filter(|o| status.admits(&text(o, "status")))
        .collect();
    if admitted.is_empty() {
        return Ok(None);
    }

    // Gaps per subject: `table.column = value` for values, concept name for concepts.
    let Annotations {
        value_gaps,
        concept_gaps,
        conflicts,
    } = collect_annotations(items);

    // Tables referenced by any admitted item, with their recovered columns.
    let all = ledger.all_objects()?;
    let tables: BTreeMap<String, &KirObject> = all
        .iter()
        .filter(|o| o.kind == ObjectKind::Table)
        .map(|o| {
            (
                o.name.rsplit('.').next().unwrap_or(&o.name).to_lowercase(),
                o,
            )
        })
        .collect();
    let mut wanted: BTreeSet<String> = BTreeSet::new();
    for o in &admitted {
        let t = text(o, "table");
        if tables.contains_key(&t) {
            wanted.insert(t);
        }
    }
    let rationale: HashMap<String, &KirObject> = items
        .iter()
        .filter(|o| kind(o) == RATIONALE)
        .map(|o| (o.id.to_string(), o))
        .collect();

    let mut used: BTreeSet<String> = BTreeSet::new();
    let table_class: BTreeMap<String, String> = wanted
        .iter()
        .map(|t| (t.clone(), unique(camel(t), &mut used, "Table")))
        .collect();

    // Enums: one per (table, column) with admitted values.
    let mut enum_values: BTreeMap<(String, String), Vec<&KirObject>> = BTreeMap::new();
    for o in admitted.iter().filter(|o| kind(o) == ENUM_MEANING) {
        enum_values
            .entry((text(o, "table"), text(o, "column")))
            .or_default()
            .push(o);
    }
    let enum_name: BTreeMap<(String, String), String> = enum_values
        .keys()
        .map(|(t, c)| {
            let n = unique(format!("{}{}", camel(t), camel(c)), &mut used, "Enum");
            ((t.clone(), c.clone()), n)
        })
        .collect();

    // Constraints per (table, column).
    let mut constraints: BTreeMap<String, Vec<&KirObject>> = BTreeMap::new();
    for o in admitted.iter().filter(|o| kind(o) == CONSTRAINT) {
        constraints.entry(text(o, "table")).or_default().push(o);
    }

    let mut classes = Mapping::new();
    for (t, class) in &table_class {
        let obj = tables[t];
        let attrs = table_attributes(obj, t, &enum_name);

        let slot_usage = table_slot_usage(ledger, t, constraints.get(t), &attrs, &enum_name);

        let description = obj
            .properties
            .get("description")
            .and_then(Json::as_str)
            .map(|d| d.trim().to_string())
            .unwrap_or_else(|| format!("Table `{}` as recovered from its DDL.", obj.name));
        classes.insert(
            s(class.clone()),
            map(vec![
                ("description", s(description)),
                (
                    "attributes",
                    if attrs.is_empty() {
                        Value::Null
                    } else {
                        Value::Mapping(attrs)
                    },
                ),
                (
                    "slot_usage",
                    if slot_usage.is_empty() {
                        Value::Null
                    } else {
                        Value::Mapping(slot_usage)
                    },
                ),
                ("annotations", {
                    let sugg: Vec<(String, String)> = vocab
                        .suggest(t)
                        .into_iter()
                        .map(|x| (x.id, x.match_type))
                        .collect();
                    let mut a = vec![
                        ("ekos_table", s(obj.name.clone())),
                        ("ekos_status", s("observed")),
                    ];
                    a.extend(suggestion_annotations(&sugg));
                    map(a)
                }),
            ]),
        );
    }

    // Concepts.
    for o in admitted.iter().filter(|o| kind(o) == CONCEPT) {
        let subject = text(o, "table");
        let Some(parent) = table_class.get(&subject) else {
            continue;
        };
        let expert = text(o, "expert_name");
        let class = unique(
            if expert.is_empty() {
                o.name.clone()
            } else {
                camel(&expert)
            },
            &mut used,
            "Concept",
        );
        let notes = ConceptNotes {
            gap: concept_gaps.get(&o.name),
            conflicts: conflicts
                .get(&o.name)
                .map(Vec::as_slice)
                .unwrap_or_default(),
        };
        let entry = concept_class(ledger, o, parent, &rationale, &notes)?;
        classes.insert(s(class), entry);
    }

    // Enums.
    let mut enums = Mapping::new();
    for ((t, c), values) in &enum_values {
        let pvs = permissible_values(ledger, t, c, values, &value_gaps);
        enums.insert(
            s(enum_name[&(t.clone(), c.clone())].clone()),
            map(vec![
                (
                    "description",
                    s(format!(
                        "Coded values of `{t}.{c}` recovered from code traces."
                    )),
                ),
                ("permissible_values", Value::Mapping(pvs)),
            ]),
        );
    }

    let id = format!("https://w3id.org/ekos/{name}");
    Ok(Some(map(vec![
        ("id", s(id.clone())),
        ("name", s(name)),
        (
            "title",
            s(format!(
                "{name} — business semantics recovered by EKOS (draft)"
            )),
        ),
        (
            "description",
            s(
                "Draft business-semantics schema recovered by EKOS (RFC 0170) from SQL predicates, \
               CHECK constraints, lookup seed rows, column comments and git history. Classes \
               derived from tables are observed structure; concepts and enum meanings are \
               hypotheses until a domain expert confirms them.",
            ),
        ),
        (
            "prefixes",
            map(vec![
                ("linkml", s("https://w3id.org/linkml/")),
                ("ekos", s("https://w3id.org/ekos/")),
                (name, s(format!("{id}/"))),
            ]),
        ),
        ("default_prefix", s(name)),
        ("default_range", s("string")),
        ("imports", Value::Sequence(vec![s("linkml:types")])),
        (
            "annotations",
            map(vec![(
                "ekos_export_status",
                s(format!("{status:?}").to_lowercase()),
            )]),
        ),
        ("classes", Value::Mapping(classes)),
        (
            "enums",
            if enums.is_empty() {
                Value::Null
            } else {
                Value::Mapping(enums)
            },
        ),
    ])))
}

// ── RFC 0245: Apache Ossie ─────────────────────────────────────────────────────────────────

/// Options for `ekos export ossie`.
pub struct OssieOptions {
    pub status: StatusFilter,
    pub out: Option<PathBuf>,
    pub name: Option<String>,
    /// JSON instead of YAML.
    pub json: bool,
    pub database: Option<String>,
    pub source: Option<String>,
}

/// `<state>/ossie/current.json`: the Ossie files the latest `recover` found (RFC 0245). The ledger
/// keeps a deleted file's objects; every reader leaves out what this does not list.
fn ossie_manifest_path(config: &EkosConfig, cwd: &Path) -> PathBuf {
    config.ekos_dir(cwd).join("ossie").join("current.json")
}

/// The current Ossie files, or `None` before any `recover` recorded them.
pub fn current_ossie_files(config: &EkosConfig, cwd: &Path) -> Option<BTreeSet<String>> {
    let bytes = std::fs::read(ossie_manifest_path(config, cwd)).ok()?;
    let v: Json = serde_json::from_slice(&bytes).ok()?;
    Some(
        v["files"]
            .as_array()?
            .iter()
            .filter_map(|f| f.as_str().map(str::to_string))
            .collect(),
    )
}

/// Record the Ossie files this `recover` found — empty too, so a deleted file drops out.
pub fn write_current_ossie_files(config: &EkosConfig, cwd: &Path, files: &[String]) -> Result<()> {
    let path = ossie_manifest_path(config, cwd);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut files = files.to_vec();
    files.sort();
    files.dedup();
    std::fs::write(
        &path,
        serde_json::to_vec_pretty(&serde_json::json!({"files": files}))?,
    )
    .with_context(|| format!("writing {}", path.display()))
}

/// What [`ossie_model`] needs besides the store: shared by the CLI and the MCP tool.
pub struct OssieRequest<'a> {
    pub status: StatusFilter,
    pub name: Option<&'a str>,
    pub database: Option<&'a str>,
    pub source: Option<&'a str>,
}

/// The ledger as one Apache Ossie semantic model (RFC 0245): every table and view, their keys
/// and foreign keys, the business meanings the status filter admits, and the metrics of Ossie
/// files ingested from the sources. Reads only; fails when the result would not pass the
/// structural checks of Ossie's own validator.
pub fn ossie_model(
    config: &EkosConfig,
    cwd: &Path,
    ledger: &dyn KnowledgeStore,
    req: &OssieRequest,
) -> Result<ekos_ossie::SemanticModel> {
    let files = current_ossie_files(config, cwd);
    let objects: Vec<KirObject> = ledger
        .all_objects()?
        .into_iter()
        .filter(|o| ekos_semantic::ossie_links::is_current(o, files.as_ref()))
        .collect();
    let relationships = ledger.all_relationships()?;
    let items = super::semantics::items_in(config, cwd, ledger)?;
    let admits = |s: &str| req.status.admits(s);
    let project = default_schema_name(cwd);
    let opts = ekos_ossie::BuildOptions {
        name: req
            .name
            .map(str::to_string)
            .unwrap_or_else(|| project.clone()),
        export_status: req.status.as_str().into(),
        admits: &admits,
        database: req.database.map(str::to_string),
        source: req.source.map(str::to_string),
        ekos_version: env!("CARGO_PKG_VERSION").into(),
        project: Some(project),
    };
    let view = ekos_ossie::LedgerView {
        objects: &objects,
        relationships: &relationships,
        items: &items,
    };
    let model = ekos_ossie::from_ledger(&view, &opts, &|o| evidence_refs(ledger, o, 3))
        .map_err(anyhow::Error::msg)?;
    let problems = ekos_ossie::check(&model);
    if !problems.is_empty() {
        anyhow::bail!(
            "the Ossie model failed its own structural checks (an EKOS bug — please report it):\n{}",
            problems
                .iter()
                .map(|p| format!("  {}: {}", p.path, p.message))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }
    Ok(model)
}

/// The header comment of an exported YAML document.
pub const OSSIE_HEADER: &str = "# Apache Ossie semantic model compiled by EKOS (RFC 0245), spec 0.2.0.dev0.\n\
     # Every element's `EKOS` extension carries its ledger id, status and evidence. Business\n\
     # meanings marked \"unreviewed\" are hypotheses, not confirmed definitions.\n\
     # Edit `status`/`label`/`name`/`description` inside the extensions' `concepts` and\n\
     # `enum_meanings`, then `ekos import ossie <file>` turns the edits into review decisions.\n";

/// `ekos export ossie`.
pub fn ossie(config: &EkosConfig, cwd: &Path, opts: &OssieOptions) -> Result<()> {
    let ledger = crate::commands::store::open_store_read_only(config, cwd)?;
    let model = ossie_model(
        config,
        cwd,
        &*ledger,
        &OssieRequest {
            status: opts.status,
            name: opts.name.as_deref(),
            database: opts.database.as_deref(),
            source: opts.source.as_deref(),
        },
    )?;
    let json = opts.json
        || opts
            .out
            .as_ref()
            .is_some_and(|p| p.extension().is_some_and(|e| e == "json"));
    let text = if json {
        format!("{}\n", serde_json::to_string_pretty(&model.to_json())?)
    } else {
        format!("{OSSIE_HEADER}{}", model.to_yaml())
    };
    match &opts.out {
        Some(path) => {
            std::fs::write(path, &text).with_context(|| format!("writing {}", path.display()))?;
            let fields: usize = model.datasets.iter().map(|d| d.fields.len()).sum();
            let metrics: usize = model.datasets.iter().map(|d| d.metrics.len()).sum();
            println!(
                "Wrote {} — {} dataset(s), {fields} field(s), {} relationship(s), {metrics} \
                 metric(s), status filter {}.",
                path.display(),
                model.datasets.len(),
                model.relationships.len(),
                opts.status.as_str()
            );
        }
        None => print!("{text}"),
    }
    Ok(())
}

/// `ekos export dbt-semantic --out <dir>` (RFC 0246): the current Apache Ossie models as a dbt
/// semantic layer — `semantic_models.yml` (MetricFlow `semantic_models:` + `metrics:`),
/// `models.yml` (key tests) and `skipped.md` (what MetricFlow cannot express, and why).
pub fn dbt_semantic(config: &EkosConfig, cwd: &Path, out: &Path, time_spine: bool) -> Result<()> {
    let ledger = crate::commands::store::open_store_read_only(config, cwd)?;
    let objects = ledger.all_objects()?;
    let relationships = ledger.all_relationships()?;
    let files = current_ossie_files(config, cwd);
    let knowledge = ekos_ossie::knowledge::resolve(&objects, &relationships, files.as_ref());
    if knowledge.tables.is_empty() {
        anyhow::bail!(
            "no Apache Ossie model is linked to a compiled table — add an `*.ossie.yaml` to the \
             project's sources and run the pipeline (RFC 0245)"
        );
    }
    let names: HashMap<ekos_kir::KirId, String> = objects
        .iter()
        .filter(|o| o.kind == ObjectKind::Table)
        .map(|o| (o.id, ekos_ossie::build::name_parts(&o.name).join(".")))
        .collect();
    let mut fks = Vec::new();
    for r in relationships
        .iter()
        .filter(|r| r.kind == RelationshipKind::ForeignKey)
    {
        let (Some(from), Some(to)) = (names.get(&r.from), names.get(&r.to)) else {
            continue;
        };
        let Some((fc, tc)) = r
            .properties
            .get("fk_desc")
            .and_then(Json::as_str)
            .and_then(ekos_ossie::fk::fk_columns)
        else {
            continue;
        };
        for (f, t) in fc.into_iter().zip(tc) {
            fks.push(ekos_dbt_gen::semantic::KeyJoin {
                from_table: from.clone(),
                from_column: f,
                to_table: to.clone(),
                to_column: t,
            });
        }
    }
    let layer = ekos_dbt_gen::semantic::render_semantic_layer(&knowledge, &fks);
    std::fs::create_dir_all(out).with_context(|| format!("creating {}", out.display()))?;
    for (name, text) in [
        ("semantic_models.yml", &layer.semantic_yml),
        ("models.yml", &layer.models_yml),
        ("skipped.md", &layer.skipped_md),
    ] {
        let path = out.join(name);
        std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
    }
    if time_spine {
        for (name, text) in [
            (
                "metricflow_time_spine.sql",
                ekos_dbt_gen::semantic::TIME_SPINE_SQL,
            ),
            ("time_spine.yml", ekos_dbt_gen::semantic::TIME_SPINE_YML),
        ] {
            let path = out.join(name);
            std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
        }
    }
    println!(
        "Wrote {} — {} semantic model(s), {} metric(s), {} skipped (see skipped.md).",
        out.display(),
        layer.semantic_models,
        layer.metrics,
        layer.skipped
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sql_types_map_to_linkml_types() {
        assert_eq!(linkml_range("INT"), "integer");
        assert_eq!(linkml_range("BIGSERIAL"), "integer");
        assert_eq!(linkml_range("NUMERIC(12,2)"), "decimal");
        assert_eq!(linkml_range("DOUBLE PRECISION"), "float");
        assert_eq!(linkml_range("TIMESTAMP WITH TIME ZONE"), "datetime");
        assert_eq!(linkml_range("DATE"), "date");
        assert_eq!(linkml_range("BOOLEAN"), "boolean");
        assert_eq!(linkml_range("CHARACTER VARYING(10)"), "string");
    }

    #[test]
    fn like_patterns_become_anchored_regexes() {
        assert_eq!(like_to_regex("AR-%"), "^AR-.*$");
        assert_eq!(like_to_regex("a_c.d"), "^a.c\\.d$");
    }

    #[test]
    fn names_are_unique_and_never_start_with_a_digit() {
        let mut used = BTreeSet::new();
        assert_eq!(unique("Parts".into(), &mut used, "Concept"), "Parts");
        assert_eq!(unique("Parts".into(), &mut used, "Concept"), "PartsConcept");
        assert_eq!(
            unique("Parts".into(), &mut used, "Concept"),
            "PartsConcept2"
        );
        assert_eq!(unique("1099".into(), &mut used, "X"), "N1099");
    }

    #[test]
    fn only_admitted_statuses_are_exported() {
        assert!(!StatusFilter::Confirmed.admits("hypothesis"));
        assert!(StatusFilter::Hypothesis.admits("hypothesis"));
        assert!(StatusFilter::All.admits("hypothesis"));
        assert!(!StatusFilter::All.admits("rejected"));
    }
}
