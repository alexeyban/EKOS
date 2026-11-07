//! The EKOS ledger as one Ossie semantic model (RFC 0245 §3). Pure: the caller loads the objects.
//!
//! | EKOS | Ossie |
//! |---|---|
//! | `Table` / `Custom("View")` | dataset; one field per column, `datatype` from the SQL type |
//! | table `primary_key` / `unique_keys` (or per-column flags) | `primary_key` / `unique_keys` |
//! | `ForeignKey` edge | relationship, when both ends are exported and every column is a field |
//! | `EnumMeaning` | the field's `ai_context.instructions` |
//! | `BusinessConcept` | the dataset's `ai_context.instructions` |
//! | `OssieDataset` linked to the table | its synonyms, instructions and, when its fields are the table's columns unchanged, its metrics |
//!
//! Nothing is invented: a metric appears only when an ingested Ossie file defined it, and a
//! business concept (a predicate) is never rewritten as an aggregation. Every element carries an
//! `EKOS` extension with its `ekos_id`, status and evidence; unreviewed meanings say so in words.

use crate::datatype;
use crate::fk::fk_columns;
use crate::kinds::{LINK, OSSIE_DATASET, OSSIE_METRIC, is_concept, is_enum_meaning};
use crate::model::*;
use ekos_kir::{KirId, KirObject, KirRelationship, ObjectKind, RelationshipKind};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// What the builder reads.
pub struct LedgerView<'a> {
    /// Every current object (tables, views, ingested Ossie objects).
    pub objects: &'a [KirObject],
    pub relationships: &'a [KirRelationship],
    /// The current RFC 0170 items (the semantics manifest already applied).
    pub items: &'a [KirObject],
}

/// How to build.
pub struct BuildOptions<'a> {
    pub name: String,
    /// The `--status` filter's name, recorded on the model for `ekos import ossie`.
    pub export_status: String,
    /// Whether a review status is exported.
    pub admits: &'a dyn Fn(&str) -> bool,
    /// Only tables of this database (`[[databases]]` name), case-insensitive.
    pub database: Option<String>,
    /// Only tables attributed to this `[[sources]]` name.
    pub source: Option<String>,
    pub ekos_version: String,
    pub project: Option<String>,
}

fn prop<'a>(o: &'a KirObject, k: &str) -> &'a Value {
    o.properties.get(k).unwrap_or(&Value::Null)
}

fn text(o: &KirObject, k: &str) -> String {
    match prop(o, k) {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        v => v.to_string(),
    }
}

