//! RFC 0246: the ingested Ossie models as facts about the compiled tables — the one resolver every
//! consumer (text-to-SQL, dbt semantic layer, business semantics, ask, EKOS Migrate) reads.
//!
//! Only datasets linked to a table at commit (`DependsOn {link: "ossie_source"}`) count, and only
//! from the files the latest `recover` found. Metric expressions are rewritten over the tables by
//! the spec's name resolution (a field becomes its expression, `source.col` the column, a model
//! metric's `dataset.field` the table's expression), as an AST walk. What cannot be resolved is
//! listed in [`OssieKnowledge::skipped`] with the reason — never guessed.

use crate::build::{name_parts, plain_column};
use crate::kinds::{LINK, OSSIE_DATASET, OSSIE_METRIC};
use ekos_kir::{KirId, KirObject, KirRelationship, ObjectKind, RelationshipKind};
use serde::Serialize;
use serde_json::Value;
use sqlparser::ast::{Expr, Ident, visit_expressions_mut};
use sqlparser::dialect::GenericDialect;
use sqlparser::parser::Parser;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::ops::ControlFlow;

/// What the Ossie models state about one column.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ColumnFacts {
    pub column: String,
    pub description: Option<String>,
    pub synonyms: Vec<String>,
    pub is_time: bool,
    pub datatype: Option<String>,
}

/// What the Ossie models state about one table.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct TableFacts {
    pub table_id: KirId,
    /// The name SQL refers to it by (unquoted, dotted parts kept).
    pub table: String,
    pub synonyms: Vec<String>,
    pub description: Option<String>,
    pub instructions: Option<String>,
    pub primary_key: Vec<String>,
    pub unique_keys: Vec<Vec<String>>,
    pub columns: Vec<ColumnFacts>,
    /// `ossie:<path>` of every file that described it.
    pub origins: Vec<String>,
}

/// A join a model declares between two tables.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Join {
    pub name: String,
    pub from_table: String,
    pub from_columns: Vec<String>,
    pub to_table: String,
    pub to_columns: Vec<String>,
    pub origin: String,
}

/// A metric rewritten over the compiled tables.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Metric {
    pub ekos_id: String,
    pub name: String,
    /// `dataset` or `model`.
    pub scope: String,
    /// The SQL over the tables, columns qualified by table.
    pub sql: String,
    /// Every table the SQL reads, sorted.
    pub tables: Vec<String>,
    pub description: Option<String>,
    pub synonyms: Vec<String>,
    pub datatype: Option<String>,
    pub origin: String,
    pub line: Option<u64>,
}

/// Something a model states that could not be resolved, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Skipped {
    pub what: String,
    pub origin: String,
    pub reason: String,
}

/// Everything the current Ossie models state about the compiled tables.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct OssieKnowledge {
    pub tables: Vec<TableFacts>,
    pub joins: Vec<Join>,
    pub metrics: Vec<Metric>,
    pub skipped: Vec<Skipped>,
}

impl OssieKnowledge {
    /// The facts about a table, by ledger id.
    pub fn table(&self, id: &KirId) -> Option<&TableFacts> {
        self.tables.iter().find(|t| &t.table_id == id)
    }

    /// The metrics that read only `table` (case-insensitive).
    pub fn metrics_of<'a>(&'a self, table: &'a str) -> impl Iterator<Item = &'a Metric> + 'a {
        self.metrics
            .iter()
            .filter(move |m| m.tables.iter().any(|t| t.eq_ignore_ascii_case(table)))
    }

    /// Nothing is known.
    pub fn is_empty(&self) -> bool {
        self.tables.is_empty() && self.metrics.is_empty() && self.joins.is_empty()
    }
}

fn custom(o: &KirObject, kind: &str) -> bool {
    matches!(&o.kind, ObjectKind::Custom(k) if k == kind)
}

fn text(v: &Value, k: &str) -> Option<String> {
    v.get(k)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .map(str::to_string)
}

