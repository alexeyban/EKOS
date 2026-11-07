//! The structural checks of Ossie's `validation/validate.py`, without its sqlglot SQL parse:
//! names unique (compared case-folded, as the validator does), keys and relationship columns
//! naming declared fields, matching arity, and one namespace for a dataset's fields and metrics.
//! The JSON Schema itself is checked by the tests against the vendored copy.

use crate::model::{Expression, SemanticModel};
use std::collections::{BTreeMap, BTreeSet};

/// One structural problem, with a JSON-pointer-like path to the element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub path: String,
    pub message: String,
}

fn fold(s: &str) -> String {
    s.to_lowercase()
}

fn duplicates<'a>(names: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut dup = BTreeSet::new();
    for n in names {
        if !seen.insert(fold(n)) {
            dup.insert(n.to_string());
        }
    }
    dup.into_iter().collect()
}

fn expression(e: &Expression, path: &str, out: &mut Vec<Problem>) {
    let sql_ok = e.sql.as_deref().is_some_and(|s| !s.trim().is_empty());
    if !sql_ok && e.dialects.is_empty() {
        out.push(Problem {
            path: path.into(),
            message: "an expression needs `sql` or `dialects`".into(),
        });
    }
    if e.sql.as_deref().is_some_and(|s| s.trim().is_empty())
        || e.dialects.iter().any(|d| d.expression.trim().is_empty())
    {
        out.push(Problem {
            path: path.into(),
            message: "an expression is empty".into(),
        });
    }
}

fn problem(out: &mut Vec<Problem>, path: String, message: String) {
    out.push(Problem { path, message });
}

/// One dataset's own checks: source, one namespace for fields and metrics, keys naming fields,
/// expressions present.
fn check_dataset(at: &str, d: &crate::model::Dataset, out: &mut Vec<Problem>) {
    let fields: BTreeSet<String> = d.fields.iter().map(|f| fold(&f.name)).collect();
    if d.source.trim().is_empty() {
        problem(
            out,
            format!("{at}/source"),
            "a dataset needs a `source`".into(),
        );
    }
    let names = d
        .fields
        .iter()
        .map(|f| f.name.as_str())
        .chain(d.metrics.iter().map(|m| m.name.as_str()));
    for n in duplicates(names) {
        problem(
            out,
            at.to_string(),
            format!(
                "`{n}` is declared twice among the fields and metrics of `{}`",
                d.name
            ),
        );
    }
    let keys = std::iter::once(("primary_key", &d.primary_key))
        .chain(d.unique_keys.iter().map(|k| ("unique_keys", k)));
    for (what, key) in keys {
        for c in key.iter().filter(|c| !fields.contains(&fold(c))) {
            problem(
                out,
                format!("{at}/{what}"),
                format!("`{c}` is not a field of `{}`", d.name),
            );
        }
    }
    if d.unique_keys.iter().any(Vec::is_empty) {
        problem(
            out,
            format!("{at}/unique_keys"),
            "an empty unique key".into(),
        );
    }
    for (j, f) in d.fields.iter().enumerate() {
        expression(&f.expression, &format!("{at}/fields/{j}/expression"), out);
    }
    for (j, mm) in d.metrics.iter().enumerate() {
        expression(&mm.expression, &format!("{at}/metrics/{j}/expression"), out);
    }
}

/// One relationship: equal, non-empty arity and both ends naming fields of existing datasets.
fn check_relationship(
    at: &str,
    r: &crate::model::Relationship,
    fields_of: &BTreeMap<String, BTreeSet<String>>,
    out: &mut Vec<Problem>,
) {
    if r.from_columns.is_empty() || r.from_columns.len() != r.to_columns.len() {
        problem(
            out,
            at.to_string(),
            format!(
                "`from_columns` ({}) and `to_columns` ({}) must be non-empty and equally long",
                r.from_columns.len(),
                r.to_columns.len()
            ),
        );
    }
    for (end, ds, cols) in [
        ("from", &r.from, &r.from_columns),
        ("to", &r.to, &r.to_columns),
    ] {
        let Some(f) = fields_of.get(&fold(ds)) else {
            problem(out, format!("{at}/{end}"), format!("no dataset `{ds}`"));
            continue;
        };
        for c in cols.iter().filter(|c| !f.contains(&fold(c))) {
            problem(
                out,
                format!("{at}/{end}_columns"),
                format!("`{c}` is not a field of `{ds}`"),
            );
        }
    }
}

/// Every structural problem of `m` (empty = valid as far as these checks go).
pub fn check(m: &SemanticModel) -> Vec<Problem> {
    let mut out = Vec::new();
    if m.name.trim().is_empty() {
        problem(&mut out, "/name".into(), "the model has no name".into());
    }
    if m.datasets.is_empty() {
        problem(
            &mut out,
            "/datasets".into(),
            "a model needs at least one dataset".into(),
        );
    }
    let groups = [
        (
            "/datasets",
            "dataset",
            m.datasets
                .iter()
                .map(|d| d.name.as_str())
                .collect::<Vec<_>>(),
        ),
        (
            "/relationships",
            "relationship",
            m.relationships.iter().map(|r| r.name.as_str()).collect(),
        ),
        (
            "/metrics",
            "metric",
            m.metrics.iter().map(|x| x.name.as_str()).collect(),
        ),
    ];
    for (path, what, names) in groups {
        for d in duplicates(names.into_iter()) {
            problem(
                &mut out,
                path.into(),
                format!("{what} name `{d}` is used twice"),
            );
        }
    }
    let fields_of: BTreeMap<String, BTreeSet<String>> = m
        .datasets
        .iter()
        .map(|d| {
            (
                fold(&d.name),
                d.fields.iter().map(|f| fold(&f.name)).collect(),
            )
        })
        .collect();
    for (i, d) in m.datasets.iter().enumerate() {
        check_dataset(&format!("/datasets/{i}"), d, &mut out);
    }
    for (i, r) in m.relationships.iter().enumerate() {
        check_relationship(&format!("/relationships/{i}"), r, &fields_of, &mut out);
    }
    for (i, mm) in m.metrics.iter().enumerate() {
        expression(
            &mm.expression,
            &format!("/metrics/{i}/expression"),
            &mut out,
        );
    }
    out
}
