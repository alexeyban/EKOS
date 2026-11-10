//! RFC 0254 §3.4 — where recovered logic assumes something the data contradicts.
//!
//! Pure: reads the predicate sites code carries (RFC 0170 `PredicateSite`s on views, routine
//! statements, Pentaho steps, …), the measured profile signals (RFC 0252, counts only) and the
//! live tables' column profiles, and returns RFC 0172 `ConflictingEvidence` of type
//! `code_vs_data`, each with a `check`, the measured value and both claims — the code site and the
//! measurement. Nothing here queries a database.
//!
//! | check | when |
//! |---|---|
//! | `predicate_misses_values` | the branches of a `CASE` (or a Pentaho step's `IN` list) over a column enumerate values, and rows holding *other* values reach `min_share` |
//! | `dead_branch` | a branch's value is in no row, or is `dead` (not seen for `dead_after` months) |
//! | `range_assumption` | a comparison constant lies outside the column's measured min/max |
//! | `lookup_not_unique` | code joins a column to one that is not unique on the looked-up side (the join fans out) |
//!
//! Claim texts carry no counts (a re-profile must not reopen a decided conflict); the counts are
//! in `measured`.

use crate::conflicts::{self, Claim};
use crate::data_usage::{ColumnSignal, DataSignals};
use ekos_kir::predicates::{Clause, PredicateSite};
use ekos_kir::{KirGraph, KirObject, ObjectKind};
use ekos_observation_sdk::signals::{literal_text, months_between};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

/// `conflict_type` of every finding here.
pub const CODE_VS_DATA: &str = "code_vs_data";

/// Thresholds (RFC 0254 `[code_vs_data]`).
#[derive(Debug, Clone, Copy)]
pub struct Settings {
    /// Share of a column's rows outside an enumeration before it is a finding.
    pub min_share: f64,
}

impl Default for Settings {
    fn default() -> Self {
        Self { min_share: 0.01 }
    }
}

/// Kinds that carry predicate sites.
const CARRIERS: &[&str] = &[
    "View",
    "ProcedureStatement",
    "Procedure",
    "TransformNode",
    "PerlSymbol",
    "PerlPackage",
];

fn custom(o: &KirObject) -> Option<&str> {
    match &o.kind {
        ObjectKind::Custom(k) => Some(k),
        _ => None,
    }
}

fn s<'a>(o: &'a KirObject, k: &str) -> &'a str {
    o.properties.get(k).and_then(Value::as_str).unwrap_or("")
}

fn tail(n: &str) -> String {
    n.rsplit('.').next().unwrap_or(n).to_lowercase()
}

/// One site with its carrier and the measured column it resolves to.
struct Hit<'a> {
    carrier: &'a KirObject,
    site: PredicateSite,
    signal: &'a ColumnSignal,
}

/// The measured column a site is about: its relation's table, or the only scoped relation that has
/// a measured column of that name. `None` when ambiguous or unmeasured.
fn resolve<'a>(
    site: &PredicateSite,
    by_col: &BTreeMap<(String, String), Vec<&'a ColumnSignal>>,
) -> Vec<&'a ColumnSignal> {
    let col = site.column.to_lowercase();
    let candidates: Vec<String> = match &site.relation {
        Some(r) => vec![tail(r)],
        None => site.scope.iter().map(|r| tail(r)).collect(),
    };
    let found: Vec<&Vec<&ColumnSignal>> = candidates
        .iter()
        .filter_map(|t| by_col.get(&(t.clone(), col.clone())))
        .collect();
    match found.as_slice() {
        [one] => one.to_vec(),
        _ => Vec::new(),
    }
}

fn hits<'a>(objects: &'a [KirObject], signals: &'a DataSignals) -> Vec<Hit<'a>> {
    let mut by_col: BTreeMap<(String, String), Vec<&ColumnSignal>> = BTreeMap::new();
    for c in &signals.columns {
        by_col
            .entry((tail(&c.table), c.column.to_lowercase()))
            .or_default()
            .push(c);
    }
    let mut out = Vec::new();
    for o in objects
        .iter()
        .filter(|o| custom(o).is_some_and(|k| CARRIERS.contains(&k)))
    {
        let Some(Value::Array(preds)) = o.properties.get("predicates") else {
            continue;
        };
        for v in preds {
            let Ok(site) = serde_json::from_value::<PredicateSite>(v.clone()) else {
                continue;
            };
            for signal in resolve(&site, &by_col) {
                out.push(Hit {
                    carrier: o,
                    site: site.clone(),
                    signal,
                });
            }
        }
    }
    out
}

