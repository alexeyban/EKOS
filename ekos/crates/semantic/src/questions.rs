//! RFC 0254 §3.1 — open questions: what nobody can explain yet, as objects a person can be asked.
//!
//! Pure. [`derive`] turns current origins — unexplained code values, values only the data holds,
//! dead codes, open conflicts (including `code_vs_data`), unclear inferred references, tables
//! with no key, stale annotations — into keyed `OpenQuestion` objects with a deterministic text,
//! context, priority and addressees. A question is keyed by origin kind + origin id + type, so an
//! unchanged origin yields the same question on every commit.
//!
//! **Status is computed on read** ([`status`]), never written: `open`, `answered` (a person's
//! active testimony answers it, RFC 0253), `closed` (the origin was decided) or `obsolete` (the
//! origin is no longer current).

use crate::business_semantics::{ENUM_MEANING, EXPLAINED_BY, GAP, RATIONALE};
use ekos_kir::{KirId, KirObject, KirRelationship, ObjectKind, RelationshipKind};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use uuid::Uuid;

pub const KIND: &str = "OpenQuestion";
/// RFC 0253's testimony kind; a testimony with `answers = <question id>` answers it.
pub const TESTIMONY: &str = "Testimony";

/// The question types (RFC 0254 §3.1).
pub const TYPES: &[&str] = &[
    "meaning_of_value",
    "unexplained_values",
    "dead_code",
    "which_is_right",
    "logic_vs_data",
    "is_this_a_reference",
    "what_is_a_row",
    "still_true",
];

/// Who is likely to know: `pattern` (a table, `table.column`, or a `*` glob over either) →
/// people (console user names or e-mail addresses).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Owners {
    pub rules: Vec<(String, Vec<String>)>,
    /// Asked when nothing better is known.
    pub reviewers: Vec<String>,
}

/// What derivation reads. The `*_current` sets are the latest manifests; `None` = all.
pub struct Origins<'a> {
    pub objects: &'a [KirObject],
    pub relationships: &'a [KirRelationship],
    pub semantics_current: Option<&'a BTreeSet<String>>,
    pub conflicts_current: Option<&'a BTreeSet<String>>,
    pub references_current: Option<&'a BTreeSet<String>>,
    pub grain_current: Option<&'a BTreeSet<String>>,
    /// Lowercase table names a person put out of scope (RFC 0213): they produce no questions.
    pub out_of_scope: &'a BTreeSet<String>,
    pub owners: &'a Owners,
}

fn kid(key: &str) -> KirId {
    KirId(Uuid::new_v5(&Uuid::NAMESPACE_URL, key.as_bytes()))
}

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

fn current(set: Option<&BTreeSet<String>>, o: &KirObject) -> bool {
    set.is_none_or(|c| c.contains(&o.id.to_string()))
}

/// A draft before priority and routing.
struct Draft<'a> {
    origin: &'a KirObject,
    qtype: &'static str,
    subject: Option<KirId>,
    table: String,
    column: String,
    text: String,
    context: Value,
    blocking: bool,
    share: f64,
}

fn describes(rels: &[KirRelationship], from: KirId) -> Option<KirId> {
    rels.iter()
        .find(|r| r.from == from && r.kind == RelationshipKind::Custom("Describes".into()))
        .map(|r| r.to)
}

fn gap_drafts<'a>(o: &'a KirObject, rels: &[KirRelationship]) -> Option<Draft<'a>> {
    let (table, column) = (tail(s(o, "table")), s(o, "column").to_lowercase());
    let (qtype, text, share) = match s(o, "gap_type") {
        "unexplained_value" => {
            let seen: Vec<String> = o
                .properties
                .get("seen_in")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            let n = o
                .properties
                .get("usage_sites")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let eg = seen
                .first()
                .map(|x| format!(", e.g. {x}"))
                .unwrap_or_default();
            (
                "meaning_of_value",
                format!(
                    "What does {table}.{column} = {} mean? It is used in {n} place(s){eg}.",
                    s(o, "value")
                ),
                0.0,
            )
        }
        crate::data_usage::DATA_ONLY => {
            let share = o
                .properties
                .get("share")
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            let distinct = o
                .properties
                .get("distinct")
                .and_then(Value::as_i64)
                .map(|d| format!("{d} value(s)"))
                .unwrap_or_else(|| "values".into());
            (
                "unexplained_values",
                format!(
                    "{:.1}% of rows in {table}.{column} hold {distinct} no code mentions. What are they?",
                    share * 100.0
                ),
                share,
            )
        }
        _ => return None,
    };
    Some(Draft {
        origin: o,
        qtype,
        subject: describes(rels, o.id),
        table,
        column,
        text,
        context: json!({"gap_type": s(o, "gap_type"), "value": o.properties.get("value"),
                        "rows": o.properties.get("rows"), "database": o.properties.get("database")}),
        blocking: false,
        share,
    })
}

