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
//! | `null_not_handled` | code equates a column with another relation's in an inner join (or a `WHERE`), nothing in the statement handles NULL, and the column's profile counts NULLs: those rows vanish |
//! | `cast_will_fail` | code converts a text column to a date or a number, and the profile counts values that would not convert ([`ConformanceSignal`]) |
//!
//! Claim texts carry no counts (a re-profile must not reopen a decided conflict); the counts are
//! in `measured`.

use crate::conflicts::{self, Claim};
use crate::data_usage::{ColumnSignal, ConformanceSignal, DataSignals};
use ekos_kir::predicates::{CAST_OP, Clause, PredicateSite};
use ekos_kir::{KirGraph, KirObject, ObjectKind};
use ekos_observation_sdk::signals::{literal_text, months_between};
use serde::Serialize;
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
            if site.clause.is_structural() {
                continue;
            }
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
    column: &str,
    attribute: &str,
    check: &str,
    claims: Vec<Claim>,
    measured: Value,
) -> KirGraph {
    let mut g = conflicts::conflict(subject, attribute, CODE_VS_DATA, claims, None);
    if let Some(o) = g.objects.first_mut() {
        o.properties.insert("check".into(), json!(check));
        o.properties.insert("column".into(), json!(column));
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
            // A CASE with an ELSE handles every other value on purpose.
            Clause::Case => h.site.op == "in" && h.site.case_else == Some(false),
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
            &c.column,
            &format!("code_vs_data:predicate_misses_values:{}:{}", short(h.carrier), c.column),
            "predicate_misses_values",
            vec![
                code_claim(h.carrier, h.site.line, format!("{} handles {} = {handled_list}", short(h.carrier), c.column)),
                data_claim(c, format!("{}.{} also holds values none of these branches handles", c.table, c.column)),
            ],
            json!({"rows_unhandled": other, "share": sh, "column_rows": c.coverage.non_null, "handled": values}),
        ));
    }
}

/// A carrier's name without its schema: a routine read from a file and the same routine read
/// from the live catalog (`public.f`) are one piece of logic.
fn short(carrier: &KirObject) -> &str {
    carrier.name.rsplit('.').next().unwrap_or(&carrier.name)
}

/// (routine, database, table, column).
type DeadKey = (String, String, String, String);