fn strings(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn prop<'a>(o: &'a KirObject, k: &str) -> Option<&'a Value> {
    o.properties.get(k)
}

fn fold(s: &str) -> String {
    s.to_lowercase()
}

fn origin(o: &KirObject) -> String {
    format!(
        "ossie:{}",
        prop(o, "path").and_then(Value::as_str).unwrap_or("?")
    )
}

/// One linked dataset, ready for resolution.
struct Linked<'a> {
    obj: &'a KirObject,
    table: &'a KirObject,
    /// The table's SQL name parts.
    parts: Vec<String>,
    /// field (case-folded) → its expression over the table, qualified.
    fields: HashMap<String, Expr>,
    /// field (case-folded) → the plain column it is, if it is one.
    plain: HashMap<String, String>,
    errors: Vec<String>,
}

fn parse(sql: &str) -> Result<Expr, String> {
    Parser::new(&GenericDialect {})
        .try_with_sql(sql)
        .and_then(|mut p| p.parse_expr())
        .map_err(|e| format!("does not parse: {e}"))
}

fn qualified(parts: &[String], col: &Ident) -> Expr {
    let mut ids: Vec<Ident> = parts.iter().map(|p| Ident::new(p.clone())).collect();
    ids.push(col.clone());
    Expr::CompoundIdentifier(ids)
}

/// A field expression with every bare column qualified by its table.
fn qualify_field(sql: &str, parts: &[String]) -> Result<Expr, String> {
    let mut e = parse(sql)?;
    let _ = visit_expressions_mut(&mut e, |x| {
        if let Expr::Identifier(id) = x {
            *x = qualified(parts, id);
        }
        ControlFlow::<()>::Continue(())
    });
    Ok(e)
}

fn linked<'a>(d: &'a KirObject, table: &'a KirObject) -> Linked<'a> {
    let parts = name_parts(&table.name);
    let mut l = Linked {
        obj: d,
        table,
        parts,
        fields: HashMap::new(),
        plain: HashMap::new(),
        errors: Vec::new(),
    };
    for f in prop(d, "fields")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let (Some(name), Some(sql)) = (text(f, "name"), text(f, "sql")) else {
            continue;
        };
        if let Some(c) = plain_column(&sql) {
            l.plain.insert(fold(&name), c);
        }
        match qualify_field(&sql, &l.parts) {
            Ok(e) => {
                l.fields.insert(fold(&name), e);
            }
            Err(e) => l.errors.push(format!("field `{name}` {e}")),
        }
    }
    l
}

/// Rewrite a metric expression by `resolve_ident`, collecting the tables it reads.
fn rewrite(
    sql: &str,
    mut resolve: impl FnMut(&[Ident]) -> Result<(Expr, Vec<String>), String>,
) -> Result<(String, Vec<String>), String> {
    let mut e = parse(sql)?;
    let mut err: Option<String> = None;
    let mut tables: BTreeSet<String> = BTreeSet::new();
    let _ = visit_expressions_mut(&mut e, |x| {
        let ids: Option<Vec<Ident>> = match x {
            Expr::Identifier(id) => Some(vec![id.clone()]),
            Expr::CompoundIdentifier(ids) => Some(ids.clone()),
            _ => None,
        };
        if let Some(ids) = ids {
            match resolve(&ids) {
                Ok((sub, ts)) => {
                    tables.extend(ts);
                    // Parentheses keep precedence, and only a compound expression needs them.
                    *x = match sub {
                        Expr::Identifier(_)
                        | Expr::CompoundIdentifier(_)
                        | Expr::Function(_)
                        | Expr::Value(_)
                        | Expr::Nested(_) => sub,
                        other => Expr::Nested(Box::new(other)),
                    };
                }
                Err(e) => {
                    err.get_or_insert(e);
                    return ControlFlow::Break(());
                }
            }
        }
        ControlFlow::Continue(())
    });
    match err {
        Some(e) => Err(e),
        None => Ok((e.to_string(), tables.into_iter().collect())),
    }
}