fn dead_code_draft<'a>(o: &'a KirObject, rels: &[KirRelationship]) -> Option<Draft<'a>> {
    let usage = o.properties.get("data_usage")?.as_array()?;
    let dead = usage.iter().find(|u| u["dead"] == true)?;
    let (table, column) = (tail(s(o, "table")), s(o, "column").to_lowercase());
    let n = o
        .properties
        .get("usage_sites")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    Some(Draft {
        origin: o,
        qtype: "dead_code",
        subject: describes(rels, o.id),
        text: format!(
            "{table}.{column} = {} has not appeared since {}, but {n} code path(s) handle it. Can it be dropped?",
            s(o, "value"),
            dead["last_seen"].as_str().unwrap_or("?")
        ),
        table,
        column,
        context: json!({"value": s(o, "value"), "label": o.properties.get("label"), "data_usage": dead}),
        blocking: false,
        share: dead["share"].as_f64().unwrap_or(0.0),
    })
}

fn claim_text(c: &Value) -> String {
    let v = match &c["value"] {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    format!("{} ({})", v, c["source"].as_str().unwrap_or("?"))
}

fn conflict_draft(o: &KirObject) -> Option<Draft<'_>> {
    if s(o, "status") != "open"
        || o.properties
            .get("explanation")
            .is_some_and(|e| !e.is_null())
    {
        return None;
    }
    let claims = o.properties.get("claims")?.as_array()?;
    let subject: KirId = s(o, "subject_id").parse::<Uuid>().ok().map(KirId)?;
    let table = tail(s(o, "subject_name"));
    let (qtype, text, share) = if s(o, "conflict_type") == crate::code_vs_data::CODE_VS_DATA
        && s(o, "check") == "predicate_misses_values"
    {
        let m = &o.properties["measured"];
        let share = m["share"].as_f64().unwrap_or(0.0);
        (
            "logic_vs_data",
            format!(
                "{} — but the data also holds other values ({:.1}% of rows). Where should they go?",
                claims.first().map(claim_text).unwrap_or_default(),
                share * 100.0
            ),
            share,
        )
    } else {
        let a = claims.first().map(claim_text).unwrap_or_default();
        let b = claims.get(1).map(claim_text).unwrap_or_default();
        (
            "which_is_right",
            format!("{a} says one thing; {b} says another. Which is right?"),
            0.0,
        )
    };
    Some(Draft {
        origin: o,
        qtype,
        subject: Some(subject),
        table,
        column: String::new(),
        text,
        context: json!({"conflict_type": s(o, "conflict_type"), "check": o.properties.get("check"),
                        "attribute": s(o, "attribute"), "claims": claims, "measured": o.properties.get("measured")}),
        blocking: true,
        share,
    })
}

fn reference_draft(o: &KirObject) -> Option<Draft<'_>> {
    if s(o, "verdict") != "unclear" {
        return None;
    }
    let rate = o
        .properties
        .get("inclusion_rate")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    let child = format!("{}.{}", s(o, "child_table"), s(o, "child_column"));
    let parent = format!("{}.{}", s(o, "parent_table"), s(o, "parent_column"));
    Some(Draft {
        origin: o,
        qtype: "is_this_a_reference",
        subject: None,
        table: s(o, "child_table").to_lowercase(),
        column: s(o, "child_column").to_lowercase(),
        text: format!(
            "{:.1}% of {child} values exist in {parent}. Is {child} a reference to {parent}?",
            rate * 100.0
        ),
        context: json!({"child": child, "parent": parent, "orphans": o.properties.get("orphans"),
                        "population": o.properties.get("population"), "seed": s(o, "seed")}),
        blocking: true,
        share: rate,
    })
}