/// `dead_branch`: per routine and column, the branch values no row holds, or not for `dead_after`
/// months — one finding listing them, not one per value.
fn dead_branches(
    objects: &[KirObject],
    hits: &[Hit<'_>],
    dead_after: i64,
    out: &mut Vec<KirGraph>,
) {
    // (routine, database, table, column) → (first hit, dead values)
    let mut groups: BTreeMap<DeadKey, (&Hit<'_>, BTreeSet<String>)> = BTreeMap::new();
    for h in hits
        .iter()
        .filter(|h| matches!(h.site.clause, Clause::Case | Clause::Condition))
        .filter(|h| h.site.op == "in")
    {
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
            let key = (
                short(h.carrier).to_string(),
                c.database.clone(),
                c.table.clone(),
                c.column.clone(),
            );
            groups
                .entry(key)
                .or_insert((h, BTreeSet::new()))
                .1
                .insert(v.clone());
        }
    }
    for ((routine, _, _, _), (h, values)) in groups {
        let c = h.signal;
        let Some(subject) = table_of(objects, c) else {
            continue;
        };
        let list = values.iter().cloned().collect::<Vec<_>>().join(", ");
        let detail: Vec<Value> = values
            .iter()
            .map(|v| {
                let u = &c.coverage.literals[v];
                json!({"value": v, "rows": u.rows, "last_seen": u.last_seen})
            })
            .collect();
        let none_at_all = values.iter().all(|v| c.coverage.literals[v].rows == 0);
        out.push(finding(
            subject,
            &c.column,
            &format!("code_vs_data:dead_branch:{routine}:{}", c.column),
            "dead_branch",
            vec![
                code_claim(h.carrier, h.site.line, format!("{routine} has branches for {} = {list}", c.column)),
                data_claim(c, format!("{}.{} = {list}: {}", c.table, c.column, if none_at_all { "never in the data" } else { "not seen recently" })),
            ],
            json!({"values": detail, "latest_in_column": c.coverage.latest,
                   "why": if none_at_all { format!("no row holds {list}") } else { format!("no recent row holds {list}") }}),
        ));
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
            &c.column,
            &format!(
                "code_vs_data:range_assumption:{}:{}{}{v}",
                short(h.carrier),
                c.column,
                h.site.op
            ),
            "range_assumption",
            vec![
                code_claim(
                    h.carrier,
                    h.site.line,
                    format!(
                        "{} compares {} {} {v}",
                        short(h.carrier),
                        c.column,
                        h.site.op
                    ),
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
            s(o, "parent_column"),
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

// ── Structural sites: joins and conversions ──────────────────────────────────────────────────

/// A structural site (RFC 0254 [`Clause::Join`] / [`Clause::Cast`]) with its carrier and the live
/// table + column it resolves to.
struct Structural<'a> {
    carrier: &'a KirObject,
    site: PredicateSite,
    table: &'a KirObject,
    column: &'a Value,
}

fn column_named<'a>(t: &'a KirObject, column: &str) -> Option<&'a Value> {
    t.properties
        .get("columns")
        .and_then(Value::as_array)?
        .iter()
        .find(|c| {
            c["name"]
                .as_str()
                .is_some_and(|n| n.eq_ignore_ascii_case(column))
        })
}

/// Live tables by lowercase bare name.
fn live_by_name(objects: &[KirObject]) -> BTreeMap<String, Vec<&KirObject>> {
    let mut m: BTreeMap<String, Vec<&KirObject>> = BTreeMap::new();
    for o in objects
        .iter()
        .filter(|o| o.kind == ObjectKind::Table && s(o, "origin") == "live")
    {
        let v = m.entry(tail(&o.name)).or_default();
        if !v.iter().any(|x| x.id == o.id) {
            v.push(o);
        }
    }
    m
}

/// Every live copy of the table a site names (every database that has it) holding the column; for
/// an unqualified column, the one in-scope relation that has it — none when several do.
fn live_columns<'a>(
    site: &PredicateSite,
    live: &BTreeMap<String, Vec<&'a KirObject>>,
) -> Vec<(&'a KirObject, &'a Value)> {
    let names: Vec<String> = match &site.relation {
        Some(r) => vec![tail(r)],
        None => site.scope.iter().map(|r| tail(r)).collect(),
    };
    let per_name: Vec<Vec<(&KirObject, &Value)>> = names
        .iter()
        .map(|n| {
            live.get(n)
                .into_iter()
                .flatten()
                .filter_map(|t| Some((*t, column_named(t, &site.column)?)))
                .collect::<Vec<_>>()
        })
        .filter(|v| !v.is_empty())
        .collect();
    match per_name.as_slice() {
        [one] => one.clone(),
        _ => Vec::new(),
    }
}

fn structural<'a>(objects: &'a [KirObject], clause: Clause) -> Vec<Structural<'a>> {
    let live = live_by_name(objects);
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
            if site.clause != clause {
                continue;
            }
            for (table, column) in live_columns(&site, &live) {
                out.push(Structural {
                    carrier: o,
                    site: site.clone(),
                    table,
                    column,
                });
            }
        }
    }
    out
}

fn table_rows(t: &KirObject) -> Option<i64> {
    t.properties
        .get("profile")
        .and_then(|p| p["row_count"].as_i64())
        .or_else(|| t.properties.get("num_rows").and_then(Value::as_i64))
}

fn live_claim(t: &KirObject, column: &str, what: String) -> Claim {
    Claim {
        value: json!(what),
        path: format!(
            "profile://{}/{}.{}.{}",
            s(t, "database"),
            s(t, "owner"),
            t.name,
            column
        ),
        line: None,
        source: "profile".into(),
    }
}

/// (routine, database, owner, table, column).
type ColumnKey = (String, String, String, String, String);

