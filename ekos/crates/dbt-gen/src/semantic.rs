//! RFC 0246 — the ingested Apache Ossie models as a dbt semantic layer (MetricFlow YAML).
//!
//! Pure rendering over [`OssieKnowledge`]: no store, no LLM.
//!
//! | Ossie / EKOS | MetricFlow |
//! |---|---|
//! | a dataset linked to a table | a `semantic_models:` entry, `model: ref('<table>')` |
//! | a one-column primary key | entity `type: primary`, named after the table |
//! | a declared join or foreign key to such a table | entity `type: foreign` with that name |
//! | a field | a dimension: `time` (`time_granularity: day`) when it is a time axis, else `categorical` |
//! | `SUM/AVG/MIN/MAX/COUNT/COUNT DISTINCT` of one table's columns | a measure + a `simple` metric |
//! | one such aggregate divided by another | a `ratio` metric over two simple ones |
//!
//! Everything else — windows, `LAG`, nested aggregates, arithmetic on aggregates, a measure whose
//! model has no time dimension (MetricFlow requires one) — is listed in `skipped.md` with the
//! reason, never approximated.

use ekos_ossie::knowledge::{Metric, OssieKnowledge, TableFacts};
use serde_yaml::{Mapping, Value};
use sqlparser::ast::{
    BinaryOperator, DuplicateTreatment, Expr, FunctionArg, FunctionArgExpr, FunctionArguments,
};
use sqlparser::dialect::GenericDialect;
use sqlparser::parser::Parser;
use std::collections::BTreeMap;

/// RFC 0246: MetricFlow needs a daily time spine model in the project. A project that already
/// uses the semantic layer has one, so it is written only on request (`--with-time-spine`).
pub const TIME_SPINE_SQL: &str = "{{ config(materialized='table') }}\n\
with days as (\n    {{ dbt.date_spine('day', \"cast('2000-01-01' as date)\", \"cast('2040-01-01' as date)\") }}\n)\n\
select cast(date_day as date) as date_day from days\n";

/// The time spine model's YAML (dbt 1.9+ `time_spine:`).
pub const TIME_SPINE_YML: &str = "version: 2\nmodels:\n  - name: metricflow_time_spine\n    \
time_spine:\n      standard_granularity_column: date_day\n    columns:\n      - name: date_day\n        \
granularity: day\n";

/// The rendered files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticLayer {
    /// `semantic_models:` and `metrics:`.
    pub semantic_yml: String,
    /// `models:` with key tests.
    pub models_yml: String,
    /// What could not be expressed, and why.
    pub skipped_md: String,
    pub semantic_models: usize,
    pub metrics: usize,
    pub skipped: usize,
}

/// A join between two tables, one column pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyJoin {
    pub from_table: String,
    pub from_column: String,
    pub to_table: String,
    pub to_column: String,
}

fn s(v: impl Into<String>) -> Value {
    Value::String(v.into())
}

fn map(pairs: Vec<(&str, Value)>) -> Value {
    let mut m = Mapping::new();
    for (k, v) in pairs {
        if !v.is_null() {
            m.insert(s(k), v);
        }
    }
    Value::Mapping(m)
}

/// A dbt-safe name: lower-case letters, digits and `_`.
fn ident(name: &str) -> String {
    let out: String = name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    if out.starts_with(|c: char| c.is_ascii_digit()) {
        format!("m_{out}")
    } else {
        out
    }
}

/// The table name a dbt model is called by: the last dotted part.
fn model_name(table: &str) -> String {
    ident(table.rsplit('.').next().unwrap_or(table))
}

/// One aggregate over one table: `(agg, expression over that table's columns, table)`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Aggregate {
    agg: &'static str,
    expr: String,
    table: String,
}

/// The single table an expression's columns belong to, with the qualifiers stripped.
fn unqualify(e: &Expr) -> Option<(String, String)> {
    use sqlparser::ast::visit_expressions_mut;
    use std::ops::ControlFlow;
    let mut e = e.clone();
    let mut table: Option<String> = None;
    let mut mixed = false;
    let _ = visit_expressions_mut(&mut e, |x| {
        if let Expr::CompoundIdentifier(ids) = x
            && ids.len() >= 2
        {
            let t = ids[..ids.len() - 1]
                .iter()
                .map(|i| i.value.clone())
                .collect::<Vec<_>>()
                .join(".");
            match &table {
                Some(prev) if !prev.eq_ignore_ascii_case(&t) => mixed = true,
                _ => table = Some(t),
            }
            *x = Expr::Identifier(ids[ids.len() - 1].clone());
        }
        ControlFlow::<()>::Continue(())
    });
    if mixed {
        return None;
    }
    Some((table?, e.to_string()))
}

