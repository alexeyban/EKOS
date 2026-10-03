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
    fn admits(self, status: &str) -> bool {
        match self {
            StatusFilter::Confirmed => status == "confirmed",
            StatusFilter::Hypothesis => status == "hypothesis",
            StatusFilter::All => status != "rejected",
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
    let schema = build_schema(&*ledger, &items, opts.status, &name)?;
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

/// The schema, or `None` when the status filter admits nothing.
pub fn build_schema(
    ledger: &dyn KnowledgeStore,
    items: &[KirObject],
    status: StatusFilter,
    name: &str,
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
                if let Some(e) = enum_name.get(&(t.clone(), col_l.clone())) {
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

        let mut slot_usage = Mapping::new();
        for c in constraints.get(t).into_iter().flatten() {
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
                match (p["op"].as_str().unwrap_or_default(), values.as_slice()) {
                    (">=", [v]) => usage.push(("minimum_value", num(v).unwrap_or(Value::Null))),
                    ("<=", [v]) => usage.push(("maximum_value", num(v).unwrap_or(Value::Null))),
                    ("between", [a, b]) => {
                        usage.push(("minimum_value", num(a).unwrap_or(Value::Null)));
                        usage.push(("maximum_value", num(b).unwrap_or(Value::Null)));
                    }
                    ("like", [v]) => usage.push(("pattern", s(like_to_regex(&unquote(v))))),
                    ("in", _) => {
                        if let Some(e) = enum_name.get(&(t.clone(), col.clone())) {
                            usage.push(("range", s(e.clone())));
                        }
                    }
                    ("is_not_null", _) => usage.push(("required", Value::Bool(true))),
                    _ => {}
                }
                if usage.iter().all(|(_, v)| matches!(v, Value::Null)) {
                    usage.clear();
                }
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
                (
                    "annotations",
                    map(vec![
                        ("ekos_table", s(obj.name.clone())),
                        ("ekos_status", s("observed")),
                    ]),
                ),
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
        if let Some(q) = concept_gaps.get(&o.name) {
            comments.push(s(format!("GAP: {q}")));
        }
        for q in conflicts.get(&o.name).into_iter().flatten() {
            comments.push(s(format!("CONFLICT: {q}")));
        }
        classes.insert(
            s(class),
            map(vec![
                ("is_a", s(parent.clone())),
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
                            concept_gaps
                                .get(&o.name)
                                .map(|q| s(q.clone()))
                                .unwrap_or(Value::Null),
                        ),
                    ]),
                ),
            ]),
        );
    }

    // Enums.
    let mut enums = Mapping::new();
    for ((t, c), values) in &enum_values {
        let mut pvs = Mapping::new();
        let mut sorted = values.clone();
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
            let gap = value_gaps.get(&(t.clone(), c.clone(), value.clone()));
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
