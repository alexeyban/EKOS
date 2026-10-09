//! RFC 0172 — ConflictingEvidence: when two sources make different claims about the same attribute
//! of the same thing, keep both claims, link them to the thing, and let a person resolve it.
//!
//! Three detectors, all pure and deterministic:
//!
//! | Detector | Where it runs | What it compares |
//! |---|---|---|
//! | [`duplicate_definitions`] | `SemanticCompilerPass`, right after artifacts are combined | objects that share an id (one table created in two files) |
//! | [`merge_losses`] | `SemanticCompilerPass`, before `apply_merges` | the members of an exact-name merge group |
//! | [`label_mismatches`] | `ekos commit`, after business-semantics synthesis | an `EnumMeaning`'s labels from different sources |
//!
//! Values are normalized before comparison (SQL type aliases, case), a fact only one side states is
//! unknown rather than false, and prose is never compared. Review is human-only: [`resolve`] is
//! called by the CLI and must never be reachable from MCP.

use ekos_identity::MergeProposal;
use ekos_kir::{
    KirEvidence, KirGraph, KirId, KirObject, KirRelationship, ObjectKind, RelationshipKind,
    SourceLocation,
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use uuid::Uuid;

pub const KIND: &str = "ConflictingEvidence";
/// conflict → the object it is about.
pub const DISPUTES: &str = "Disputes";

pub const OPEN: &str = "open";
pub const RESOLVED: &str = "resolved";
pub const DISMISSED: &str = "dismissed";

pub const DUPLICATE_DEFINITION: &str = "duplicate_definition";
pub const MERGE_LOSS: &str = "merge_loss";
pub const LABEL_MISMATCH: &str = "label_mismatch";
/// Phase 3: a column's documentation makes a checkable claim the measured data contradicts.
pub const DOC_VS_DATA: &str = "doc_vs_data";

/// Review state carried across commits; never part of the signature.
pub const REVIEW_FIELDS: [&str; 7] = [
    "status",
    "resolution",
    "picked_claim",
    "reviewed_by",
    "reviewed_at",
    "review_note",
    "reviewed_signature",
];

fn kid(key: &str) -> KirId {
    KirId(Uuid::new_v5(&Uuid::NAMESPACE_URL, key.as_bytes()))
}

/// Whether `o` is a conflict object.
pub fn is_conflict(o: &KirObject) -> bool {
    matches!(&o.kind, ObjectKind::Custom(k) if k == KIND)
}

// ── Normalization ───────────────────────────────────────────────────────────────────────────

/// A SQL type in one canonical spelling, so aliases never count as a disagreement.
pub fn normalize_type(t: &str) -> String {
    let t = t.trim().to_lowercase();
    let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
    let (base, args) = match t.find('(') {
        Some(i) => (t[..i].trim().to_string(), t[i..].replace(' ', "")),
        None => (t.clone(), String::new()),
    };
    let base = match base.as_str() {
        "int" | "integer" | "int4" | "serial" | "serial4" => "int",
        "bigint" | "int8" | "bigserial" | "serial8" => "bigint",
        "smallint" | "int2" | "smallserial" | "serial2" => "smallint",
        "bool" | "boolean" => "bool",
        "varchar" | "character varying" => "varchar",
        "char" | "character" | "bpchar" => "char",
        "decimal" | "numeric" => "numeric",
        "float8" | "double precision" | "double" => "double",
        "float4" | "real" => "real",
        "timestamp" | "timestamp without time zone" => "timestamp",
        "timestamptz" | "timestamp with time zone" => "timestamptz",
        "time" | "time without time zone" => "time",
        "timetz" | "time with time zone" => "timetz",
        other => other,
    };
    format!("{base}{args}")
}

fn normalize_expr(e: &str) -> String {
    e.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// The comparable facts an object states, by attribute. Only what is stated is included.
fn facts(o: &KirObject) -> BTreeMap<String, Value> {
    let mut out = BTreeMap::new();
    if let Some(cols) = o.properties.get("columns").and_then(Value::as_array) {
        let mut names = BTreeSet::new();
        for c in cols {
            let Some(name) = c.get("name").and_then(Value::as_str) else {
                continue;
            };
            let n = name.trim_matches('"').to_lowercase();
            if let Some(t) = c.get("data_type").and_then(Value::as_str) {
                out.insert(format!("columns.{n}.data_type"), json!(normalize_type(t)));
            }
            for flag in ["not_null", "primary_key"] {
                if let Some(b) = c.get(flag).and_then(Value::as_bool) {
                    out.insert(format!("columns.{n}.{flag}"), json!(b));
                }
            }
            names.insert(n);
        }
        if !names.is_empty() {
            out.insert("columns".into(), json!(names));
        }
    }
    if let Some(cs) = o
        .properties
        .get("check_constraints")
        .and_then(Value::as_array)
    {
        let exprs: BTreeSet<String> = cs
            .iter()
            .filter_map(|c| c.get("expression").and_then(Value::as_str))
            .map(normalize_expr)
            .collect();
        if !exprs.is_empty() {
            out.insert("check_constraints".into(), json!(exprs));
        }
    }
    out
}

// ── Building conflicts ──────────────────────────────────────────────────────────────────────

/// One source's side of a disagreement.
#[derive(Debug, Clone, PartialEq)]
pub struct Claim {
    pub value: Value,
    pub path: String,
    pub line: Option<u32>,
    pub source: String,
}

/// A file and line.
type Origin = (String, Option<u32>);

/// Where an object came from: its first evidence record that names a file.
fn origin(o: &KirObject, evidence: &HashMap<KirId, &KirEvidence>) -> Origin {
    o.evidence
        .iter()
        .filter_map(|id| evidence.get(id))
        .map(|e| (e.location.path.clone(), e.location.line))
        .find(|(p, _)| !p.is_empty())
        .unwrap_or_default()
}

/// The signature of a conflict: its attribute and what each claim says, where. Lines are left out
/// so an edit above the definition does not reopen a review (the RFC 0170 rule).
fn signature(attribute: &str, claims: &[Claim]) -> String {
    let c: Vec<Value> = claims
        .iter()
        .map(|c| json!([c.value, c.path, c.source]))
        .collect();
    ekos_common::ContentHash::of_str(&json!({"attribute": attribute, "claims": c}).to_string())
        .as_str()
        .to_string()
}

/// Add one conflict (object, one evidence per claim, `Disputes`) to `out`.
fn emit(
    out: &mut KirGraph,
    subject: &KirObject,
    attribute: &str,
    conflict_type: &str,
    mut claims: Vec<Claim>,
    chosen: Option<Value>,
) {
    claims.sort_by(|a, b| {
        (&a.path, a.line, a.value.to_string()).cmp(&(&b.path, b.line, b.value.to_string()))
    });
    let id = kid(&format!("conflicting-evidence:{}:{attribute}", subject.id));
    let mut o = KirObject::new(
        format!("{}.{attribute}", subject.name),
        ObjectKind::Custom(KIND.into()),
    );
    o.id = id;
    for (i, c) in claims.iter().enumerate() {
        let mut ev = KirEvidence::new(
            SourceLocation {
                path: c.path.clone(),
                line: c.line,
                column: None,
            },
            format!("{} {attribute} = {}", subject.name, c.value),
        );
        ev.id = kid(&format!("conflicting-evidence-claim:{id}:{i}"));
        o.evidence.push(ev.id);
        out.evidence.push(ev);
    }
    let props = [
        ("subject_id", json!(subject.id.to_string())),
        ("subject_name", json!(subject.name)),
        ("subject_kind", json!(kind_label(&subject.kind))),
        ("attribute", json!(attribute)),
        ("conflict_type", json!(conflict_type)),
        (
            "claims",
            json!(
                claims
                    .iter()
                    .map(|c| json!({"value": c.value, "path": c.path, "line": c.line, "source": c.source}))
                    .collect::<Vec<_>>()
            ),
        ),
        ("chosen", chosen.unwrap_or(Value::Null)),
        ("signature", json!(signature(attribute, &claims))),
        ("status", json!(OPEN)),
    ];
    for (k, v) in props {
        o.properties.insert(k.into(), v);
    }
    out.relationships.push(KirRelationship::deterministic(
        RelationshipKind::Custom(DISPUTES.into()),
        id,
        subject.id,
        attribute,
    ));
    out.objects.push(o);
}

fn kind_label(kind: &ObjectKind) -> String {
    match kind {
        ObjectKind::Custom(k) => k.clone(),
        other => format!("{other:?}"),
    }
}

/// One conflict about `subject`, as a graph to append — for detectors outside this module (RFC 0172
/// Phase 3, `ekos migrate assess`). Same id rule, evidence and `Disputes` link as every other.
pub fn conflict(
    subject: &KirObject,
    attribute: &str,
    conflict_type: &str,
    claims: Vec<Claim>,
    chosen: Option<Value>,
) -> KirGraph {
    let mut out = KirGraph::new();
    emit(&mut out, subject, attribute, conflict_type, claims, chosen);
    out
}

/// Compare `members` (the last one is what EKOS keeps) and emit one conflict per attribute on
/// which the members that state it disagree.
fn compare_members(
    out: &mut KirGraph,
    subject: &KirObject,
    members: &[&KirObject],
    conflict_type: &str,
    evidence: &HashMap<KirId, &KirEvidence>,
) {
    let per_member: Vec<(BTreeMap<String, Value>, Origin)> = members
        .iter()
        .map(|m| (facts(m), origin(m, evidence)))
        .collect();
    let attributes: BTreeSet<&String> = per_member.iter().flat_map(|(f, _)| f.keys()).collect();
    for attr in attributes {
        let stating: Vec<(&Value, &(String, Option<u32>))> = per_member
            .iter()
            .filter_map(|(f, o)| f.get(attr).map(|v| (v, o)))
            .collect();
        let distinct: BTreeSet<String> = stating.iter().map(|(v, _)| v.to_string()).collect();
        if stating.len() < 2 || distinct.len() < 2 {
            continue;
        }
        let claims = stating
            .iter()
            .map(|(v, (path, line))| Claim {
                value: (*v).clone(),
                path: path.clone(),
                line: *line,
                source: kind_label(&subject.kind),
            })
            .collect();
        let chosen = facts(members[members.len() - 1]).get(attr).cloned();
        emit(out, subject, attr, conflict_type, claims, chosen);
    }
}

/// Detector 1. Objects sharing an id collapse to the last one (path order, devlog_242); every
/// attribute on which they disagree becomes a conflict, returned as a graph to add.
pub fn duplicate_definitions(graph: &mut KirGraph) -> KirGraph {
    let mut out = KirGraph::new();
    let mut positions: HashMap<KirId, Vec<usize>> = HashMap::new();
    for (i, o) in graph.objects.iter().enumerate() {
        positions.entry(o.id).or_default().push(i);
    }
    let mut groups: Vec<Vec<usize>> = positions.into_values().filter(|v| v.len() > 1).collect();
    if groups.is_empty() {
        return out;
    }
    groups.sort();
    {
        let evidence: HashMap<KirId, &KirEvidence> =
            graph.evidence.iter().map(|e| (e.id, e)).collect();
        for g in &groups {
            let members: Vec<&KirObject> = g.iter().map(|&i| &graph.objects[i]).collect();
            let subject = members[members.len() - 1];
            compare_members(&mut out, subject, &members, DUPLICATE_DEFINITION, &evidence);
        }
    }
    let drop: BTreeSet<usize> = groups
        .iter()
        .flat_map(|g| g[..g.len() - 1].iter().copied())
        .collect();
    let mut i = 0;
    graph.objects.retain(|_| {
        let keep = !drop.contains(&i);
        i += 1;
        keep
    });
    out
}

/// Detector 2. For each exact-name merge group about to be folded into its canonical object, the
/// attributes on which the members disagree.
pub fn merge_losses(graph: &KirGraph, proposals: &[MergeProposal]) -> KirGraph {
    let mut out = KirGraph::new();
    let by_id: HashMap<KirId, &KirObject> = graph.objects.iter().map(|o| (o.id, o)).collect();
    let evidence: HashMap<KirId, &KirEvidence> = graph.evidence.iter().map(|e| (e.id, e)).collect();
    let mut sorted: Vec<&MergeProposal> = proposals.iter().filter(|p| p.exact_name_match).collect();
    sorted.sort_by_key(|p| p.canonical_id.to_string());
    for p in sorted {
        let Some(canonical) = by_id.get(&p.canonical_id) else {
            continue;
        };
        let mut members: Vec<&KirObject> = p
            .source_ids
            .iter()
            .filter(|id| **id != p.canonical_id)
            .filter_map(|id| by_id.get(id).copied())
            .collect();
        members.sort_by_key(|m| m.id.to_string());
        if members.is_empty() {
            continue;
        }
        members.push(canonical); // last = kept
        compare_members(&mut out, canonical, &members, MERGE_LOSS, &evidence);
    }
    out
}

fn label_words(label: &str) -> BTreeSet<String> {
    let mut spaced = String::new();
    let mut prev_lower = false;
    for ch in label.chars() {
        if ch.is_uppercase() && prev_lower {
            spaced.push(' ');
        }
        prev_lower = ch.is_lowercase();
        spaced.push(ch);
    }
    spaced
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

/// One label's sources, and the first place it was read: (sources, path, line).
type LabelSources = (BTreeSet<String>, String, Option<u32>);

/// Detector 3. An `EnumMeaning` whose labels from *different* sources share no word (`asset` from
/// a column comment vs `L` from a CASE that really rewrites the code). Same-source variants and
/// spellings of one label (`Sales Order` / `sales_order`) are not conflicts.
pub fn label_mismatches(enums: &[KirObject]) -> KirGraph {
    let mut out = KirGraph::new();
    let mut sorted: Vec<&KirObject> = enums
        .iter()
        .filter(|o| matches!(&o.kind, ObjectKind::Custom(k) if k == "EnumMeaning"))
        .collect();
    sorted.sort_by_key(|o| o.id.to_string());
    for e in sorted {
        let Some(meanings) = e.properties.get("meanings").and_then(Value::as_array) else {
            continue;
        };
        // label → (sources, first path/line)
        let mut labels: BTreeMap<String, LabelSources> = BTreeMap::new();
        for m in meanings {
            let Some(label) = m.get("label").and_then(Value::as_str).map(str::trim) else {
                continue;
            };
            if label.is_empty() {
                continue;
            }
            let entry = labels.entry(label.to_string()).or_insert_with(|| {
                (
                    BTreeSet::new(),
                    m.get("path")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    m.get("line").and_then(Value::as_u64).map(|l| l as u32),
                )
            });
            if let Some(s) = m.get("source").and_then(Value::as_str) {
                entry.0.insert(s.to_string());
            }
        }
        let list: Vec<(&String, &LabelSources)> = labels.iter().collect();
        let clash = list.iter().enumerate().any(|(i, (a, (sa, _, _)))| {
            list[i + 1..].iter().any(|(b, (sb, _, _))| {
                sa.is_disjoint(sb) && label_words(a).is_disjoint(&label_words(b))
            })
        });
        if !clash {
            continue;
        }
        let claims = list
            .iter()
            .map(|(label, (sources, path, line))| Claim {
                value: json!(label),
                path: path.clone(),
                line: *line,
                source: sources.iter().cloned().collect::<Vec<_>>().join(", "),
            })
            .collect();
        emit(
            &mut out,
            e,
            "label",
            LABEL_MISMATCH,
            claims,
            e.properties.get("label").cloned(),
        );
    }
    out
}

// ── Review ──────────────────────────────────────────────────────────────────────────────────

fn status(o: &KirObject) -> &str {
    o.properties
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or(OPEN)
}

/// Bring the ledger's review of `current` onto a freshly detected conflict. The review holds while
/// the claims are unchanged; otherwise the conflict reopens with the old decision kept.
pub fn carry_forward(fresh: &mut KirObject, current: Option<&KirObject>) {
    let Some(current) = current else { return };
    if status(current) == OPEN {
        return;
    }
    let unchanged =
        current.properties.get("reviewed_signature") == fresh.properties.get("signature");
    if unchanged {
        for f in REVIEW_FIELDS {
            if let Some(v) = current.properties.get(f) {
                fresh.properties.insert(f.into(), v.clone());
            }
        }
    } else {
        fresh.properties.insert(
            "previous_review".into(),
            json!({
                "status": status(current),
                "resolution": current.properties.get("resolution"),
                "reviewed_by": current.properties.get("reviewed_by"),
                "reviewed_at": current.properties.get("reviewed_at"),
                "review_note": current.properties.get("review_note"),
            }),
        );
        fresh.properties.insert(
            "review_reason".into(),
            json!("the claims changed after this conflict was reviewed"),
        );
    }
}

/// A human decision on a conflict.
#[derive(Debug, Clone, PartialEq)]
pub enum Resolution {
    /// The claim at this 1-based position is right.
    Pick(usize),
    /// Both are right: e.g. two facets of one code, or two schema versions on purpose.
    BothValid,
}

/// The new version of `current` after a human decision. The CLI writes it; nothing else may.
pub fn resolve(
    current: &KirObject,
    decision: &Resolution,
    by: &str,
    at: &str,
    note: Option<&str>,
) -> Result<KirObject, String> {
    if !is_conflict(current) {
        return Err("not a ConflictingEvidence item".into());
    }
    if by.trim().is_empty() {
        return Err("a decision needs a reviewer".into());
    }
    let claims = current
        .properties
        .get("claims")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut o = current.clone();
    match decision {
        Resolution::Pick(n) => {
            let claim = claims
                .get(n.wrapping_sub(1))
                .ok_or_else(|| format!("--pick must be 1..={}", claims.len()))?;
            o.properties.insert("status".into(), json!(RESOLVED));
            o.properties.insert("resolution".into(), json!("picked"));
            o.properties.insert("picked_claim".into(), claim.clone());
        }
        Resolution::BothValid => {
            if note.is_none_or(|n| n.trim().is_empty()) {
                return Err("say why both are valid: --both-valid needs --note".into());
            }
            o.properties.insert("status".into(), json!(DISMISSED));
            o.properties
                .insert("resolution".into(), json!("both_valid"));
            o.properties.remove("picked_claim");
        }
    }
    o.properties.insert("reviewed_by".into(), json!(by));
    o.properties.insert("reviewed_at".into(), json!(at));
    o.properties.insert(
        "reviewed_signature".into(),
        current
            .properties
            .get("signature")
            .cloned()
            .unwrap_or(Value::Null),
    );
    match note {
        Some(n) if !n.trim().is_empty() => {
            o.properties.insert("review_note".into(), json!(n));
        }
        _ => {
            o.properties.remove("review_note");
        }
    }
    o.properties.remove("review_reason");
    o.properties.remove("previous_review");
    Ok(o)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(name: &str, path: &str, cols: Value, checks: Value) -> (KirObject, KirEvidence) {
        let ev = KirEvidence::new(
            SourceLocation {
                path: path.into(),
                line: Some(3),
                column: None,
            },
            format!("CREATE TABLE {name}"),
        );
        let mut o = KirObject::new(name, ObjectKind::Table);
        o.id = kid(&format!("t:{name}"));
        o.evidence.push(ev.id);
        o.properties.insert("columns".into(), cols);
        if !checks.is_null() {
            o.properties.insert("check_constraints".into(), checks);
        }
        (o, ev)
    }

    fn graph(items: Vec<(KirObject, KirEvidence)>) -> KirGraph {
        let mut g = KirGraph::new();
        for (o, e) in items {
            g.objects.push(o);
            g.evidence.push(e);
        }
        g
    }

    fn attrs(g: &KirGraph) -> Vec<String> {
        g.objects
            .iter()
            .map(|o| o.properties["attribute"].as_str().unwrap().to_string())
            .collect()
    }

    #[test]
    fn type_aliases_are_one_type() {
        for (a, b) in [
            ("INT", "integer"),
            ("serial", "int4"),
            ("BOOLEAN", "bool"),
            ("character varying(20)", "VARCHAR (20)"),
            ("timestamp with time zone", "timestamptz"),
            ("NUMERIC(10, 2)", "decimal(10,2)"),
        ] {
            assert_eq!(normalize_type(a), normalize_type(b), "{a} vs {b}");
        }
        assert_ne!(normalize_type("int"), normalize_type("bigint"));
        assert_ne!(normalize_type("varchar(20)"), normalize_type("varchar(40)"));
    }

    #[test]
    fn a_table_defined_twice_differently_is_a_conflict_and_collapses_to_the_last() {
        let a = table(
            "user_preference",
            "sql/Pg-database.sql",
            json!([{"name":"id","data_type":"int"},{"name":"language","data_type":"varchar(6)"}]),
            Value::Null,
        );
        let b = table(
            "user_preference",
            "sql/changes/1.9/transpose_user_prefs.sql",
            json!([{"name":"id","data_type":"serial"},{"name":"user_id","data_type":"INT"},{"name":"name","data_type":"text"}]),
            json!([{"expression": "user_id IS NULL OR user_id > 0"}]),
        );
        let mut g = graph(vec![a, b]);
        let c = duplicate_definitions(&mut g);
        assert_eq!(g.objects.len(), 1, "collapsed to one definition");
        assert!(
            g.objects[0].properties["columns"]
                .to_string()
                .contains("user_id"),
            "the later one is kept"
        );
        // id serial vs int is the same type; the column sets differ; only one side states a CHECK.
        assert_eq!(attrs(&c), vec!["columns"]);
        let o = &c.objects[0];
        assert_eq!(o.name, "user_preference.columns");
        assert_eq!(o.properties["conflict_type"], DUPLICATE_DEFINITION);
        assert_eq!(o.properties["status"], OPEN);
        let paths: Vec<&str> = o.properties["claims"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["path"].as_str().unwrap())
            .collect();
        assert_eq!(
            paths,
            vec![
                "sql/Pg-database.sql",
                "sql/changes/1.9/transpose_user_prefs.sql"
            ]
        );
        assert_eq!(c.evidence.len(), 2);
        assert_eq!(c.relationships.len(), 1);
        assert_eq!(c.relationships[0].to, g.objects[0].id);
    }

    #[test]
    fn identical_duplicates_collapse_silently() {
        let cols = json!([{"name":"id","data_type":"int"}]);
        let mut g = graph(vec![
            table("t", "a.sql", cols.clone(), Value::Null),
            table("t", "b.sql", cols, Value::Null),
        ]);
        assert!(duplicate_definitions(&mut g).objects.is_empty());
        assert_eq!(g.objects.len(), 1);
    }

    #[test]
    fn a_column_type_disagreement_is_its_own_conflict() {
        let mut g = graph(vec![
            table(
                "t",
                "a.sql",
                json!([{"name":"amount","data_type":"int","not_null":true}]),
                Value::Null,
            ),
            table(
                "t",
                "b.sql",
                json!([{"name":"AMOUNT","data_type":"numeric(10,2)"}]),
                Value::Null,
            ),
        ]);
        let c = duplicate_definitions(&mut g);
        // not_null is stated by one side only: unknown, not a disagreement.
        assert_eq!(attrs(&c), vec!["columns.amount.data_type"]);
        assert_eq!(c.objects[0].properties["chosen"], "numeric(10,2)");
    }

    #[test]
    fn detection_is_independent_of_input_order_and_ids_are_stable() {
        let mk = || {
            vec![
                table(
                    "t",
                    "a.sql",
                    json!([{"name":"x","data_type":"int"}]),
                    Value::Null,
                ),
                table(
                    "t",
                    "b.sql",
                    json!([{"name":"x","data_type":"text"}]),
                    Value::Null,
                ),
            ]
        };
        let mut g1 = graph(mk());
        let c1 = duplicate_definitions(&mut g1);
        let mut items = mk();
        items.reverse();
        let mut g2 = graph(items);
        let c2 = duplicate_definitions(&mut g2);
        assert_eq!(c1.objects[0].id, c2.objects[0].id);
        assert_eq!(
            c1.objects[0].properties["claims"],
            c2.objects[0].properties["claims"]
        );
        assert_eq!(
            c1.objects[0].properties["signature"],
            c2.objects[0].properties["signature"]
        );
    }

    #[test]
    fn merge_groups_report_what_the_merge_would_drop() {
        let (a, ea) = table(
            "orders",
            "schema.sql",
            json!([{"name":"total","data_type":"int"}]),
            Value::Null,
        );
        let (mut b, eb) = table(
            "orders",
            "models/orders.py",
            json!([{"name":"total","data_type":"bigint"}]),
            Value::Null,
        );
        b.id = kid("orm:orders");
        let g = graph(vec![(a.clone(), ea), (b.clone(), eb)]);
        let p = MergeProposal {
            canonical_id: a.id,
            canonical_name: "orders".into(),
            canonical_kind: ObjectKind::Table,
            source_ids: vec![a.id, b.id],
            confidence: 1.0,
            exact_name_match: true,
        };
        let c = merge_losses(&g, std::slice::from_ref(&p));
        assert_eq!(attrs(&c), vec!["columns.total.data_type"]);
        assert_eq!(c.objects[0].properties["conflict_type"], MERGE_LOSS);
        assert_eq!(
            c.objects[0].properties["chosen"], "int",
            "the canonical is kept"
        );
        let fuzzy = MergeProposal {
            exact_name_match: false,
            ..p
        };
        assert!(
            merge_losses(&g, &[fuzzy]).objects.is_empty(),
            "fuzzy groups are not merged"
        );
    }

    fn enum_meaning(meanings: Value) -> KirObject {
        let mut o = KirObject::new(
            "account.category = 'A'",
            ObjectKind::Custom("EnumMeaning".into()),
        );
        o.id = kid("enum:a");
        o.properties.insert("label".into(), json!("asset"));
        o.properties.insert("meanings".into(), meanings);
        o
    }

    #[test]
    fn labels_from_different_sources_that_share_no_word_conflict() {
        let e = enum_meaning(json!([
            {"label":"asset","source":"column_comment","path":"sql/Pg-database.sql","line":72},
            {"label":"L","source":"case_label","path":"sql/modules/FinStatements.sql","line":690}
        ]));
        let c = label_mismatches(&[e]);
        assert_eq!(attrs(&c), vec!["label"]);
        assert_eq!(c.objects[0].properties["conflict_type"], LABEL_MISMATCH);
        assert_eq!(
            c.objects[0].properties["claims"].as_array().unwrap().len(),
            2
        );
    }

    #[test]
    fn spellings_of_one_label_and_same_source_variants_do_not() {
        let spelled = enum_meaning(json!([
            {"label":"Sales Order","source":"lookup_seed","path":"a.sql"},
            {"label":"sales_order","source":"app_constant","path":"b.pm"},
            {"label":"SalesOrder","source":"case_label","path":"c.sql"}
        ]));
        let same_source = enum_meaning(json!([
            {"label":"asset","source":"column_comment","path":"a.sql"},
            {"label":"holding","source":"column_comment","path":"b.sql"}
        ]));
        assert!(label_mismatches(&[spelled, same_source]).objects.is_empty());
    }

    #[test]
    fn a_review_holds_while_the_claims_are_unchanged_and_reopens_when_they_change() {
        let mut g = graph(vec![
            table(
                "t",
                "a.sql",
                json!([{"name":"x","data_type":"int"}]),
                Value::Null,
            ),
            table(
                "t",
                "b.sql",
                json!([{"name":"x","data_type":"text"}]),
                Value::Null,
            ),
        ]);
        let first = duplicate_definitions(&mut g).objects.remove(0);
        let reviewed = resolve(
            &first,
            &Resolution::Pick(2),
            "ann",
            "2026-10-09",
            Some("b.sql is newer"),
        )
        .unwrap();
        assert_eq!(reviewed.properties["status"], RESOLVED);
        assert_eq!(reviewed.properties["picked_claim"]["path"], "b.sql");

        let mut again = first.clone();
        carry_forward(&mut again, Some(&reviewed));
        assert_eq!(again.properties["status"], RESOLVED);

        let mut g2 = graph(vec![
            table(
                "t",
                "a.sql",
                json!([{"name":"x","data_type":"int"}]),
                Value::Null,
            ),
            table(
                "t",
                "b.sql",
                json!([{"name":"x","data_type":"bigint"}]),
                Value::Null,
            ),
        ]);
        let mut changed = duplicate_definitions(&mut g2).objects.remove(0);
        carry_forward(&mut changed, Some(&reviewed));
        assert_eq!(changed.properties["status"], OPEN);
        assert_eq!(changed.properties["previous_review"]["status"], RESOLVED);
    }

    #[test]
    fn resolve_validates_its_input() {
        let mut g = graph(vec![
            table(
                "t",
                "a.sql",
                json!([{"name":"x","data_type":"int"}]),
                Value::Null,
            ),
            table(
                "t",
                "b.sql",
                json!([{"name":"x","data_type":"text"}]),
                Value::Null,
            ),
        ]);
        let c = duplicate_definitions(&mut g).objects.remove(0);
        assert!(resolve(&c, &Resolution::Pick(3), "ann", "t", None).is_err());
        assert!(resolve(&c, &Resolution::Pick(0), "ann", "t", None).is_err());
        assert!(
            resolve(&c, &Resolution::BothValid, "ann", "t", None).is_err(),
            "needs a note"
        );
        assert!(resolve(&c, &Resolution::Pick(1), " ", "t", None).is_err());
        let d = resolve(
            &c,
            &Resolution::BothValid,
            "ann",
            "t",
            Some("two schema versions"),
        )
        .unwrap();
        assert_eq!(d.properties["status"], DISMISSED);
    }
}