fn aggregate(e: &Expr) -> Option<Aggregate> {
    let Expr::Function(f) = e else { return None };
    if f.over.is_some() || f.filter.is_some() {
        return None;
    }
    let FunctionArguments::List(list) = &f.args else {
        return None;
    };
    let distinct = matches!(list.duplicate_treatment, Some(DuplicateTreatment::Distinct));
    let name = f.name.to_string().to_uppercase();
    let agg = match (name.as_str(), distinct) {
        ("SUM", false) => "sum",
        ("AVG", false) => "average",
        ("MIN", false) => "min",
        ("MAX", false) => "max",
        ("COUNT", false) => "count",
        ("COUNT", true) => "count_distinct",
        _ => return None,
    };
    let [FunctionArg::Unnamed(arg)] = list.args.as_slice() else {
        return None;
    };
    let FunctionArgExpr::Expr(inner) = arg else {
        return None;
    };
    // A nested aggregate is not one measure.
    if matches!(inner, Expr::Function(_)) && aggregate(inner).is_some() {
        return None;
    }
    let (table, expr) = unqualify(inner)?;
    Some(Aggregate { agg, expr, table })
}

/// What a metric becomes in MetricFlow.
enum Shape {
    Simple(Aggregate),
    Ratio(Aggregate, Aggregate),
}

fn shape(m: &Metric) -> Result<Shape, String> {
    let e = Parser::new(&GenericDialect {})
        .try_with_sql(&m.sql)
        .and_then(|mut p| p.parse_expr())
        .map_err(|e| format!("does not parse: {e}"))?;
    let strip = |e: &Expr| match e {
        Expr::Nested(inner) => (**inner).clone(),
        other => other.clone(),
    };
    if let Some(a) = aggregate(&e) {
        return Ok(Shape::Simple(a));
    }
    if let Expr::BinaryOp {
        left,
        op: BinaryOperator::Divide,
        right,
    } = &e
        && let (Some(n), Some(d)) = (aggregate(&strip(left)), aggregate(&strip(right)))
    {
        return Ok(Shape::Ratio(n, d));
    }
    Err(
        "not one aggregate of one table, nor a ratio of two: windows, LAG, nested aggregates \
         and arithmetic have no MetricFlow form EKOS writes"
            .into(),
    )
}

/// Per table: its facts, its time dimension, its primary entity.
struct Model<'a> {
    facts: &'a TableFacts,
    name: String,
    time: Option<String>,
    measures: Vec<Value>,
}

type Models<'a> = BTreeMap<String, Model<'a>>;
/// What could not be exported: (what, where, why).
type Skipped = Vec<(String, String, String)>;

/// A measure on its table's model; MetricFlow needs that model to have a time dimension.
fn add_measure(a: &Aggregate, name: &str, models: &mut Models) -> Result<(), String> {
    let Some(m) = models.get_mut(&a.table.to_lowercase()) else {
        return Err(format!(
            "{} is not a dataset of the semantic model",
            a.table
        ));
    };
    if m.time.is_none() {
        return Err(format!(
            "MetricFlow needs a time dimension on {}, the measure's own model; mark a date \
             field `is_time` in the Ossie dataset",
            m.name
        ));
    }
    m.measures.push(map(vec![
        ("name", s(name)),
        ("agg", s(a.agg)),
        ("expr", s(a.expr.clone())),
    ]));
    Ok(())
}

fn simple_metric(name: &str, label: Value, description: Value) -> Value {
    map(vec![
        ("name", s(name)),
        ("label", label),
        ("description", description),
        ("type", s("simple")),
        ("type_params", map(vec![("measure", s(name))])),
    ])
}