/// The first non-empty of the given string properties.
fn first_text(o: &KirObject, keys: &[&str]) -> String {
    keys.iter()
        .map(|k| text(o, k))
        .find(|t| !t.trim().is_empty())
        .unwrap_or_default()
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn fold(s: &str) -> String {
    s.to_lowercase()
}

/// The parts of a possibly quoted, dotted SQL name: `"public"."Orders"` → `[public, Orders]`.
pub fn name_parts(name: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    for c in name.trim().chars() {
        match (quote, c) {
            (None, '"' | '`') => quote = Some(c),
            (None, '[') => quote = Some(']'),
            (Some(q), c) if c == q => quote = None,
            (None, '.') => parts.push(std::mem::take(&mut cur)),
            (_, c) => cur.push(c),
        }
    }
    parts.push(cur);
    parts.retain(|p| !p.is_empty());
    parts
}

/// The unqualified, unquoted name of a table.
pub fn short_name(name: &str) -> String {
    name_parts(name).pop().unwrap_or_else(|| name.to_string())
}

/// A column as an `OSSIE_SQL_2026` expression: bare when it is a regular identifier, else
/// double-quoted (the spec follows ANSI quoting).
pub fn column_expression(column: &str) -> String {
    let plain = column
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && column
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_');
    if plain {
        column.to_string()
    } else {
        format!("\"{}\"", column.replace('"', "\"\""))
    }
}

/// The column a plain expression names (`col` or `"col"`), or `None` for anything computed.
pub fn plain_column(sql: &str) -> Option<String> {
    let t = sql.trim();
    if let Some(inner) = t.strip_prefix('"').and_then(|r| r.strip_suffix('"')) {
        return (!inner.is_empty()).then(|| inner.replace("\"\"", "\""));
    }
    (column_expression(t) == t && !t.is_empty()).then(|| t.to_string())
}

/// A name not yet used (case-folded), by suffixing `_2`, `_3`, …
fn unique(name: &str, used: &mut BTreeSet<String>) -> String {
    let base = if name.trim().is_empty() {
        "unnamed"
    } else {
        name
    };
    if used.insert(fold(base)) {
        return base.to_string();
    }
    (2..)
        .map(|n| format!("{base}_{n}"))
        .find(|c| used.insert(fold(c)))
        .expect("an unused suffix exists")
}

fn is_view(o: &KirObject) -> bool {
    matches!(&o.kind, ObjectKind::Custom(k) if k == "View")
}

fn is_custom(o: &KirObject, kind: &str) -> bool {
    matches!(&o.kind, ObjectKind::Custom(k) if k == kind)
}

fn selected(o: &KirObject, opts: &BuildOptions) -> bool {
    let db_ok = opts
        .database
        .as_ref()
        .is_none_or(|d| text(o, "database").eq_ignore_ascii_case(d));
    let src_ok = opts.source.as_ref().is_none_or(|s| {
        text(o, "source") == *s || strings(prop(o, "sources")).iter().any(|x| x == s)
    });
    db_ok && src_ok
}

/// `database.owner.table`, `schema.table` or `table`, as precise as the ledger knows it.
fn source_of(o: &KirObject) -> String {
    let parts = name_parts(&o.name);
    if parts.len() > 1 {
        return parts.join(".");
    }
    let name = parts.first().cloned().unwrap_or_else(|| o.name.clone());
    let owner = first_text(o, &["owner", "schema"]);
    let database = text(o, "database");
    match (database.is_empty(), owner.is_empty()) {
        (false, false) => format!("{database}.{owner}.{name}"),
        (_, false) => format!("{owner}.{name}"),
        _ => name,
    }
}

/// One table's column: name, SQL type, description, key flags.
struct Column {
    name: String,
    data_type: String,
    description: String,
    primary_key: bool,
    unique: bool,
}

fn columns_of(o: &KirObject) -> Vec<Column> {
    let Some(cols) = prop(o, "columns").as_array() else {
        return Vec::new();
    };
    let mut seen = BTreeSet::new();
    cols.iter()
        .filter_map(|c| match c {
            Value::String(n) => Some(Column {
                name: n.clone(),
                data_type: String::new(),
                description: String::new(),
                primary_key: false,
                unique: false,
            }),
            Value::Object(m) => Some(Column {
                name: m.get("name")?.as_str()?.to_string(),
                data_type: m
                    .get("data_type")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                description: m
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                primary_key: m.get("primary_key").and_then(Value::as_bool) == Some(true),
                unique: m.get("unique").and_then(Value::as_bool) == Some(true),
            }),
            _ => None,
        })
        .filter(|c| !c.name.is_empty() && seen.insert(fold(&c.name)))
        .collect()
}

/// A key's columns as the dataset's field names; `None` when one is not a field.
fn as_fields(cols: &[String], fields: &[Field]) -> Option<Vec<String>> {
    cols.iter()
        .map(|c| {
            let c = short_name(c);
            fields
                .iter()
                .find(|f| fold(&f.name) == fold(&c))
                .map(|f| f.name.clone())
        })
        .collect()
}

fn keys_of(o: &KirObject, cols: &[Column], fields: &[Field]) -> (Vec<String>, Vec<Vec<String>>) {
    let stated_pk = strings(prop(o, "primary_key"));
    let pk = if stated_pk.is_empty() {
        cols.iter()
            .filter(|c| c.primary_key)
            .map(|c| c.name.clone())
            .collect()
    } else {
        stated_pk
    };
    let primary_key = as_fields(&pk, fields).unwrap_or_default();
    let stated: Vec<Vec<String>> = prop(o, "unique_keys")
        .as_array()
        .map(|a| a.iter().map(strings).collect())
        .unwrap_or_default();
    let uniques = if stated.is_empty() {
        cols.iter()
            .filter(|c| c.unique)
            .map(|c| vec![c.name.clone()])
            .collect()
    } else {
        stated
    };
    let mut unique_keys: Vec<Vec<String>> = Vec::new();
    for k in uniques.iter().filter(|k| !k.is_empty()) {
        if let Some(k) = as_fields(k, fields)
            && !unique_keys.contains(&k)
        {
            unique_keys.push(k);
        }
    }
    (primary_key, unique_keys)
}

/// The words an unreviewed meaning carries in every place it is exported.
fn status_note(status: &str) -> String {
    match status {
        "confirmed" => String::new(),
        "" => " (unreviewed)".into(),
        s => format!(" ({s}, unreviewed)"),
    }
}

/// One exported dataset plus what the later steps attach to it.
struct Built {
    object: usize,
    dataset: Dataset,
    ext: serde_json::Map<String, Value>,
    instructions: Vec<String>,
    synonyms: Vec<String>,
}

fn base_dataset(
    o: &KirObject,
    idx: usize,
    used: &mut BTreeSet<String>,
    evidence: &dyn Fn(&KirObject) -> Vec<String>,
) -> Built {
    let cols = columns_of(o);
    let fields: Vec<Field> = cols
        .iter()
        .map(|c| Field {
            name: c.name.clone(),
            expression: Expression::sql(column_expression(&c.name)),
            description: (!c.description.trim().is_empty()).then(|| c.description.clone()),
            datatype: datatype::from_sql(&c.data_type),
            ..Default::default()
        })
        .collect();
    let (primary_key, unique_keys) = keys_of(o, &cols, &fields);
    let mut ext = serde_json::Map::new();
    ext.insert("ekos_id".into(), json!(o.id.to_string()));
    ext.insert("status".into(), json!("observed"));
    ext.insert(
        "kind".into(),
        json!(if is_view(o) { "View" } else { "Table" }),
    );
    for k in ["origin", "dialect", "database"] {
        let v = text(o, k);
        if !v.is_empty() {
            ext.insert(k.into(), json!(v));
        }
    }
    let ev = evidence(o);
    if !ev.is_empty() {
        ext.insert("evidence".into(), json!(ev));
    }
    let description = text(o, "description");
    Built {
        object: idx,
        dataset: Dataset {
            name: unique(&short_name(&o.name), used),
            source: source_of(o),
            primary_key,
            unique_keys,
            description: (!description.trim().is_empty()).then_some(description),
            fields,
            ..Default::default()
        },
        ext,
        instructions: Vec::new(),
        synonyms: Vec::new(),
    }
}

/// `EnumMeaning`s onto their fields' `ai_context` and `EKOS` extension.
fn attach_enum_meanings(
    built: &mut [Built],
    by_table: &HashMap<String, Vec<usize>>,
    items: &[KirObject],
    admits: &dyn Fn(&str) -> bool,
) {
    let mut per_field: BTreeMap<(String, String), Vec<&KirObject>> = BTreeMap::new();
    for e in items.iter().filter(|o| is_enum_meaning(o)) {
        if admits(&text(e, "status")) {
            per_field
                .entry((
                    fold(&short_name(&text(e, "table"))),
                    fold(&text(e, "column")),
                ))
                .or_default()
                .push(e);
        }
    }
    for ((table, column), mut meanings) in per_field {
        meanings.sort_by_key(|e| (text(e, "value"), e.id.to_string()));
        let lines: Vec<String> = meanings
            .iter()
            .filter_map(|e| {
                let label = first_text(e, &["expert_label", "label"]);
                (!label.is_empty()).then(|| {
                    format!(
                        "{} = {label}{}",
                        text(e, "value"),
                        status_note(&text(e, "status"))
                    )
                })
            })
            .collect();
        let entries: Vec<Value> = meanings
            .iter()
            .map(|e| {
                json!({
                    "ekos_id": e.id.to_string(),
                    "value": text(e, "value"),
                    "label": first_text(e, &["expert_label", "label"]),
                    "status": text(e, "status"),
                })
            })
            .collect();
        for &b in by_table.get(&table).into_iter().flatten() {
            let Some(f) = built[b]
                .dataset
                .fields
                .iter_mut()
                .find(|f| fold(&f.name) == column)
            else {
                continue;
            };
            if !lines.is_empty() {
                f.ai_context = AiContext::from_parts(
                    Some(format!("Codes: {}.", lines.join("; "))),
                    Vec::new(),
                );
            }
            f.custom_extensions = vec![CustomExtension::ekos(&json!({
                "enum_meanings": entries,
            }))];
        }
    }
}

/// `BusinessConcept`s onto their tables' datasets.
fn attach_concepts(
    built: &mut [Built],
    by_table: &HashMap<String, Vec<usize>>,
    items: &[KirObject],
    admits: &dyn Fn(&str) -> bool,
) {
    let mut concepts: Vec<&KirObject> = items
        .iter()
        .filter(|o| is_concept(o) && admits(&text(o, "status")))
        .collect();
    concepts.sort_by_key(|c| (first_text(c, &["expert_name"]), c.name.clone()));
    for c in concepts {
        let name = first_text(c, &["expert_name"]);
        let name = if name.is_empty() {
            c.name.clone()
        } else {
            name
        };
        let description = first_text(c, &["expert_description", "description", "definition"]);
        let line = format!(
            "Business concept \"{name}\"{}: {description}",
            status_note(&text(c, "status"))
        );
        let entry = json!({
            "ekos_id": c.id.to_string(),
            "name": name,
            "description": first_text(c, &["expert_description", "description"]),
            "definition": text(c, "definition"),
            "status": text(c, "status"),
        });
        for &b in by_table
            .get(&fold(&short_name(&text(c, "table"))))
            .into_iter()
            .flatten()
        {
            built[b].instructions.push(line.clone());
            built[b]
                .ext
                .entry("concepts")
                .or_insert_with(|| json!([]))
                .as_array_mut()
                .expect("concepts is a list")
                .push(entry.clone());
        }
    }
}

fn ossie_fields(d: &KirObject) -> Vec<(String, Option<String>, &Value)> {
    prop(d, "fields")
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|f| {
                    let name = f.get("name")?.as_str()?.to_string();
                    let col = f.get("sql").and_then(Value::as_str).and_then(plain_column);
                    Some((name, col, f))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn metric_of(m: &KirObject, path: &str) -> Option<Metric> {
    let expression: Expression = serde_json::from_value(prop(m, "expression").clone()).ok()?;
    expression.text()?;
    let datatype = serde_json::from_value(prop(m, "datatype").clone()).ok();
    let description = text(m, "description");
    Some(Metric {
        name: m.name.clone(),
        expression,
        description: (!description.trim().is_empty()).then_some(description),
        datatype,
        ai_context: AiContext::from_parts(None, strings(prop(m, "synonyms"))),
        custom_extensions: vec![CustomExtension::ekos(&json!({
            "ekos_id": m.id.to_string(),
            "status": "observed",
            "origin": format!("ossie:{path}"),
        }))],
    })
}

/// Ingested Ossie datasets linked to an exported table: their synonyms and instructions, field
/// synonyms and descriptions by column, and — only when every one of their fields is a table
/// column under its own name, so their metric expressions stay valid — their dataset metrics.
fn attach_ingested(
    built: &mut [Built],
    by_object: &HashMap<KirId, usize>,
    objects: &[KirObject],
    relationships: &[KirRelationship],
) {
    let by_id: HashMap<KirId, &KirObject> = objects.iter().map(|o| (o.id, o)).collect();
    let mut links: Vec<(&KirObject, usize)> = relationships
        .iter()
        .filter(|r| r.kind == RelationshipKind::DependsOn)
        .filter(|r| r.properties.get("link").and_then(Value::as_str) == Some(LINK))
        .filter_map(|r| {
            let d = by_id.get(&r.from).filter(|d| is_custom(d, OSSIE_DATASET))?;
            Some((*d, *by_object.get(&r.to)?))
        })
        .collect();
    links.sort_by_key(|(d, b)| (*b, text(d, "path"), d.id.to_string()));
    links.dedup_by_key(|(d, b)| (d.id, *b));
    for (d, b) in links {
        let target = &mut built[b];
        let path = text(d, "path");
        target.synonyms.extend(strings(prop(d, "synonyms")));
        let instructions = text(d, "instructions");
        if !instructions.trim().is_empty() {
            target.instructions.push(instructions);
        }
        if target.dataset.description.is_none() {
            let desc = text(d, "description");
            target.dataset.description = (!desc.trim().is_empty()).then_some(desc);
        }
        let fields = ossie_fields(d);
        for (_, col, f) in &fields {
            let Some(col) = col else { continue };
            let Some(ours) = target
                .dataset
                .fields
                .iter_mut()
                .find(|x| fold(&x.name) == fold(col))
            else {
                continue;
            };
            let syn = strings(f.get("synonyms").unwrap_or(&Value::Null));
            if !syn.is_empty() {
                let mut all = ours
                    .ai_context
                    .as_ref()
                    .map(|a| a.synonyms().to_vec())
                    .unwrap_or_default();
                all.extend(
                    syn.into_iter()
                        .filter(|s| !all.contains(s))
                        .collect::<Vec<_>>(),
                );
                let instructions = ours
                    .ai_context
                    .as_ref()
                    .and_then(|a| a.instructions().map(str::to_string));
                ours.ai_context = AiContext::from_parts(instructions, all);
            }
            if ours.description.is_none() {
                ours.description = f
                    .get("description")
                    .and_then(Value::as_str)
                    .filter(|s| !s.trim().is_empty())
                    .map(str::to_string);
            }
        }
        let identity = fields.iter().all(|(name, col, _)| {
            col.as_ref().is_some_and(|c| {
                fold(c) == fold(name)
                    && target
                        .dataset
                        .fields
                        .iter()
                        .any(|x| fold(&x.name) == fold(c))
            })
        });
        if !identity {
            continue;
        }
        let mut metrics: Vec<&KirObject> = relationships
            .iter()
            .filter(|r| r.kind == RelationshipKind::Contains && r.from == d.id)
            .filter_map(|r| by_id.get(&r.to).copied())
            .filter(|m| is_custom(m, OSSIE_METRIC))
            .collect();
        metrics.sort_by_key(|m| (m.name.clone(), m.id.to_string()));
        for m in metrics {
            let taken = target
                .dataset
                .fields
                .iter()
                .map(|f| &f.name)
                .chain(target.dataset.metrics.iter().map(|x| &x.name))
                .any(|n| fold(n) == fold(&m.name));
            if !taken && let Some(metric) = metric_of(m, &path) {
                target.dataset.metrics.push(metric);
            }
        }
    }
}

fn relationships_of(
    built: &[Built],
    by_object: &HashMap<KirId, usize>,
    relationships: &[KirRelationship],
) -> Vec<Relationship> {
    let mut out: Vec<Relationship> = Vec::new();
    let mut seen = BTreeSet::new();
    let mut fks: Vec<&KirRelationship> = relationships
        .iter()
        .filter(|r| r.kind == RelationshipKind::ForeignKey)
        .collect();
    fks.sort_by_key(|r| r.id.to_string());
    for r in fks {
        let (Some(&a), Some(&b)) = (by_object.get(&r.from), by_object.get(&r.to)) else {
            continue;
        };
        let Some((from_cols, to_cols)) = r
            .properties
            .get("fk_desc")
            .and_then(Value::as_str)
            .and_then(fk_columns)
        else {
            continue;
        };
        let (Some(from_columns), Some(to_columns)) = (
            as_fields(&from_cols, &built[a].dataset.fields),
            as_fields(&to_cols, &built[b].dataset.fields),
        ) else {
            continue;
        };
        let (from, to) = (&built[a].dataset.name, &built[b].dataset.name);
        if from_columns.len() != to_columns.len()
            || !seen.insert((
                fold(from),
                fold(to),
                from_columns.clone(),
                to_columns.clone(),
            ))
        {
            continue;
        }
        let mut ext = json!({"ekos_id": r.id.to_string()});
        for k in ["constraint", "origin"] {
            if let Some(v) = r.properties.get(k).filter(|v| !v.is_null()) {
                ext[k] = v.clone();
            }
        }
        out.push(Relationship {
            name: format!("{from}_{}_to_{to}", from_columns.join("_")),
            from: from.clone(),
            to: to.clone(),
            from_columns,
            to_columns,
            custom_extensions: vec![CustomExtension::ekos(&ext)],
            ..Default::default()
        });
    }
    out.sort_by(|x, y| x.name.cmp(&y.name));
    let mut used = BTreeSet::new();
    for r in &mut out {
        r.name = unique(&r.name, &mut used);
    }
    out
}

/// The ledger as one Ossie model. `evidence` gives an object's `path:line` references.
/// Fails only when no table or view is selected (the schema needs one dataset).
pub fn from_ledger(
    ledger: &LedgerView,
    opts: &BuildOptions,
    evidence: &dyn Fn(&KirObject) -> Vec<String>,
) -> Result<SemanticModel, String> {
    let mut tables: Vec<(usize, &KirObject)> = ledger
        .objects
        .iter()
        .enumerate()
        .filter(|(_, o)| o.kind == ObjectKind::Table || is_view(o))
        .filter(|(_, o)| selected(o, opts))
        .collect();
    if tables.is_empty() {
        return Err(
            "no tables or views to export — run the pipeline (`ekos build … commit`) first, or \
             widen `--database` / `--source`"
                .into(),
        );
    }
    tables.sort_by_key(|(_, o)| {
        (
            fold(&short_name(&o.name)),
            is_view(o),
            text(o, "origin"),
            o.id.to_string(),
        )
    });
    let mut used = BTreeSet::new();
    let mut built: Vec<Built> = tables
        .iter()
        .map(|(i, o)| base_dataset(o, *i, &mut used, evidence))
        .collect();
    let mut by_table: HashMap<String, Vec<usize>> = HashMap::new();
    let mut by_object: HashMap<KirId, usize> = HashMap::new();
    for (b, x) in built.iter().enumerate() {
        let o = &ledger.objects[x.object];
        by_table
            .entry(fold(&short_name(&o.name)))
            .or_default()
            .push(b);
        by_object.insert(o.id, b);
    }
    attach_enum_meanings(&mut built, &by_table, ledger.items, opts.admits);
    attach_concepts(&mut built, &by_table, ledger.items, opts.admits);
    attach_ingested(&mut built, &by_object, ledger.objects, ledger.relationships);
    let relationships = relationships_of(&built, &by_object, ledger.relationships);
    let datasets = built
        .into_iter()
        .map(|mut b| {
            let mut synonyms: Vec<String> = Vec::new();
            for s in b.synonyms {
                if !synonyms.contains(&s) {
                    synonyms.push(s);
                }
            }
            let instructions = (!b.instructions.is_empty()).then(|| b.instructions.join("\n"));
            b.dataset.ai_context = AiContext::from_parts(instructions, synonyms);
            b.dataset.custom_extensions = vec![CustomExtension::ekos(&Value::Object(b.ext))];
            b.dataset
        })
        .collect();
    let mut model_ext = json!({
        "export_status": opts.export_status,
        "ekos_version": opts.ekos_version,
    });
    if let Some(p) = &opts.project {
        model_ext["project"] = json!(p);
    }
    // The selection, so `ekos import ossie` rebuilds the same baseline.
    if let Some(d) = &opts.database {
        model_ext["database"] = json!(d);
    }
    if let Some(s) = &opts.source {
        model_ext["source"] = json!(s);
    }
    Ok(SemanticModel {
        version: SPEC_VERSION.into(),
        name: opts.name.clone(),
        description: Some(
            "Compiled by EKOS from observed schemas: every dataset is a table or view EKOS read, \
             every key and relationship one the schema states."
                .into(),
        ),
        ai_context: AiContext::from_parts(
            Some(format!(
                "Business meanings are included with review status `{}`; any marked \
                 \"unreviewed\" is a hypothesis EKOS derived from code, not a confirmed \
                 definition. Each element's EKOS extension names its ledger id and evidence.",
                opts.export_status
            )),
            Vec::new(),
        ),
        datasets,
        relationships,
        metrics: Vec::new(),
        custom_extensions: vec![CustomExtension::ekos(&model_ext)],
    })
}