fn metric_record(m: &KirObject, scope: &str, sql: String, tables: Vec<String>) -> Metric {
    let p = |k: &str| prop(m, k).and_then(Value::as_str).map(str::to_string);
    Metric {
        ekos_id: m.id.to_string(),
        name: m.name.clone(),
        scope: scope.into(),
        sql,
        tables,
        description: p("description").filter(|s| !s.trim().is_empty()),
        synonyms: strings(prop(m, "synonyms")),
        datatype: p("datatype"),
        origin: origin(m),
        line: prop(m, "line").and_then(Value::as_u64),
    }
}

fn table_name(l: &Linked) -> String {
    l.parts.join(".")
}

/// A dataset-scoped metric: unqualified names are the dataset's fields, `source.col` its columns.
fn dataset_metric(
    l: &Linked,
    m: &KirObject,
    sql: &str,
    metrics: &BTreeSet<String>,
) -> Result<Metric, String> {
    let (sql, tables) = rewrite(sql, |ids| match ids {
        [one] => match l.fields.get(&fold(&one.value)) {
            Some(e) => Ok((e.clone(), vec![table_name(l)])),
            None if metrics.contains(&fold(&one.value)) => {
                Err(format!("refers to the metric `{}`", one.value))
            }
            None => Err(format!("`{}` is not a field of the dataset", one.value)),
        },
        [src, col]
            if src.value.eq_ignore_ascii_case("source") && !l.fields.contains_key("source") =>
        {
            Ok((qualified(&l.parts, col), vec![table_name(l)]))
        }
        _ => Err(format!(
            "`{}` is not a field of the dataset",
            ids.iter()
                .map(|i| i.value.as_str())
                .collect::<Vec<_>>()
                .join(".")
        )),
    })?;
    Ok(metric_record(m, "dataset", sql, tables))
}

/// A model-scoped metric: `dataset.field`, resolved through the dataset's table.
fn model_metric(
    by_dataset: &HashMap<String, &Linked>,
    m: &KirObject,
    sql: &str,
) -> Result<Metric, String> {
    let (sql, tables) = rewrite(sql, |ids| match ids {
        [ds, f] => {
            let l = by_dataset
                .get(&fold(&ds.value))
                .ok_or_else(|| format!("the dataset `{}` is not linked to a table", ds.value))?;
            let e = l
                .fields
                .get(&fold(&f.value))
                .ok_or_else(|| format!("`{}.{}` is not a field", ds.value, f.value))?;
            Ok((e.clone(), vec![table_name(l)]))
        }
        _ => Err(format!(
            "`{}` must be written `dataset.field`",
            ids.iter()
                .map(|i| i.value.as_str())
                .collect::<Vec<_>>()
                .join(".")
        )),
    })?;
    Ok(metric_record(m, "model", sql, tables))
}

fn table_facts(l: &Linked) -> TableFacts {
    let d = l.obj;
    let map_cols = |fields: Vec<String>| -> Option<Vec<String>> {
        fields
            .iter()
            .map(|f| l.plain.get(&fold(f)).cloned())
            .collect()
    };
    let columns = prop(d, "fields")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|f| {
            let name = text(f, "name")?;
            let column = l.plain.get(&fold(&name))?.clone();
            Some(ColumnFacts {
                column,
                description: text(f, "description"),
                synonyms: strings(f.get("synonyms")),
                is_time: f.get("is_time").and_then(Value::as_bool) == Some(true),
                datatype: text(f, "datatype"),
            })
        })
        .collect();
    let unique_keys = prop(d, "unique_keys")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|k| map_cols(strings(Some(k))))
        .filter(|k| !k.is_empty())
        .collect();
    let p = |k: &str| {
        prop(d, k)
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .map(str::to_string)
    };
    TableFacts {
        table_id: l.table.id,
        table: table_name(l),
        synonyms: strings(prop(d, "synonyms")),
        description: p("description"),
        instructions: p("instructions"),
        primary_key: map_cols(strings(prop(d, "primary_key"))).unwrap_or_default(),
        unique_keys,
        columns,
        origins: vec![origin(d)],
    }
}