/// One Ossie metric as MetricFlow metrics (a ratio brings its two simple parts), its measures
/// added to their models.
fn metric_yaml(metric: &Metric, models: &mut Models) -> Result<Vec<Value>, String> {
    let name = ident(&metric.name);
    let label = s(metric.name.clone());
    let description = metric.description.clone().map(s).unwrap_or(Value::Null);
    match shape(metric)? {
        Shape::Simple(a) => {
            add_measure(&a, &name, models)?;
            Ok(vec![simple_metric(&name, label, description)])
        }
        Shape::Ratio(n, d) => {
            let (nn, dn) = (format!("{name}_numerator"), format!("{name}_denominator"));
            add_measure(&n, &nn, models)?;
            add_measure(&d, &dn, models)?;
            Ok(vec![
                simple_metric(&nn, Value::Null, Value::Null),
                simple_metric(&dn, Value::Null, Value::Null),
                map(vec![
                    ("name", s(name)),
                    ("label", label),
                    ("description", description),
                    ("type", s("ratio")),
                    (
                        "type_params",
                        map(vec![("numerator", s(nn)), ("denominator", s(dn))]),
                    ),
                ]),
            ])
        }
    }
}

/// A model's primary entity (its one-column key) and a foreign entity per join to another
/// model's primary entity.
fn entities(m: &Model, models: &Models, joins: &[KeyJoin]) -> Vec<Value> {
    let t = m.facts;
    let mut out = Vec::new();
    if let [pk] = t.primary_key.as_slice() {
        out.push(map(vec![
            ("name", s(m.name.clone())),
            ("type", s("primary")),
            ("expr", s(pk.clone())),
        ]));
    }
    let mut foreign: Vec<(String, String)> = joins
        .iter()
        .filter(|j| j.from_table.eq_ignore_ascii_case(&t.table))
        .filter_map(|j| {
            let target = models.get(&j.to_table.to_lowercase())?;
            let [pk] = target.facts.primary_key.as_slice() else {
                return None;
            };
            pk.eq_ignore_ascii_case(&j.to_column)
                .then(|| (target.name.clone(), j.from_column.clone()))
        })
        .filter(|(entity, _)| *entity != m.name)
        .collect();
    foreign.sort();
    foreign.dedup_by(|a, b| a.0 == b.0);
    for (entity, col) in foreign {
        out.push(map(vec![
            ("name", s(entity)),
            ("type", s("foreign")),
            ("expr", s(col)),
        ]));
    }
    out
}

fn dimension(c: &ekos_ossie::knowledge::ColumnFacts) -> Value {
    let mut pairs = vec![("name", s(ident(&c.column)))];
    if c.is_time {
        pairs.push(("type", s("time")));
        pairs.push(("type_params", map(vec![("time_granularity", s("day"))])));
    } else {
        pairs.push(("type", s("categorical")));
    }
    if ident(&c.column) != c.column {
        pairs.push(("expr", s(c.column.clone())));
    }
    if let Some(d) = &c.description {
        pairs.push(("description", s(d.clone())));
    }
    map(pairs)
}

fn semantic_model(m: &Model, models: &Models, joins: &[KeyJoin]) -> Value {
    let t = m.facts;
    let pk = match t.primary_key.as_slice() {
        [pk] => Some(pk.as_str()),
        _ => None,
    };
    let dimensions: Vec<Value> = t
        .columns
        .iter()
        .filter(|c| pk != Some(c.column.as_str()))
        .map(dimension)
        .collect();
    let entities = entities(m, models, joins);
    let mut pairs = vec![
        ("name", s(m.name.clone())),
        (
            "description",
            t.description.clone().map(s).unwrap_or(Value::Null),
        ),
        ("model", s(format!("ref('{}')", m.name))),
    ];
    if pk.is_none() {
        // No one-column key (none, or a composite one): MetricFlow still needs the model's grain
        // named when it has dimensions.
        pairs.push(("primary_entity", s(m.name.clone())));
    }
    if let Some(time) = &m.time {
        pairs.push((
            "defaults",
            map(vec![("agg_time_dimension", s(ident(time)))]),
        ));
    }
    for (key, list) in [
        ("entities", entities),
        ("dimensions", dimensions),
        ("measures", m.measures.clone()),
    ] {
        if !list.is_empty() {
            pairs.push((key, Value::Sequence(list)));
        }
    }
    map(pairs)
}

/// A model's key tests: a one-column key is unique and not null. `None` without such a key.
fn key_tests(m: &Model) -> Option<Value> {
    let t = m.facts;
    let mut cols: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for k in std::iter::once(&t.primary_key).chain(&t.unique_keys) {
        if let [col] = k.as_slice() {
            cols.entry(col.clone())
                .or_insert_with(|| vec![s("unique"), s("not_null")]);
        }
    }
    (!cols.is_empty()).then(|| {
        map(vec![
            ("name", s(m.name.clone())),
            (
                "columns",
                Value::Sequence(
                    cols.into_iter()
                        .map(|(c, t)| map(vec![("name", s(c)), ("data_tests", Value::Sequence(t))]))
                        .collect(),
                ),
            ),
        ])
    })
}