/// `null_not_handled`: per routine and column, an inner-join equality on a column the profile
/// counts NULLs in, with nothing in the statement handling NULL there.
fn unhandled_nulls(objects: &[KirObject], out: &mut Vec<KirGraph>) {
    let mut groups: BTreeMap<ColumnKey, (&Structural<'_>, BTreeSet<String>)> = BTreeMap::new();
    let found = structural(objects, Clause::Join);
    for h in &found {
        // A join inside a subquery (`EXISTS`, a scalar subquery) drops no row of the statement.
        if h.site.null_handled != Some(false) || h.site.subquery {
            continue;
        }
        let nulls = h.column["profile"]["nulls"].as_i64().unwrap_or(0);
        if nulls <= 0 {
            continue;
        }
        let name = h.column["name"]
            .as_str()
            .unwrap_or(&h.site.column)
            .to_string();
        // A routine read from a file and the same routine read from the catalog differ in case.
        let key = (
            short(h.carrier).to_lowercase(),
            s(h.table, "database").to_string(),
            s(h.table, "owner").to_string(),
            h.table.name.clone(),
            name,
        );
        groups
            .entry(key)
            .or_insert((h, BTreeSet::new()))
            .1
            .extend(h.site.values.iter().cloned());
    }
    for ((routine, _, _, table, column), (h, joined)) in groups {
        let p = &h.column["profile"];
        let nulls = p["nulls"].as_i64().unwrap_or(0);
        let rows = table_rows(h.table);
        let others = joined.iter().cloned().collect::<Vec<_>>().join(", ");
        out.push(finding(
            h.table,
            &column,
            &format!("code_vs_data:null_not_handled:{routine}:{column}"),
            "null_not_handled",
            vec![
                code_claim(
                    h.carrier,
                    h.site.line,
                    format!("{routine} joins {table}.{column} = {others} and does not handle NULL"),
                ),
                live_claim(
                    h.table,
                    &column,
                    format!("{table}.{column} holds NULL: those rows drop out of the join"),
                ),
            ],
            json!({"nulls": nulls, "rows": rows, "share": rows.map(|r| share(nulls, r)),
                   "nulls_exact": p["nulls_exact"].as_bool().unwrap_or(false),
                   "joined_to": joined}),
        ));
    }
}

/// Whether a catalog type holds text.
fn is_text_type(data_type: &str) -> bool {
    let t = data_type.to_ascii_lowercase();
    ["char", "text", "string", "clob"]
        .iter()
        .any(|w| t.contains(w))
}

/// One text column code converts: what `ekos profile run` measures for `cast_will_fail`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct CastTarget {
    pub database: String,
    pub owner: String,
    pub table: String,
    pub column: String,
    /// `date` or `number`.
    pub class: String,
    pub format: Option<String>,
}

/// Every live text column code converts to a date or a number, once per (class, format).
pub fn cast_targets(objects: &[KirObject]) -> BTreeSet<CastTarget> {
    structural(objects, Clause::Cast)
        .into_iter()
        .filter(|h| h.site.op == CAST_OP)
        .filter(|h| h.column["data_type"].as_str().is_some_and(is_text_type))
        .filter_map(|h| {
            Some(CastTarget {
                database: s(h.table, "database").to_string(),
                owner: s(h.table, "owner").to_string(),
                table: h.table.name.clone(),
                column: h.column["name"].as_str()?.to_string(),
                class: h.site.values.first()?.clone(),
                format: h.site.values.get(1).cloned(),
            })
        })
        .collect()
}

/// (routine, database, owner, table, column, class, format).
type CastKey = (
    String,
    String,
    String,
    String,
    String,
    String,
    Option<String>,
);