/// Merge a second description of the same table: first wins for single values, lists union.
fn merge(into: &mut TableFacts, other: TableFacts) {
    let union = |a: &mut Vec<String>, b: Vec<String>| {
        for x in b {
            if !a.contains(&x) {
                a.push(x);
            }
        }
    };
    union(&mut into.synonyms, other.synonyms);
    union(&mut into.origins, other.origins);
    into.description = into.description.take().or(other.description);
    into.instructions = into.instructions.take().or(other.instructions);
    if into.primary_key.is_empty() {
        into.primary_key = other.primary_key;
    }
    for k in other.unique_keys {
        if !into.unique_keys.contains(&k) {
            into.unique_keys.push(k);
        }
    }
    for c in other.columns {
        match into
            .columns
            .iter_mut()
            .find(|x| fold(&x.column) == fold(&c.column))
        {
            Some(x) => {
                union(&mut x.synonyms, c.synonyms);
                x.description = x.description.take().or(c.description);
                x.is_time |= c.is_time;
                x.datatype = x.datatype.take().or(c.datatype);
            }
            None => into.columns.push(c),
        }
    }
}

/// Model key of a dataset: (file, model).
fn model_key(o: &KirObject) -> (String, String) {
    (
        prop(o, "path")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        prop(o, "model")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
    )
}

fn joins_of(
    l: &Linked,
    peers: &HashMap<String, &Linked>,
    out: &mut Vec<Join>,
    skipped: &mut Vec<Skipped>,
) {
    for r in prop(l.obj, "relationships")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let name = text(r, "name").unwrap_or_default();
        let to = text(r, "to").unwrap_or_default();
        let resolved = peers.get(&fold(&to)).and_then(|t| {
            let from_columns: Option<Vec<String>> = strings(r.get("from_columns"))
                .iter()
                .map(|f| l.plain.get(&fold(f)).cloned())
                .collect();
            let to_columns: Option<Vec<String>> = strings(r.get("to_columns"))
                .iter()
                .map(|f| t.plain.get(&fold(f)).cloned())
                .collect();
            Some((t, from_columns?, to_columns?))
        });
        match resolved {
            Some((t, fc, tc)) if !fc.is_empty() && fc.len() == tc.len() => out.push(Join {
                name,
                from_table: table_name(l),
                from_columns: fc,
                to_table: table_name(t),
                to_columns: tc,
                origin: origin(l.obj),
            }),
            _ => skipped.push(Skipped {
                what: format!("relationship `{name}`"),
                origin: origin(l.obj),
                reason: format!(
                    "`{to}` is not a linked dataset of the model, or a key field is computed"
                ),
            }),
        }
    }
}