fn grain_draft<'a>(o: &'a KirObject, rels: &[KirRelationship]) -> Option<Draft<'a>> {
    if s(o, "source") != "none" {
        return None;
    }
    let table = s(o, "table").to_lowercase();
    Some(Draft {
        origin: o,
        qtype: "what_is_a_row",
        subject: describes(rels, o.id),
        text: format!(
            "No unique key found in {table} ({} rows). What does one row represent?",
            o.properties.get("rows").cloned().unwrap_or(Value::Null)
        ),
        table,
        column: String::new(),
        context: json!({"database": s(o, "database"), "owner": s(o, "owner"), "tested": o.properties.get("tested")}),
        blocking: false,
        share: 0.0,
    })
}

/// Stale annotations: the entity changed since a person wrote about it.
fn annotation_drafts<'a>(objects: &'a [KirObject]) -> Vec<Draft<'a>> {
    let by_id: HashMap<String, &KirObject> =
        objects.iter().map(|o| (o.id.to_string(), o)).collect();
    let mut out = Vec::new();
    for a in ekos_kir::annotations::current(objects) {
        let entity = by_id.get(&a.target).copied();
        let digest = entity.map(ekos_kir::annotations::digest_of_object);
        if ekos_kir::annotations::effective(&a, digest.as_deref())
            != ekos_kir::annotations::Effective::Stale
        {
            continue;
        }
        let Some(origin) = objects.iter().find(|o| o.id == a.id) else {
            continue;
        };
        let name = entity
            .map(|e| e.name.clone())
            .unwrap_or_else(|| a.target.clone());
        out.push(Draft {
            origin,
            qtype: "still_true",
            subject: entity.map(|e| e.id),
            table: tail(&name),
            column: String::new(),
            text: format!(
                "{} noted \"{}\" on {name}, which has changed since. Is it still true?",
                a.author, a.text
            ),
            context: json!({"author": a.author, "annotation": a.id.to_string()}),
            blocking: false,
            share: 0.0,
        });
    }
    out
}

/// Whether `pattern` (a name or `*` glob, case-insensitive) matches `text`.
fn glob(pattern: &str, text: &str) -> bool {
    let (p, t) = (pattern.to_lowercase(), text.to_lowercase());
    match p.split_once('*') {
        None => p == t,
        Some((head, rest)) => {
            t.starts_with(head)
                && (rest.is_empty() || {
                    let after = &t[head.len()..];
                    (0..=after.len()).any(|i| glob(rest, &after[i..]))
                })
        }
    }
}

/// Ranked addressees: configured owners, then authors of the code that introduced the origin
/// (git blame), then reviewers. Unique, in that order.
fn addressees(
    d: &Draft<'_>,
    o: &Origins<'_>,
    authors: &HashMap<KirId, Vec<String>>,
) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let target = if d.column.is_empty() {
        d.table.clone()
    } else {
        format!("{}.{}", d.table, d.column)
    };
    for (pattern, people) in &o.owners.rules {
        if glob(pattern, &target) || glob(pattern, &d.table) {
            out.extend(people.iter().cloned());
        }
    }
    if let Some(a) = authors.get(&d.origin.id) {
        out.extend(a.iter().cloned());
    }
    out.extend(o.owners.reviewers.iter().cloned());
    let mut seen = BTreeSet::new();
    out.retain(|p| seen.insert(p.to_lowercase()));
    out
}