/// `cast_will_fail`: per routine and conversion, a text column whose measured values do not all
/// convert.
fn failing_casts(objects: &[KirObject], measured: &[ConformanceSignal], out: &mut Vec<KirGraph>) {
    let mut groups: BTreeMap<CastKey, (&Structural<'_>, &ConformanceSignal)> = BTreeMap::new();
    let found = structural(objects, Clause::Cast);
    for h in &found {
        let (Some(class), format) = (h.site.values.first(), h.site.values.get(1)) else {
            continue;
        };
        let Some(name) = h.column["name"].as_str() else {
            continue;
        };
        let Some(m) = measured.iter().find(|m| {
            m.database == s(h.table, "database")
                && m.owner == s(h.table, "owner")
                && m.table == h.table.name
                && m.column == name
                && &m.class == class
                && m.format.as_ref() == format
        }) else {
            continue;
        };
        if m.failing <= 0 {
            continue;
        }
        let key = (
            short(h.carrier).to_lowercase(),
            m.database.clone(),
            m.owner.clone(),
            m.table.clone(),
            m.column.clone(),
            m.class.clone(),
            m.format.clone(),
        );
        groups.entry(key).or_insert((h, m));
    }
    for ((routine, _, _, table, column, class, format), (h, m)) in groups {
        let how = match &format {
            Some(f) => format!("a {class} (format {f})"),
            None => format!("a {class}"),
        };
        out.push(finding(
            h.table,
            &column,
            &format!(
                "code_vs_data:cast_will_fail:{routine}:{column}:{class}{}",
                format
                    .as_deref()
                    .map(|f| format!(":{f}"))
                    .unwrap_or_default()
            ),
            "cast_will_fail",
            vec![
                code_claim(
                    h.carrier,
                    h.site.line,
                    format!("{routine} converts {table}.{column} to {how}"),
                ),
                live_claim(
                    h.table,
                    &column,
                    format!("{table}.{column} holds values that do not convert to {how}"),
                ),
            ],
            json!({"rows_failing": m.failing, "column_rows": m.non_null,
                   "share": share(m.failing, m.non_null), "class": class, "format": format}),
        ));
    }
}

// ── RFC 0228 hand-off: expected row-count deltas ────────────────────────────────────────────────

/// RFC 0254 §3.4 → RFC 0228: a finding a person resolved by picking the **code's** claim says the
/// rows the code drops are dropped on purpose. A mart built from that logic is then expected to
/// lack those rows, and reconciliation must not flag the difference again.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Expectation {
    /// The conflict that records the decision.
    pub conflict: String,
    /// `predicate_misses_values` (rows holding values no branch handles) or `null_not_handled`
    /// (rows holding NULL in a join column).
    pub check: String,
    pub database: String,
    pub owner: String,
    pub table: String,
    pub column: String,
    /// Where the code is (`path:line` of the code claim).
    pub code: String,
    /// What the code handles: the enumerated values, or the columns it joins to.
    pub handled: Vec<String>,
    /// Rows the latest profile counted as dropped by the code, and their share of the column.
    pub expected_missing_rows: i64,
    pub share: Option<f64>,
    pub decided_by: String,
    pub decided_at: String,
    pub note: Option<String>,
}

fn strings_of(v: &Value) -> Vec<String> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|x| x.as_str().map(str::to_string))
        .collect()
}