/// Resolve the current Ossie models against the compiled tables. `current_files` is the
/// `ossie/current.json` list (`None`: no manifest yet, every file counts).
pub fn resolve(
    objects: &[KirObject],
    relationships: &[KirRelationship],
    current_files: Option<&BTreeSet<String>>,
) -> OssieKnowledge {
    let current = |o: &KirObject| {
        current_files
            .is_none_or(|f| f.contains(prop(o, "path").and_then(Value::as_str).unwrap_or("")))
    };
    let by_id: HashMap<KirId, &KirObject> = objects.iter().map(|o| (o.id, o)).collect();
    let mut links: Vec<(&KirObject, &KirObject)> = relationships
        .iter()
        .filter(|r| r.kind == RelationshipKind::DependsOn)
        .filter(|r| r.properties.get("link").and_then(Value::as_str) == Some(LINK))
        .filter_map(|r| {
            let d = by_id
                .get(&r.from)
                .filter(|d| custom(d, OSSIE_DATASET) && current(d))?;
            let t = by_id.get(&r.to)?;
            Some((*d, *t))
        })
        .collect();
    links.sort_by_key(|(d, t)| (d.id.to_string(), t.id.to_string()));
    links.dedup_by_key(|(d, t)| (d.id, t.id));
    let linked: Vec<Linked> = links.into_iter().map(|(d, t)| linked(d, t)).collect();

    let mut k = OssieKnowledge::default();
    for l in &linked {
        for e in &l.errors {
            k.skipped.push(Skipped {
                what: format!("dataset `{}`", l.obj.name),
                origin: origin(l.obj),
                reason: e.clone(),
            });
        }
    }

    // Tables, merged across files.
    let mut tables: BTreeMap<String, TableFacts> = BTreeMap::new();
    for l in &linked {
        let f = table_facts(l);
        match tables.get_mut(&l.table.id.to_string()) {
            Some(t) => merge(t, f),
            None => {
                tables.insert(l.table.id.to_string(), f);
            }
        }
    }
    k.tables = tables.into_values().collect();
    k.tables.sort_by(|a, b| a.table.cmp(&b.table));

    // Datasets of one model, by name — for model metrics and relationships.
    let mut models: BTreeMap<(String, String), HashMap<String, &Linked>> = BTreeMap::new();
    for l in &linked {
        models
            .entry(model_key(l.obj))
            .or_default()
            .insert(fold(&l.obj.name), l);
    }

    for l in &linked {
        let peers = &models[&model_key(l.obj)];
        joins_of(l, peers, &mut k.joins, &mut k.skipped);
    }

    // Metrics: dataset-scoped through `Contains` from their dataset, model-scoped by model key.
    let contains: HashMap<KirId, KirId> = relationships
        .iter()
        .filter(|r| r.kind == RelationshipKind::Contains)
        .filter(|r| by_id.get(&r.to).is_some_and(|m| custom(m, OSSIE_METRIC)))
        .map(|r| (r.to, r.from))
        .collect();
    let linked_by_dataset: HashMap<KirId, &Linked> = linked.iter().map(|l| (l.obj.id, l)).collect();
    let mut metrics: Vec<&KirObject> = objects
        .iter()
        .filter(|o| custom(o, OSSIE_METRIC) && current(o))
        .collect();
    metrics.sort_by_key(|m| (m.name.clone(), m.id.to_string()));
    for m in metrics {
        let Some(sql) = prop(m, "expression").and_then(|e| {
            serde_json::from_value::<crate::Expression>(e.clone())
                .ok()
                .and_then(|e| e.text().map(str::to_string))
        }) else {
            continue;
        };
        let scope = prop(m, "scope")
            .and_then(Value::as_str)
            .unwrap_or("dataset");
        let result = if scope == "model" {
            match models.get(&model_key(m)) {
                Some(peers) => model_metric(peers, m, &sql),
                None => Err("no dataset of its model is linked to a table".into()),
            }
        } else {
            match contains.get(&m.id).and_then(|d| linked_by_dataset.get(d)) {
                Some(l) => {
                    let names: BTreeSet<String> = objects
                        .iter()
                        .filter(|o| {
                            custom(o, OSSIE_METRIC) && contains.get(&o.id) == Some(&l.obj.id)
                        })
                        .map(|o| fold(&o.name))
                        .collect();
                    dataset_metric(l, m, &sql, &names)
                }
                None => Err("its dataset is not linked to a table".into()),
            }
        };
        match result {
            Ok(metric) => k.metrics.push(metric),
            Err(reason) => k.skipped.push(Skipped {
                what: format!("metric `{}`", m.name),
                origin: origin(m),
                reason,
            }),
        }
    }
    k.joins.sort();
    k.joins.dedup_by(|a, b| {
        (&a.from_table, &a.from_columns, &a.to_table, &a.to_columns)
            == (&b.from_table, &b.from_columns, &b.to_table, &b.to_columns)
    });
    k
}