fn skipped_markdown(skipped: &Skipped) -> String {
    let mut md = String::from("# Not exported to MetricFlow\n\n");
    if skipped.is_empty() {
        md.push_str("Nothing: every metric and dataset was exported.\n");
        return md;
    }
    md.push_str("| What | Where | Why |\n|---|---|---|\n");
    for (what, origin, why) in skipped {
        md.push_str(&format!(
            "| {what} | {origin} | {} |\n",
            why.replace('|', "\\|")
        ));
    }
    md
}

/// The schema's foreign keys and the models' declared joins, one column pair each.
fn all_joins(k: &OssieKnowledge, fks: &[KeyJoin]) -> Vec<KeyJoin> {
    let mut joins: Vec<KeyJoin> = fks.to_vec();
    for j in &k.joins {
        for (fc, tc) in j.from_columns.iter().zip(&j.to_columns) {
            joins.push(KeyJoin {
                from_table: j.from_table.clone(),
                from_column: fc.clone(),
                to_table: j.to_table.clone(),
                to_column: tc.clone(),
            });
        }
    }
    joins
}

/// The MetricFlow semantic layer of the current Ossie models. `fks` are the schema's foreign
/// keys (the models' declared joins are added), for foreign entities.
pub fn render_semantic_layer(k: &OssieKnowledge, fks: &[KeyJoin]) -> SemanticLayer {
    let mut models: Models = k
        .tables
        .iter()
        .map(|t| {
            let time = t
                .columns
                .iter()
                .find(|c| c.is_time)
                .map(|c| c.column.clone());
            let model = Model {
                facts: t,
                name: model_name(&t.table),
                time,
                measures: Vec::new(),
            };
            (t.table.to_lowercase(), model)
        })
        .collect();
    let mut skipped: Skipped = k
        .skipped
        .iter()
        .map(|s| (s.what.clone(), s.origin.clone(), s.reason.clone()))
        .collect();
    let mut metrics: Vec<Value> = Vec::new();
    for metric in &k.metrics {
        match metric_yaml(metric, &mut models) {
            Ok(v) => metrics.extend(v),
            Err(reason) => skipped.push((
                format!("metric `{}`", metric.name),
                metric.origin.clone(),
                reason,
            )),
        }
    }
    let joins = all_joins(k, fks);
    let semantic_models: Vec<Value> = models
        .values()
        .map(|m| semantic_model(m, &models, &joins))
        .collect();
    let tests: Vec<Value> = models.values().filter_map(key_tests).collect();

    let mut origins: Vec<String> = k.tables.iter().flat_map(|t| t.origins.clone()).collect();
    origins.sort();
    origins.dedup();
    let header = format!(
        "# dbt semantic layer generated by EKOS (RFC 0246) from Apache Ossie models:\n# {}\n\
         # Regenerate with `ekos export dbt-semantic`; skipped metrics are listed in skipped.md.\n\
         # MetricFlow needs a daily time spine model in the project (`--with-time-spine` adds one).\n",
        origins.join(", ")
    );
    let (n_models, n_metrics) = (semantic_models.len(), metrics.len());
    let mut semantic = vec![("semantic_models", Value::Sequence(semantic_models))];
    if !metrics.is_empty() {
        semantic.push(("metrics", Value::Sequence(metrics)));
    }
    let yaml = |v: Value| serde_yaml::to_string(&v).expect("YAML values serialise");
    SemanticLayer {
        semantic_yml: format!("{header}{}", yaml(map(semantic))),
        models_yml: format!(
            "{header}{}",
            yaml(map(vec![
                ("version", Value::Number(2.into())),
                ("models", Value::Sequence(tests)),
            ]))
        ),
        skipped_md: skipped_markdown(&skipped),
        semantic_models: n_models,
        metrics: n_metrics,
        skipped: skipped.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_kir::KirId;
    use ekos_ossie::knowledge::{ColumnFacts, Join};

    fn col(name: &str, time: bool) -> ColumnFacts {
        ColumnFacts {
            column: name.into(),
            is_time: time,
            ..Default::default()
        }
    }

    fn metric(name: &str, sql: &str) -> Metric {
        Metric {
            ekos_id: name.into(),
            name: name.into(),
            scope: "model".into(),
            sql: sql.into(),
            tables: vec![],
            description: Some(format!("{name} described")),
            synonyms: vec![],
            datatype: None,
            origin: "ossie:m.yaml".into(),
            line: None,
        }
    }

    fn knowledge() -> OssieKnowledge {
        OssieKnowledge {
            tables: vec![
                TableFacts {
                    table_id: KirId::new(),
                    table: "orders".into(),
                    description: Some("One row per order".into()),
                    primary_key: vec!["order_id".into()],
                    columns: vec![
                        col("order_id", false),
                        col("customer_id", false),
                        col("ordered_at", true),
                        col("amount", false),
                    ],
                    origins: vec!["ossie:m.yaml".into()],
                    ..Default::default()
                },
                TableFacts {
                    table_id: KirId::new(),
                    table: "customers".into(),
                    primary_key: vec!["id".into()],
                    columns: vec![col("id", false), col("region", false)],
                    origins: vec!["ossie:m.yaml".into()],
                    ..Default::default()
                },
            ],
            joins: vec![Join {
                name: "o_c".into(),
                from_table: "orders".into(),
                from_columns: vec!["customer_id".into()],
                to_table: "customers".into(),
                to_columns: vec!["id".into()],
                origin: "ossie:m.yaml".into(),
            }],
            metrics: vec![
                metric("revenue", "SUM(orders.amount)"),
                metric("buyers", "COUNT(DISTINCT orders.customer_id)"),
                metric("avg_order", "SUM(orders.amount) / COUNT(orders.order_id)"),
                metric(
                    "running",
                    "SUM(SUM(orders.amount)) OVER (ORDER BY orders.ordered_at)",
                ),
                metric("customers_n", "COUNT(customers.id)"),
            ],
            skipped: vec![],
        }
    }

    #[test]
    fn simple_and_ratio_metrics_become_measures_others_are_skipped_with_reasons() {
        let out = render_semantic_layer(&knowledge(), &[]);
        let y: Value = serde_yaml::from_str(&out.semantic_yml).unwrap();
        let orders = &y["semantic_models"][0];
        assert_eq!(orders["name"], s("customers"), "models are sorted");
        let orders = y["semantic_models"]
            .as_sequence()
            .unwrap()
            .iter()
            .find(|m| m["name"] == s("orders"))
            .unwrap();
        assert_eq!(orders["model"], s("ref('orders')"));
        assert_eq!(orders["defaults"]["agg_time_dimension"], s("ordered_at"));
        let entities: Vec<(String, String)> = orders["entities"]
            .as_sequence()
            .unwrap()
            .iter()
            .map(|e| {
                (
                    e["name"].as_str().unwrap().into(),
                    e["type"].as_str().unwrap().into(),
                )
            })
            .collect();
        assert_eq!(
            entities,
            [
                ("orders".into(), "primary".into()),
                ("customers".into(), "foreign".into())
            ]
        );
        let measures: Vec<(&str, &str, &str)> = orders["measures"]
            .as_sequence()
            .unwrap()
            .iter()
            .map(|m| {
                (
                    m["name"].as_str().unwrap(),
                    m["agg"].as_str().unwrap(),
                    m["expr"].as_str().unwrap(),
                )
            })
            .collect();
        assert_eq!(
            measures,
            [
                ("revenue", "sum", "amount"),
                ("buyers", "count_distinct", "customer_id"),
                ("avg_order_numerator", "sum", "amount"),
                ("avg_order_denominator", "count", "order_id"),
            ]
        );
        let metric_types: Vec<(&str, &str)> = y["metrics"]
            .as_sequence()
            .unwrap()
            .iter()
            .map(|m| (m["name"].as_str().unwrap(), m["type"].as_str().unwrap()))
            .collect();
        assert!(metric_types.contains(&("avg_order", "ratio")));
        assert!(metric_types.contains(&("revenue", "simple")));
        // A window function, and a measure on a model with no time dimension, are skipped.
        assert_eq!(out.skipped, 2, "{}", out.skipped_md);
        assert!(out.skipped_md.contains("metric `running`"));
        assert!(out.skipped_md.contains("time dimension on customers"));
        // Keys become tests.
        let m: Value = serde_yaml::from_str(&out.models_yml).unwrap();
        assert_eq!(m["models"][0]["columns"][0]["data_tests"][0], s("unique"));
        // Deterministic.
        assert_eq!(render_semantic_layer(&knowledge(), &[]), out);
    }
}