/// Git-blame authors per item (RFC 0170 `RationaleLink`s an item is `ExplainedBy`).
fn blame_authors(o: &Origins<'_>) -> HashMap<KirId, Vec<String>> {
    let rationale: HashMap<KirId, &KirObject> = o
        .objects
        .iter()
        .filter(|x| custom(x) == Some(RATIONALE))
        .map(|x| (x.id, x))
        .collect();
    let mut out: HashMap<KirId, Vec<String>> = HashMap::new();
    for r in o
        .relationships
        .iter()
        .filter(|r| r.kind == RelationshipKind::Custom(EXPLAINED_BY.into()))
    {
        if let Some(l) = rationale.get(&r.to) {
            let a = s(l, "author");
            if !a.is_empty() {
                out.entry(r.from).or_default().push(a.to_string());
            }
        }
    }
    for v in out.values_mut() {
        v.sort();
        v.dedup();
    }
    out
}

/// How many relationships point at each object (a cheap impact measure).
fn dependents(rels: &[KirRelationship]) -> HashMap<KirId, usize> {
    let mut m = HashMap::new();
    for r in rels {
        *m.entry(r.to).or_insert(0usize) += 1;
    }
    m
}

/// Every open question the current origins raise, keyed and prioritised.
pub fn derive(o: &Origins<'_>) -> Vec<KirObject> {
    let mut drafts: Vec<Draft<'_>> = Vec::new();
    for x in o.objects {
        match custom(x) {
            Some(GAP) if current(o.semantics_current, x) => {
                drafts.extend(gap_drafts(x, o.relationships));
            }
            Some(ENUM_MEANING) if current(o.semantics_current, x) => {
                if s(x, "status") == "hypothesis" || s(x, "status").is_empty() {
                    drafts.extend(dead_code_draft(x, o.relationships));
                }
            }
            Some(crate::conflicts::KIND) if current(o.conflicts_current, x) => {
                drafts.extend(conflict_draft(x));
            }
            Some("FkCandidate") if current(o.references_current, x) => {
                drafts.extend(reference_draft(x));
            }
            Some("GrainCandidate") if current(o.grain_current, x) => {
                drafts.extend(grain_draft(x, o.relationships));
            }
            _ => {}
        }
    }
    drafts.extend(annotation_drafts(o.objects));
    drafts.retain(|d| !o.out_of_scope.contains(&d.table));
    let impact = dependents(o.relationships);
    let authors = blame_authors(o);
    let mut out: Vec<KirObject> = drafts
        .iter()
        .map(|d| {
            let deps = d
                .subject
                .and_then(|id| impact.get(&id))
                .copied()
                .unwrap_or(0);
            let priority =
                if d.blocking { 1_000_000.0 } else { 0.0 } + (deps as f64) * 10.0 + d.share * 100.0;
            let key = format!(
                "open-question:{}:{}:{}",
                custom(d.origin).unwrap_or("?"),
                d.origin.id,
                d.qtype
            );
            let mut q = KirObject::new(d.text.clone(), ObjectKind::Custom(KIND.into()));
            q.id = kid(&key);
            for (k, v) in [
                ("question_type", json!(d.qtype)),
                ("origin_kind", json!(custom(d.origin))),
                ("origin_id", json!(d.origin.id.to_string())),
                ("subject", json!(d.subject.map(|i| i.to_string()))),
                ("table", json!(d.table)),
                ("column", json!(d.column)),
                ("text", json!(d.text)),
                ("context", d.context.clone()),
                ("priority", json!((priority * 100.0).round() / 100.0)),
                ("dependents", json!(deps)),
                ("blocking", json!(d.blocking)),
                ("addressees", json!(addressees(d, o, &authors))),
            ] {
                q.properties.insert(k.into(), v);
            }
            q.evidence = d.origin.evidence.clone();
            q
        })
        .collect();
    out.sort_by_key(rank);
    out
}

/// Highest priority first, ties by id (stable across ledger order).
pub fn rank(q: &KirObject) -> (i64, String) {
    let p = q
        .properties
        .get("priority")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    (-(p * 100.0) as i64, q.id.to_string())
}

/// A question's status, computed on read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Open,
    Answered,
    Closed,
    Obsolete,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Open => "open",
            Status::Answered => "answered",
            Status::Closed => "closed",
            Status::Obsolete => "obsolete",
        }
    }
}