/// The live table a signal measured.
fn table_of<'a>(objects: &'a [KirObject], c: &ColumnSignal) -> Option<&'a KirObject> {
    objects.iter().find(|o| {
        o.kind == ObjectKind::Table
            && s(o, "origin") == "live"
            && s(o, "database") == c.database
            && s(o, "owner") == c.owner
            && o.name == c.table
    })
}

fn code_claim(carrier: &KirObject, line: u64, what: String) -> Claim {
    let path = match s(carrier, "source_path") {
        "" => carrier.name.clone(),
        p => p.to_string(),
    };
    Claim {
        value: json!(what),
        path,
        line: (line > 0).then_some(line as u32),
        source: custom(carrier).unwrap_or("code").to_string(),
    }
}

fn data_claim(c: &ColumnSignal, what: String) -> Claim {
    Claim {
        value: json!(what),
        path: format!(
            "profile://{}/{}.{}.{}",
            c.database, c.owner, c.table, c.column
        ),
        line: None,
        source: "profile".into(),
    }
}

/// One finding as a graph to append.
fn finding(
    subject: &KirObject,
    attribute: &str,
    check: &str,
    claims: Vec<Claim>,
    measured: Value,
) -> KirGraph {
    let mut g = conflicts::conflict(subject, attribute, CODE_VS_DATA, claims, None);
    if let Some(o) = g.objects.first_mut() {
        o.properties.insert("check".into(), json!(check));
        o.properties.insert("measured".into(), measured);
    }
    g
}

fn share(n: i64, of: i64) -> f64 {
    if of <= 0 {
        0.0
    } else {
        ((n as f64 / of as f64) * 10_000.0).round() / 10_000.0
    }
}

/// (carrier id, database:owner.table, column).
type GroupKey = (String, String, String);