/// The expectations RFC 0228 reconciliation reads, from the current `code_vs_data` conflicts
/// (`current`: the conflicts manifest; `None` = all). Pure; ordered by conflict id.
pub fn expectations(objects: &[KirObject], current: Option<&BTreeSet<String>>) -> Vec<Expectation> {
    let mut newest: BTreeMap<String, &KirObject> = BTreeMap::new();
    for o in objects.iter().filter(|o| conflicts::is_conflict(o)) {
        if s(o, "conflict_type") == CODE_VS_DATA
            && current.is_none_or(|c| c.contains(&o.id.to_string()))
        {
            // Later versions of one conflict (a decision) come later in the ledger.
            newest.insert(o.id.to_string(), o);
        }
    }
    let mut out = Vec::new();
    for (id, o) in newest {
        let check = s(o, "check");
        if !matches!(check, "predicate_misses_values" | "null_not_handled")
            || s(o, "status") != conflicts::RESOLVED
        {
            continue;
        }
        let Some(picked) = o.properties.get("picked_claim") else {
            continue;
        };
        if picked["source"].as_str() == Some("profile") {
            continue; // the data is right: the code must change, nothing is expected to drop.
        }
        let claims = o.properties.get("claims").and_then(Value::as_array);
        let data_path = claims
            .into_iter()
            .flatten()
            .filter_map(|c| c["path"].as_str())
            .find_map(|p| p.strip_prefix("profile://"))
            .unwrap_or("");
        let (database, rest) = data_path.split_once('/').unwrap_or(("", ""));
        let mut parts = rest.splitn(3, '.');
        let (owner, table, column) = (
            parts.next().unwrap_or("").to_string(),
            parts.next().unwrap_or("").to_string(),
            parts.next().unwrap_or("").to_string(),
        );
        let measured = o.properties.get("measured").cloned().unwrap_or_default();
        let (rows, handled) = match check {
            "predicate_misses_values" => (
                measured["rows_unhandled"].as_i64(),
                strings_of(&measured["handled"]),
            ),
            _ => (
                measured["nulls"].as_i64(),
                strings_of(&measured["joined_to"]),
            ),
        };
        let code = match (picked["path"].as_str(), picked["line"].as_u64()) {
            (Some(p), Some(l)) => format!("{p}:{l}"),
            (Some(p), None) => p.to_string(),
            _ => String::new(),
        };
        out.push(Expectation {
            conflict: id,
            check: check.to_string(),
            database: database.to_string(),
            owner,
            table,
            column,
            code,
            handled,
            expected_missing_rows: rows.unwrap_or(0),
            share: measured["share"].as_f64(),
            decided_by: s(o, "reviewed_by").to_string(),
            decided_at: s(o, "reviewed_at").to_string(),
            note: o
                .properties
                .get("review_note")
                .and_then(Value::as_str)
                .map(str::to_string),
        });
    }
    out
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
    unhandled_nulls(objects, &mut parts);
    failing_casts(objects, &signals.conformance, &mut parts);
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
               "clause": clause, "top_level": true, "line": 7, "label": label, "case_else": false})
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
                references: None,
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
    fn a_case_with_an_else_handles_the_rest_on_purpose() {
        let mut a = site("case", "in", &["1"], None);
        a["case_else"] = json!(true);
        let objects = vec![table(), carrier(vec![a])];
        let g = detect(&objects, &signals(), None, Settings::default());
        assert!(!checks(&g).contains(&"predicate_misses_values".to_string()));
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
        // One finding per routine and column, listing both values.
        assert_eq!(checks(&g), ["dead_branch"]);
        assert_eq!(
            g.objects[0].properties["measured"]["values"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(g.objects[0].properties["column"], "status");
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

    fn live(name: &str, database: &str, columns: Value, rows: i64) -> KirObject {
        let mut t = KirObject::new(name, ObjectKind::Table);
        t.id = ekos_kir::KirId(uuid::Uuid::new_v5(
            &uuid::Uuid::NAMESPACE_URL,
            format!("{database}:{name}").as_bytes(),
        ));
        for (k, v) in [
            ("origin", "live"),
            ("database", database),
            ("owner", "public"),
        ] {
            t.properties.insert(k.into(), json!(v));
        }
        t.properties.insert("columns".into(), columns);
        t.properties
            .insert("profile".into(), json!({"row_count": rows}));
        t
    }

    fn join_site(rel: &str, col: &str, other: &str, handled: bool) -> Value {
        json!({"relation": rel, "column": col, "op": "=", "values": [other], "clause": "join",
               "top_level": true, "null_handled": handled, "line": 12})
    }

    #[test]
    fn an_inner_join_on_a_column_holding_nulls_drops_rows() {
        let invoice = live(
            "invoice",
            "erp",
            json!([{"name": "customer_id", "data_type": "integer", "profile": {"nulls": 42, "nulls_exact": true}},
                   {"name": "id", "data_type": "integer", "profile": {"nulls": 0}}]),
            1000,
        );
        let customer = live(
            "customer",
            "erp",
            json!([{"name": "id", "data_type": "integer", "profile": {"nulls": 0}}]),
            50,
        );
        let objects = vec![
            invoice,
            customer,
            carrier(vec![
                join_site("invoice", "customer_id", "customer.id", false),
                join_site("customer", "id", "invoice.customer_id", false),
            ]),
        ];
        let g = detect(&objects, &DataSignals::default(), None, Settings::default());
        assert_eq!(checks(&g), ["null_not_handled"]);
        let f = &g.objects[0];
        assert_eq!(f.properties["column"], "customer_id");
        assert_eq!(f.properties["measured"]["nulls"], 42);
        assert_eq!(f.properties["measured"]["share"], 0.042);
        assert!(!f.properties["claims"].to_string().contains("42"));

        // Handled in the statement (an `IS NOT NULL`, a `COALESCE`): nothing to say.
        let mut handled = objects.clone();
        handled[2] = carrier(vec![join_site(
            "invoice",
            "customer_id",
            "customer.id",
            true,
        )]);
        assert!(
            detect(&handled, &DataSignals::default(), None, Settings::default())
                .objects
                .is_empty()
        );
    }

    #[test]
    fn a_text_column_converted_to_a_date_with_values_that_do_not_convert() {
        let t = live(
            "import_row",
            "erp",
            json!([{"name": "posted", "data_type": "character varying"},
                   {"name": "amount", "data_type": "numeric"}]),
            500,
        );
        let cast = |col: &str, values: Value| {
            json!({"relation": "import_row", "column": col, "op": "cast", "values": values,
                   "clause": "cast", "line": 4})
        };
        let objects = vec![
            t,
            carrier(vec![
                cast("posted", json!(["date", "DD.MM.YYYY"])),
                cast("amount", json!(["number"])),
            ]),
        ];
        // Only the text column is a target for measurement.
        let targets: Vec<CastTarget> = cast_targets(&objects).into_iter().collect();
        assert_eq!(targets.len(), 1);
        assert_eq!(
            (targets[0].column.as_str(), targets[0].format.as_deref()),
            ("posted", Some("DD.MM.YYYY"))
        );
        let mut sig = DataSignals::default();
        sig.conformance.push(ConformanceSignal {
            database: "erp".into(),
            owner: "public".into(),
            table: "import_row".into(),
            column: "posted".into(),
            class: "date".into(),
            format: Some("DD.MM.YYYY".into()),
            non_null: 500,
            failing: 10,
        });
        let g = detect(&objects, &sig, None, Settings::default());
        assert_eq!(checks(&g), ["cast_will_fail"]);
        assert_eq!(g.objects[0].properties["measured"]["share"], 0.02);
        sig.conformance[0].failing = 0;
        assert!(
            detect(&objects, &sig, None, Settings::default())
                .objects
                .is_empty()
        );
    }

    #[test]
    fn a_table_in_two_databases_is_checked_in_each() {
        let cols =
            json!([{"name": "customer_id", "data_type": "integer", "profile": {"nulls": 3}}]);
        let objects = vec![
            live("invoice", "erp", cols.clone(), 10),
            live("invoice", "ch", cols, 10),
            carrier(vec![join_site(
                "invoice",
                "customer_id",
                "customer.id",
                false,
            )]),
        ];
        let g = detect(&objects, &DataSignals::default(), None, Settings::default());
        assert_eq!(g.objects.len(), 2);
    }

    #[test]
    fn a_decision_that_the_code_is_right_becomes_an_expected_delta() {
        let objects = vec![
            table(),
            carrier(vec![
                site("case", "in", &["1"], Some("'open'")),
                site("case", "in", &["2"], Some("'shipped'")),
            ]),
        ];
        let g = detect(&objects, &signals(), None, Settings::default());
        let open = g.objects[0].clone();
        assert!(
            expectations(std::slice::from_ref(&open), None).is_empty(),
            "open: nothing expected"
        );
        let claims = open.properties["claims"].as_array().unwrap();
        let code_at = 1 + claims
            .iter()
            .position(|c| c["source"] != "profile")
            .unwrap();
        let data_at = 1 + claims
            .iter()
            .position(|c| c["source"] == "profile")
            .unwrap();
        let code_right = conflicts::resolve(
            &open,
            &conflicts::Resolution::Pick(code_at),
            "ana",
            "2026-11-12",
            Some("drop on purpose"),
        )
        .unwrap();
        let data_right = conflicts::resolve(
            &open,
            &conflicts::Resolution::Pick(data_at),
            "ana",
            "2026-11-12",
            None,
        )
        .unwrap();
        let e = expectations(&[open.clone(), code_right], None);
        assert_eq!(e.len(), 1);
        assert_eq!(
            (
                e[0].table.as_str(),
                e[0].column.as_str(),
                e[0].expected_missing_rows
            ),
            ("orders", "status", 14)
        );
        assert_eq!(e[0].handled, ["1", "2"]);
        assert_eq!(e[0].code, "sql/views.sql:7");
        assert_eq!(e[0].decided_by, "ana");
        assert!(expectations(&[open.clone(), data_right], None).is_empty());
        let other = BTreeSet::from(["x".to_string()]);
        assert!(expectations(&[open], Some(&other)).is_empty());
    }

    #[test]
    fn a_join_in_a_subquery_and_a_second_spelling_of_one_view_add_nothing() {
        let cols =
            json!([{"name": "customer_id", "data_type": "integer", "profile": {"nulls": 3}}]);
        let mut sub = join_site("invoice", "customer_id", "customer.id", false);
        sub["subquery"] = json!(true);
        let mut upper = carrier(vec![join_site(
            "invoice",
            "customer_id",
            "customer.id",
            false,
        )]);
        upper.name = "V_ORDERS".into();
        upper.id = ekos_kir::KirId(uuid::Uuid::nil());
        let objects = vec![
            live("invoice", "erp", cols, 10),
            carrier(vec![
                join_site("invoice", "customer_id", "customer.id", false),
                sub,
            ]),
            upper,
        ];
        let g = detect(&objects, &DataSignals::default(), None, Settings::default());
        assert_eq!(
            g.objects.len(),
            1,
            "one finding: the subquery's join and the upper-case copy add none"
        );
    }
}