/// What status computation reads.
pub struct Reading<'a> {
    pub by_id: &'a HashMap<String, &'a KirObject>,
    /// Question ids derived by the latest run.
    pub questions_current: Option<&'a BTreeSet<String>>,
    /// Review decisions (RFC 0213) by (subject, item id).
    pub decisions: &'a HashMap<(String, String), &'a KirObject>,
    /// Question id → active testimonies answering it.
    pub answers: &'a HashMap<String, Vec<&'a KirObject>>,
}

/// `closed` when the origin was decided; `obsolete` when the latest derivation no longer raised
/// the question; `answered` when a person's active testimony answers it.
pub fn status(q: &KirObject, r: &Reading<'_>) -> Status {
    if r.questions_current
        .is_some_and(|c| !c.contains(&q.id.to_string()))
    {
        return Status::Obsolete;
    }
    let origin = r.by_id.get(s(q, "origin_id")).copied();
    let decided = origin.is_some_and(|o| match custom(o) {
        Some(crate::conflicts::KIND) => s(o, "status") != "open",
        Some(GAP) | Some(ENUM_MEANING) => {
            matches!(s(o, "status"), "confirmed" | "rejected" | "edited")
        }
        Some("FkCandidate") => r
            .decisions
            .contains_key(&("relationships".to_string(), o.id.to_string())),
        _ => false,
    });
    if decided {
        Status::Closed
    } else if r
        .answers
        .get(&q.id.to_string())
        .is_some_and(|a| !a.is_empty())
    {
        Status::Answered
    } else {
        Status::Open
    }
}