/// `predicate_misses_values` per (carrier, column): the values its CASE branches (or a Pentaho
/// step's IN lists) enumerate, against the rows holding anything else.
fn enumerations(objects: &[KirObject], hits: &[Hit<'_>], cfg: Settings, out: &mut Vec<KirGraph>) {
    let mut groups: BTreeMap<GroupKey, (Vec<&Hit<'_>>, BTreeSet<String>)> = BTreeMap::new();
    for h in hits {
        let enumerates = match h.site.clause {
            Clause::Case => h.site.op == "in",
            Clause::Where => {
                custom(h.carrier) == Some("TransformNode")
                    && h.site.op == "in"
                    && h.site.values.len() >= 2
            }
            _ => false,
        };
        if !enumerates {
            continue;
        }
        let key = (
            h.carrier.id.to_string(),
            format!(
                "{}:{}.{}",
                h.signal.database, h.signal.owner, h.signal.table
            ),
            h.signal.column.clone(),
        );
        let e = groups.entry(key).or_default();
        e.0.push(h);
        e.1.extend(h.site.values.iter().cloned());
    }
    for (hs, values) in groups.values() {
        let h = hs[0];
        let c = h.signal;
        let handled: i64 = values
            .iter()
            .filter_map(|v| c.coverage.literals.get(v).map(|u| u.rows))
            .sum();
        let other = (c.coverage.non_null - handled).max(0);
        let sh = share(other, c.coverage.non_null);
        if sh < cfg.min_share {
            continue;
        }
        let Some(subject) = table_of(objects, c) else {
            continue;
        };
        let handled_list = values.iter().cloned().collect::<Vec<_>>().join(", ");
        out.push(finding(
            subject,
            &format!("code_vs_data:predicate_misses_values:{}:{}", h.carrier.name, c.column),
            "predicate_misses_values",
            vec![
                code_claim(h.carrier, h.site.line, format!("{} handles {} = {handled_list}", h.carrier.name, c.column)),
                data_claim(c, format!("{}.{} also holds values none of these branches handles", c.table, c.column)),
            ],
            json!({"rows_unhandled": other, "share": sh, "column_rows": c.coverage.non_null, "handled": values}),
        ));
    }
}

/// `dead_branch`: a branch value no row holds, or one not seen for `dead_after` months.
fn dead_branches(
    objects: &[KirObject],
    hits: &[Hit<'_>],
    dead_after: i64,
    out: &mut Vec<KirGraph>,
) {
    let mut seen: BTreeSet<(String, String, String)> = BTreeSet::new();
    for h in hits
        .iter()
        .filter(|h| matches!(h.site.clause, Clause::Case | Clause::Condition))
    {
        if h.site.op != "in" {
            continue;
        }
        let c = h.signal;
        for v in &h.site.values {
            let Some(u) = c.coverage.literals.get(v) else {
                continue;
            };
            let dead = u.rows > 0
                && matches!((&u.last_seen, &c.coverage.latest), (Some(l), Some(t)) if months_between(l, t).is_some_and(|m| m > dead_after));
            if u.rows > 0 && !dead {
                continue;
            }
            if !seen.insert((
                h.carrier.id.to_string(),
                c.database.clone(),
                format!("{}={v}", c.column),
            )) {
                continue;
            }
            let Some(subject) = table_of(objects, c) else {
                continue;
            };
            let why = if u.rows == 0 {
                "no row holds it".to_string()
            } else {
                format!("no row since {}", u.last_seen.clone().unwrap_or_default())
            };
            out.push(finding(
                subject,
                &format!("code_vs_data:dead_branch:{}:{}={v}", h.carrier.name, c.column),
                "dead_branch",
                vec![
                    code_claim(h.carrier, h.site.line, format!("{} has a branch for {} = {v}", h.carrier.name, c.column)),
                    data_claim(c, format!("{}.{} = {}: {}", c.table, c.column, literal_text(v), if u.rows == 0 { "never in the data" } else { "not seen recently" })),
                ],
                json!({"rows": u.rows, "last_seen": u.last_seen, "latest_in_column": c.coverage.latest, "why": why}),
            ));
        }
    }
}

/// A literal as a number or an ISO date, for comparing with a profile bound.
fn comparable(v: &str) -> Option<(f64, Option<String>)> {
    let t = literal_text(v);
    if let Ok(n) = t.parse::<f64>() {
        return Some((n, None));
    }
    let date = t.get(..10).filter(|d| {
        d.len() == 10
            && d.as_bytes()[4] == b'-'
            && d.as_bytes()[7] == b'-'
            && d.chars().filter(char::is_ascii_digit).count() == 8
    })?;
    Some((0.0, Some(date.to_string())))
}

/// Whether `v` lies outside `[min, max]` of a column profile.
fn outside(v: &str, min: &Value, max: &Value) -> bool {
    let Some((n, date)) = comparable(v) else {
        return false;
    };
    match date {
        Some(d) => {
            let (lo, hi) = (min.as_str().unwrap_or(""), max.as_str().unwrap_or(""));
            !lo.is_empty()
                && !hi.is_empty()
                && (d.as_str() < &lo[..lo.len().min(10)] || d.as_str() > &hi[..hi.len().min(10)])
        }
        None => {
            let num = |x: &Value| {
                x.as_f64()
                    .or_else(|| x.as_str().and_then(|s| s.parse().ok()))
            };
            matches!((num(min), num(max)), (Some(lo), Some(hi)) if n < lo || n > hi)
        }
    }
}

/// `range_assumption`: a comparison constant outside the measured bounds (a P2 profile).
fn ranges(objects: &[KirObject], hits: &[Hit<'_>], out: &mut Vec<KirGraph>) {
    let mut seen = BTreeSet::new();
    for h in hits {
        if !matches!(
            h.site.op.as_str(),
            "<" | "<=" | ">" | ">=" | "between" | "not_between"
        ) {
            continue;
        }
        let c = h.signal;
        let Some(t) = table_of(objects, c) else {
            continue;
        };
        let Some(col) = t
            .properties
            .get("columns")
            .and_then(Value::as_array)
            .and_then(|cs| {
                cs.iter()
                    .find(|x| x["name"].as_str() == Some(c.column.as_str()))
            })
        else {
            continue;
        };
        let (min, max) = (&col["profile"]["min"], &col["profile"]["max"]);
        if min.is_null() || max.is_null() {
            continue;
        }
        let Some(v) = h.site.values.iter().find(|v| outside(v, min, max)) else {
            continue;
        };
        if !seen.insert((h.carrier.id.to_string(), c.column.clone(), v.clone())) {
            continue;
        }
        out.push(finding(
            t,
            &format!(
                "code_vs_data:range_assumption:{}:{}{}{v}",
                h.carrier.name, c.column, h.site.op
            ),
            "range_assumption",
            vec![
                code_claim(
                    h.carrier,
                    h.site.line,
                    format!("{} compares {} {} {v}", h.carrier.name, c.column, h.site.op),
                ),
                data_claim(
                    c,
                    format!("{}.{} never reaches that constant", c.table, c.column),
                ),
            ],
            json!({"constant": v, "op": h.site.op, "min": min, "max": max}),
        ));
    }
}

/// `lookup_not_unique`: a code-seeded reference whose looked-up side is not unique.
fn lookups(
    objects: &[KirObject],
    current_refs: Option<&BTreeSet<String>>,
    out: &mut Vec<KirGraph>,
) {
    for o in objects.iter().filter(|o| custom(o) == Some("FkCandidate")) {
        if current_refs.is_some_and(|c| !c.contains(&o.id.to_string())) {
            continue;
        }
        let code = matches!(s(o, "seed"), "code" | "both");
        let unique = o.properties.get("parent_unique") == Some(&json!(true));
        if !code || unique {
            continue;
        }
        let Some(subject) = objects.iter().find(|t| {
            t.kind == ObjectKind::Table
                && s(t, "origin") == "live"
                && s(t, "database") == s(o, "database")
                && s(t, "owner") == s(o, "parent_owner")
                && t.name == s(o, "parent_table")
        }) else {
            continue;
        };
        let sources: Vec<String> = o
            .properties
            .get("code_sources")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        let (path, line) = sources
            .first()
            .and_then(|p| p.rsplit_once('#'))
            .map(|(p, l)| (p.to_string(), l.parse().ok()))
            .unwrap_or_else(|| (sources.first().cloned().unwrap_or_default(), None));
        out.push(finding(
            subject,
            &format!("code_vs_data:lookup_not_unique:{}", o.name),
            "lookup_not_unique",
            vec![
                Claim {
                    value: json!(format!("code joins {} as a lookup", o.name)),
                    path,
                    line,
                    source: "code".into(),
                },
                Claim {
                    value: json!(format!(
                        "{}.{} is not unique: the join can multiply rows",
                        s(o, "parent_table"),
                        s(o, "parent_column")
                    )),
                    path: format!(
                        "profile://{}/{}.{}.{}",
                        s(o, "database"),
                        s(o, "parent_owner"),
                        s(o, "parent_table"),
                        s(o, "parent_column")
                    ),
                    line: None,
                    source: "profile".into(),
                },
            ],
            json!({"verdict": o.properties.get("verdict"), "code_sources": sources}),
        ));
    }
}

/// Every `code_vs_data` finding. `current_refs` are the latest discovery's `FkCandidate` ids.
pub fn detect(
    objects: &[KirObject],
    signals: &DataSignals,
    current_refs: Option<&BTreeSet<String>>,
    cfg: Settings,
) -> KirGraph {
    let hits = hits(objects, signals);
    let mut parts = Vec::new();
    enumerations(objects, &hits, cfg, &mut parts);
    dead_branches(objects, &hits, signals.dead_after_months, &mut parts);
    ranges(objects, &hits, &mut parts);
    lookups(objects, current_refs, &mut parts);
    let mut g = KirGraph::new();
    let mut ids = BTreeSet::new();
    for p in parts {
        for o in p.objects {
            if ids.insert(o.id.0) {
                g.objects.push(o);
            }
        }
        g.relationships.extend(p.relationships);
        g.evidence.extend(p.evidence);
    }
    g
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_observation_sdk::signals::{Coverage, Usage};

    fn table() -> KirObject {
        let mut t = KirObject::new("orders", ObjectKind::Table);
        for (k, v) in [("origin", "live"), ("database", "erp"), ("owner", "public")] {
            t.properties.insert(k.into(), json!(v));
        }
        t.properties.insert(
            "columns".into(),
            json!([{"name": "amount", "profile": {"min": "10", "max": "500"}}, {"name": "status"}]),
        );
        t
    }

    fn site(clause: &str, op: &str, values: &[&str], label: Option<&str>) -> Value {
        json!({"relation": "orders", "column": "status", "op": op, "values": values,
               "clause": clause, "top_level": true, "line": 7, "label": label})
    }

    fn carrier(preds: Vec<Value>) -> KirObject {
        let mut o = KirObject::new("v_orders", ObjectKind::Custom("View".into()));
        o.properties
            .insert("source_path".into(), json!("sql/views.sql"));
        o.properties
            .insert("predicates".into(), Value::Array(preds));
        o
    }

    fn signals() -> DataSignals {
        let mut cov = Coverage {
            non_null: 100,
            latest: Some("2026-10".into()),
            unexplained: Usage {
                rows: 4,
                ..Default::default()
            },
            ..Default::default()
        };
        for (v, n, last) in [
            ("1", 60, "2026-10"),
            ("2", 26, "2026-10"),
            ("3", 10, "2023-01"),
            ("9", 0, ""),
        ] {
            cov.literals.insert(
                v.into(),
                Usage {
                    rows: n,
                    first_seen: None,
                    last_seen: (!last.is_empty()).then(|| last.into()),
                },
            );
        }
        DataSignals {
            columns: vec![ColumnSignal {
                database: "erp".into(),
                owner: "public".into(),
                table: "orders".into(),
                column: "status".into(),
                tier: "p1".into(),
                time_column: None,
                coverage: cov,
            }],
            dead_after_months: 24,
            ..Default::default()
        }
    }

    fn checks(g: &KirGraph) -> Vec<String> {
        let mut v: Vec<String> = g
            .objects
            .iter()
            .map(|o| s(o, "check").to_string())
            .collect();
        v.sort();
        v
    }

    #[test]
    fn a_case_without_the_other_values_is_a_finding_with_its_share() {
        let objects = vec![
            table(),
            carrier(vec![
                site("case", "in", &["1"], Some("'open'")),
                site("case", "in", &["2"], Some("'shipped'")),
            ]),
        ];
        let g = detect(&objects, &signals(), None, Settings::default());
        assert_eq!(checks(&g), ["predicate_misses_values"]);
        let f = &g.objects[0];
        assert_eq!(f.properties["measured"]["rows_unhandled"], 14);
        assert_eq!(f.properties["conflict_type"], CODE_VS_DATA);
        assert!(
            !f.properties["claims"].to_string().contains("14"),
            "no count in a claim"
        );
    }

    #[test]
    fn a_never_seen_and_a_stale_branch_are_dead() {
        let objects = vec![
            table(),
            carrier(vec![
                site("case", "in", &["3"], None),
                site("case", "in", &["9"], None),
                site("case", "in", &["1"], None),
                site("case", "in", &["2"], None),
            ]),
        ];
        let g = detect(&objects, &signals(), None, Settings { min_share: 0.5 });
        assert_eq!(checks(&g), ["dead_branch", "dead_branch"]);
    }

    #[test]
    fn a_where_filter_on_one_value_is_intent_not_a_finding() {
        let objects = vec![table(), carrier(vec![site("where", "in", &["1"], None)])];
        assert!(
            detect(&objects, &signals(), None, Settings::default())
                .objects
                .is_empty()
        );
    }

    #[test]
    fn a_constant_outside_the_measured_range() {
        let mut objects = vec![table(), carrier(vec![])];
        objects[1].properties.insert(
            "predicates".into(),
            json!([{"relation": "orders", "column": "amount", "op": ">", "values": ["1000"], "clause": "where", "line": 3}]),
        );
        let mut sig = signals();
        sig.columns[0].column = "amount".into();
        let g = detect(&objects, &sig, None, Settings::default());
        assert_eq!(checks(&g), ["range_assumption"]);
    }

    #[test]
    fn a_code_join_to_a_non_unique_column_fans_out() {
        let mut fk = KirObject::new(
            "orders.cust → customer.code",
            ObjectKind::Custom("FkCandidate".into()),
        );
        for (k, v) in [
            ("seed", "code"),
            ("database", "erp"),
            ("parent_owner", "public"),
            ("parent_table", "orders"),
            ("parent_column", "status"),
        ] {
            fk.properties.insert(k.into(), json!(v));
        }
        fk.properties.insert("parent_unique".into(), json!(false));
        fk.properties
            .insert("code_sources".into(), json!(["sql/x.sql#4"]));
        let objects = vec![table(), fk];
        let g = detect(&objects, &DataSignals::default(), None, Settings::default());
        assert_eq!(checks(&g), ["lookup_not_unique"]);
    }
}