/// Active testimonies by the question they answer (the newest version of each testimony wins).
pub fn answers_index(objects: &[KirObject]) -> HashMap<String, Vec<&KirObject>> {
    let mut newest: BTreeMap<String, &KirObject> = BTreeMap::new();
    for t in objects.iter().filter(|o| custom(o) == Some(TESTIMONY)) {
        let k = t.id.to_string();
        let newer = newest
            .get(&k)
            .is_none_or(|old| s(t, "updated_at") >= s(old, "updated_at"));
        if newer {
            newest.insert(k, t);
        }
    }
    let mut out: HashMap<String, Vec<&KirObject>> = HashMap::new();
    for t in newest.into_values() {
        if s(t, "status") == "active" && !s(t, "answers").is_empty() {
            out.entry(s(t, "answers").to_string()).or_default().push(t);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obj(kind: &str, name: &str, props: Value) -> KirObject {
        let mut o = KirObject::new(name, ObjectKind::Custom(kind.into()));
        o.id = kid(name);
        for (k, v) in props.as_object().unwrap() {
            o.properties.insert(k.clone(), v.clone());
        }
        o
    }

    fn origins<'a>(
        objects: &'a [KirObject],
        scope: &'a BTreeSet<String>,
        owners: &'a Owners,
    ) -> Origins<'a> {
        Origins {
            objects,
            relationships: &[],
            semantics_current: None,
            conflicts_current: None,
            references_current: None,
            grain_current: None,
            out_of_scope: scope,
            owners,
        }
    }

    fn sample() -> Vec<KirObject> {
        vec![
            obj(
                GAP,
                "gap1",
                json!({"gap_type": "unexplained_value", "table": "orders", "column": "status", "value": "7", "usage_sites": 3, "seen_in": ["v_open"]}),
            ),
            obj(
                GAP,
                "gap2",
                json!({"gap_type": "data_only_values", "table": "orders", "column": "status", "share": 0.041, "distinct": 2}),
            ),
            obj(
                "FkCandidate",
                "fk1",
                json!({"verdict": "unclear", "child_table": "invoice", "child_column": "cust", "parent_table": "customer", "parent_column": "id", "inclusion_rate": 0.8}),
            ),
            obj(
                "GrainCandidate",
                "g1",
                json!({"source": "none", "table": "log", "rows": 9}),
            ),
            obj(GAP, "gap3", json!({"gap_type": "undocumented_concept"})),
        ]
    }

    #[test]
    fn each_origin_yields_one_question_with_a_stable_id() {
        let objects = sample();
        let (scope, owners) = (BTreeSet::new(), Owners::default());
        let a = derive(&origins(&objects, &scope, &owners));
        let types: BTreeSet<&str> = a.iter().map(|q| s(q, "question_type")).collect();
        assert_eq!(
            types,
            [
                "is_this_a_reference",
                "meaning_of_value",
                "unexplained_values",
                "what_is_a_row"
            ]
            .into_iter()
            .collect()
        );
        let mut reversed = objects.clone();
        reversed.reverse();
        let b = derive(&origins(&reversed, &scope, &owners));
        assert_eq!(
            a.iter().map(|q| q.id).collect::<Vec<_>>(),
            b.iter().map(|q| q.id).collect::<Vec<_>>(),
            "same ids, same ranking whatever the ledger order"
        );
        assert_eq!(
            s(&a[0], "question_type"),
            "is_this_a_reference",
            "blocking first"
        );
        assert!(
            s(
                a.iter()
                    .find(|q| s(q, "question_type") == "unexplained_values")
                    .unwrap(),
                "text"
            )
            .starts_with("4.1% of rows")
        );
    }

    #[test]
    fn out_of_scope_tables_ask_nothing() {
        let objects = sample();
        let scope: BTreeSet<String> = ["orders", "invoice", "log"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert!(derive(&origins(&objects, &scope, &Owners::default())).is_empty());
    }

    #[test]
    fn status_is_computed_answered_closed_obsolete() {
        let objects = sample();
        let owners = Owners::default();
        let scope = BTreeSet::new();
        let qs = derive(&origins(&objects, &scope, &owners));
        let q = qs
            .iter()
            .find(|q| s(q, "question_type") == "is_this_a_reference")
            .unwrap();
        let mut all = objects.clone();
        let by_id: HashMap<String, &KirObject> =
            all.iter().map(|o| (o.id.to_string(), o)).collect();
        let none = HashMap::new();
        let answers = HashMap::new();
        let r = Reading {
            by_id: &by_id,
            questions_current: None,
            decisions: &none,
            answers: &answers,
        };
        assert_eq!(status(q, &r), Status::Open);
        // A testimony answers it.
        let t = obj(
            TESTIMONY,
            "t1",
            json!({"answers": q.id.to_string(), "status": "active", "updated_at": "1"}),
        );
        let objs2 = vec![t];
        let answers = answers_index(&objs2);
        let r = Reading {
            by_id: &by_id,
            questions_current: None,
            decisions: &none,
            answers: &answers,
        };
        assert_eq!(status(q, &r), Status::Answered);
        // The reference is decided: closed.
        let fk = all.iter().find(|o| o.name == "fk1").unwrap();
        let d = obj("ReviewDecision", "d", json!({}));
        let decisions = HashMap::from([(("relationships".to_string(), fk.id.to_string()), &d)]);
        let r = Reading {
            by_id: &by_id,
            questions_current: None,
            decisions: &decisions,
            answers: &answers,
        };
        assert_eq!(status(q, &r), Status::Closed);
        // Not raised by the latest derivation: obsolete.
        let cur = BTreeSet::new();
        let r = Reading {
            by_id: &by_id,
            questions_current: Some(&cur),
            decisions: &none,
            answers: &answers,
        };
        assert_eq!(status(q, &r), Status::Obsolete);
        all.clear();
    }

    #[test]
    fn routing_owner_then_reviewers_never_duplicates() {
        let objects = sample();
        let owners = Owners {
            rules: vec![
                ("orders.*".into(), vec!["ana".into()]),
                ("ord*".into(), vec!["Ana".into(), "bo".into()]),
            ],
            reviewers: vec!["rev".into()],
        };
        let scope = BTreeSet::new();
        let qs = derive(&origins(&objects, &scope, &owners));
        let q = qs
            .iter()
            .find(|q| s(q, "question_type") == "meaning_of_value")
            .unwrap();
        assert_eq!(q.properties["addressees"], json!(["ana", "bo", "rev"]));
        let g = qs
            .iter()
            .find(|q| s(q, "question_type") == "what_is_a_row")
            .unwrap();
        assert_eq!(g.properties["addressees"], json!(["rev"]));
    }
}
