//! RFC 0163 *Triggers* — link each `Trigger` to the table it fires on and the function it runs, and
//! classify it **structurally**, from that function's recovered IR facts.
//!
//! Runs inside `SemanticCompilerPass::run()` beside RFC 0094's risks and RFC 0144's doc links: a
//! trigger and its function are usually in different files, so both are only together once every
//! analyzer's output is resolved — and running here means each `Trigger` carries its class from its
//! first ledger version.
//!
//! | Class | When |
//! |---|---|
//! | `DerivedColumn` | only sets `NEW` columns |
//! | `Validation` | only raises an exception or returns `NULL` (a `BEFORE` trigger dropping the row) |
//! | `Audit` | only **inserts** into tables other than its own |
//! | `Cascade` | updates or deletes tables other than its own |
//! | `Mixed` | anything else — more than one of the above, dynamic SQL, writing its own table, calling other recovered routines whose effects are not inlined, or no recognised effect |
//! | `Unknown` | the function is not recovered, is ambiguous, is not PL/pgSQL, or is only partially recovered: there is no full body to classify |
//!
//! Deliberately conservative, as the RFC requires: `Mixed` means "a human decides", and RFC 0164
//! treats it so. Classification reads only IR facts (`assigns_new`, `raises_exception`,
//! `returns_null`, `inserts`/`updates`/`deletes`, `calls`, `dynamic_sql_sites`, `fidelity`) —
//! never names — and every class carries the reasons that produced it.

use crate::procedure_lineage::NameIndex;
use ekos_kir::{KirGraph, KirId, KirObject, KirRelationship, ObjectKind, RelationshipKind};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

/// What one run did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TriggerLinkStats {
    pub triggers: usize,
    pub table_links: usize,
    pub function_links: usize,
    /// Triggers per class.
    pub classes: BTreeMap<String, usize>,
}

fn is_custom(o: &KirObject, kind: &str) -> bool {
    matches!(&o.kind, ObjectKind::Custom(k) if k == kind)
}

fn strings(o: &KirObject, key: &str) -> Vec<String> {
    o.properties
        .get(key)
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn tail(n: &str) -> String {
    n.rsplit('.').next().unwrap_or(n).to_lowercase()
}

/// Classify a trigger on `table` running `function` (`None` when unresolved, with why).
/// `callees` are the recovered routines the function calls, other than itself.
fn classify(
    table: &str,
    function: Result<&KirObject, String>,
    callees: &[String],
) -> (&'static str, Vec<String>) {
    let f = match function {
        Ok(f) => f,
        Err(why) => return ("Unknown", vec![why]),
    };
    let prop = |k: &str| f.properties.get(k).cloned().unwrap_or(Value::Null);
    if prop("language").as_str() != Some("plpgsql") {
        return (
            "Unknown",
            vec![format!(
                "function `{}` is {}, whose body is not recovered",
                f.name,
                prop("language").as_str().unwrap_or("an unknown language")
            )],
        );
    }
    if prop("fidelity").as_str() != Some("statements") {
        return (
            "Unknown",
            vec![format!(
                "function `{}` is only partially recovered — its unrecovered statements could do anything",
                f.name
            )],
        );
    }

    let mut classes: BTreeSet<&'static str> = BTreeSet::new();
    let mut reasons = Vec::new();
    let mut mixed = false;

    let new_cols = strings(f, "assigns_new");
    if !new_cols.is_empty() {
        classes.insert("DerivedColumn");
        reasons.push(format!(
            "sets {}",
            new_cols
                .iter()
                .map(|c| format!("NEW.{c}"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    let raises = prop("raises_exception").as_u64().unwrap_or(0);
    if raises > 0 {
        classes.insert("Validation");
        reasons.push(format!("raises an exception ({raises} site(s))"));
    }
    if prop("returns_null").as_bool() == Some(true) {
        classes.insert("Validation");
        reasons.push("returns NULL, dropping the row".into());
    }

    let own = tail(table);
    let mut other = |key: &str| -> Vec<String> {
        let all = strings(f, key);
        if all.iter().any(|t| tail(t) == own) {
            mixed = true;
            reasons.push(format!(
                "{} its own table `{table}`",
                key.trim_end_matches('s')
            ));
        }
        all.into_iter().filter(|t| tail(t) != own).collect()
    };
    let inserts = other("inserts");
    let changes: Vec<String> = other("updates")
        .into_iter()
        .chain(other("deletes"))
        .collect();
    if !changes.is_empty() {
        classes.insert("Cascade");
        reasons.push(format!("updates/deletes {}", changes.join(", ")));
    }
    if !inserts.is_empty() {
        if changes.is_empty() {
            classes.insert("Audit");
        } else {
            classes.insert("Cascade");
        }
        reasons.push(format!("inserts into {}", inserts.join(", ")));
    }

    if prop("dynamic_sql_sites").as_u64().unwrap_or(0) > 0 {
        mixed = true;
        reasons.push("builds SQL dynamically — its target is not statically known".into());
    }
    if !callees.is_empty() {
        mixed = true;
        reasons.push(format!(
            "calls recovered routines whose effects are not inlined: {}",
            callees.join(", ")
        ));
    }

    match (mixed, classes.len()) {
        (false, 1) => (classes.into_iter().next().unwrap(), reasons),
        (false, 0) => (
            "Mixed",
            vec!["no audit, derived-column, validation or cascade effect recognised".into()],
        ),
        _ => ("Mixed", reasons),
    }
}

/// Link every `Trigger` to its table and function, and set `classification` /
/// `classification_reasons` on it.
pub fn link_and_classify_triggers(graph: &mut KirGraph) -> TriggerLinkStats {
    let mut relations = NameIndex::default();
    let mut routines = NameIndex::default();
    for o in &graph.objects {
        if matches!(o.kind, ObjectKind::Table | ObjectKind::Dataset) || is_custom(o, "View") {
            relations.add(&o.name, o.id);
        } else if is_custom(o, "Procedure") {
            routines.add(&o.name, o.id);
        }
    }
    let index: BTreeMap<uuid::Uuid, usize> = graph
        .objects
        .iter()
        .enumerate()
        .map(|(i, o)| (o.id.0, i))
        .collect();
    let by_id = |id: KirId| index.get(&id.0).map(|&i| &graph.objects[i]);

    let mut stats = TriggerLinkStats::default();
    let mut updates: Vec<(usize, &'static str, Vec<String>)> = Vec::new();
    let mut edges = Vec::new();
    for (i, t) in graph.objects.iter().enumerate() {
        if !is_custom(t, "Trigger") {
            continue;
        }
        stats.triggers += 1;
        let table = t
            .properties
            .get("table")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if let Ok((tid, how)) = relations.resolve(&table) {
            let mut rel = KirRelationship::deterministic(
                RelationshipKind::DependsOn,
                t.id,
                tid,
                "trigger-fires-on",
            );
            rel.properties.insert("role".into(), json!("fires_on"));
            rel.properties.insert("match".into(), json!(how));
            rel.evidence = t.evidence.clone();
            edges.push(rel);
            stats.table_links += 1;
        }

        let function_name = t.properties.get("function").and_then(Value::as_str);
        let function = match function_name {
            None => Err("the trigger's function could not be read from its definition".to_string()),
            Some(name) => match routines.resolve(name) {
                Ok((fid, how)) => {
                    let mut rel = KirRelationship::deterministic(
                        RelationshipKind::Calls,
                        t.id,
                        fid,
                        "trigger-executes",
                    );
                    rel.properties.insert("match".into(), json!(how));
                    rel.evidence = t.evidence.clone();
                    edges.push(rel);
                    stats.function_links += 1;
                    by_id(fid).ok_or_else(|| format!("function `{name}` not recovered"))
                }
                Err(true) => Err(format!(
                    "function `{name}` is defined more than once — which body runs is not known"
                )),
                Err(false) => Err(format!(
                    "function `{name}` is not among the recovered routines"
                )),
            },
        };
        // A call counts when it names any recovered routine other than the function itself —
        // a built-in names none, and an ambiguous callee's effects are no more inlined.
        let callees_of = |f: &KirObject| -> Vec<String> {
            strings(f, "calls")
                .into_iter()
                .filter(|c| !routines.candidates(c).iter().all(|id| *id == f.id))
                .collect()
        };
        let (class, reasons) = match (&function, function_name) {
            // Defined in several files: which body runs is not known — but if every definition
            // classifies the same, the class holds whichever one runs.
            (Err(_), Some(name)) if routines.candidates(name).len() > 1 => {
                // A pass-through placeholder (`RETURN NEW` and nothing else) carries no logic: when
                // a real definition exists, the placeholders are set aside — and said so.
                let all: Vec<&KirObject> = routines
                    .candidates(name)
                    .into_iter()
                    .filter_map(by_id)
                    .collect();
                let is_stub = |f: &&KirObject| {
                    f.properties.get("pass_through").and_then(Value::as_bool) == Some(true)
                };
                let real: Vec<&KirObject> = all.iter().copied().filter(|f| !is_stub(f)).collect();
                // Placeholders are set aside only when a real definition remains to classify.
                let set_aside = if real.is_empty() {
                    0
                } else {
                    all.len() - real.len()
                };
                let defs = if real.is_empty() { all } else { real };
                let each: Vec<(&KirObject, (&'static str, Vec<String>))> = defs
                    .into_iter()
                    .map(|f| (f, classify(&table, Ok(f), &callees_of(f))))
                    .collect();
                let note_stubs = |reasons: &mut Vec<String>| {
                    if set_aside > 0 {
                        reasons.push(format!(
                            "set aside {set_aside} placeholder definition(s) whose body is empty or only `RETURN NEW|OLD`"
                        ));
                    }
                };
                let classes: BTreeSet<&str> = each.iter().map(|(_, (c, _))| *c).collect();
                if classes.len() == 1 {
                    let only = each[0].1.0;
                    let mut reasons = each[0].1.1.clone();
                    if each.len() > 1 {
                        reasons.push(format!(
                            "`{name}` has {} definitions; all classify as {only}",
                            each.len()
                        ));
                    }
                    note_stubs(&mut reasons);
                    (only, reasons)
                } else {
                    let per: Vec<String> = each
                        .iter()
                        .map(|(f, (c, _))| {
                            let path = f
                                .properties
                                .get("source_path")
                                .and_then(Value::as_str)
                                .unwrap_or("?");
                            format!("{c} ({path})")
                        })
                        .collect();
                    let mut reasons = vec![format!(
                        "`{name}` has {} definitions that classify differently: {}",
                        each.len(),
                        per.join(", ")
                    )];
                    note_stubs(&mut reasons);
                    ("Mixed", reasons)
                }
            }
            _ => {
                let callees = match &function {
                    Ok(f) => callees_of(f),
                    Err(_) => Vec::new(),
                };
                classify(&table, function, &callees)
            }
        };
        *stats.classes.entry(class.to_string()).or_default() += 1;
        updates.push((i, class, reasons));
    }
    for (i, class, reasons) in updates {
        let o = &mut graph.objects[i];
        o.properties.insert("classification".into(), json!(class));
        o.properties
            .insert("classification_reasons".into(), json!(reasons));
    }
    graph.relationships.extend(edges);
    stats
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obj(g: &mut KirGraph, name: &str, kind: &str, props: Value) -> KirId {
        let mut o = KirObject::new(
            name,
            if kind == "Table" {
                ObjectKind::Table
            } else {
                ObjectKind::Custom(kind.into())
            },
        );
        if let Value::Object(m) = props {
            o.properties = m.into_iter().collect();
        }
        g.add_object(o)
    }

    fn function(g: &mut KirGraph, name: &str, facts: Value) -> KirId {
        let mut props = json!({
            "language": "plpgsql", "fidelity": "statements", "assigns_new": [],
            "raises_exception": 0, "returns_null": false, "inserts": [], "updates": [],
            "deletes": [], "calls": [], "dynamic_sql_sites": 0,
        });
        for (k, v) in facts.as_object().unwrap() {
            props[k] = v.clone();
        }
        obj(g, name, "Procedure", props)
    }

    fn trigger(g: &mut KirGraph, table: &str, func: &str) -> KirId {
        obj(
            g,
            "tr",
            "Trigger",
            json!({"table": table, "function": func}),
        )
    }

    fn class_of(g: &KirGraph, id: KirId) -> (String, Vec<String>) {
        let t = g.objects.iter().find(|o| o.id == id).unwrap();
        (
            t.properties["classification"].as_str().unwrap().to_string(),
            strings(t, "classification_reasons"),
        )
    }

    fn classify_one(facts: Value) -> (String, Vec<String>) {
        let mut g = KirGraph::new();
        obj(&mut g, "invoice", "Table", json!({}));
        obj(&mut g, "helper", "Procedure", json!({"language": "sql"}));
        function(&mut g, "f", facts);
        let t = trigger(&mut g, "invoice", "f");
        link_and_classify_triggers(&mut g);
        class_of(&g, t)
    }

    #[test]
    fn each_class_comes_from_one_kind_of_effect() {
        assert_eq!(
            classify_one(json!({"assigns_new": ["updated"]})).0,
            "DerivedColumn"
        );
        assert_eq!(classify_one(json!({"raises_exception": 1})).0, "Validation");
        assert_eq!(classify_one(json!({"returns_null": true})).0, "Validation");
        assert_eq!(classify_one(json!({"inserts": ["audit_log"]})).0, "Audit");
        assert_eq!(classify_one(json!({"updates": ["balance"]})).0, "Cascade");
        assert_eq!(
            classify_one(json!({"deletes": ["line"], "inserts": ["history"]})).0,
            "Cascade",
            "a cascade that also logs is still a cascade"
        );
        // Raising *and* inserting into another table — validation plus audit: a human decides.
        let (class, reasons) = classify_one(json!({"raises_exception": 2, "inserts": ["log"]}));
        assert_eq!(class, "Mixed");
        assert!(
            reasons.iter().any(|r| r.contains("raises"))
                && reasons.iter().any(|r| r.contains("inserts into log"))
        );
    }

    #[test]
    fn anything_not_clearly_one_class_is_mixed_with_its_reasons() {
        let (c, r) = classify_one(json!({"inserts": ["invoice"]}));
        assert_eq!(c, "Mixed");
        assert!(r[0].contains("its own table"), "{r:?}");
        let (c, r) = classify_one(json!({"dynamic_sql_sites": 1, "inserts": ["log"]}));
        assert_eq!(c, "Mixed");
        assert!(r.iter().any(|x| x.contains("dynamically")), "{r:?}");
        let (c, r) = classify_one(json!({"inserts": ["log"], "calls": ["helper", "coalesce"]}));
        assert_eq!(c, "Mixed");
        assert!(
            r.iter()
                .any(|x| x.contains("helper") && !x.contains("coalesce")),
            "{r:?}"
        );
        let (c, r) = classify_one(json!({}));
        assert_eq!(c, "Mixed");
        assert!(r[0].contains("no audit"), "{r:?}");
    }

    #[test]
    fn a_function_without_a_full_body_is_unknown_never_guessed() {
        assert_eq!(classify_one(json!({"fidelity": "partial"})).0, "Unknown");
        assert_eq!(classify_one(json!({"language": "c"})).0, "Unknown");
        let mut g = KirGraph::new();
        obj(&mut g, "invoice", "Table", json!({}));
        let t = trigger(&mut g, "invoice", "missing_fn");
        link_and_classify_triggers(&mut g);
        let (c, r) = class_of(&g, t);
        assert_eq!(c, "Unknown");
        assert!(r[0].contains("missing_fn"), "{r:?}");
    }

    /// A function defined in several files (a module and its migrations): which body runs is not
    /// known, but when every definition classifies the same, the class holds whichever runs.
    /// When they disagree, a human decides — and is told each definition's class.
    #[test]
    fn several_definitions_classify_only_when_they_all_agree() {
        let mut g = KirGraph::new();
        obj(&mut g, "invoice", "Table", json!({}));
        function(&mut g, "f", json!({"raises_exception": 1}));
        function(&mut g, "f", json!({"raises_exception": 2}));
        let t = trigger(&mut g, "invoice", "f");
        link_and_classify_triggers(&mut g);
        let (c, r) = class_of(&g, t);
        assert_eq!(c, "Validation");
        assert!(r.iter().any(|x| x.contains("2 definitions")), "{r:?}");

        let mut g = KirGraph::new();
        obj(&mut g, "invoice", "Table", json!({}));
        function(&mut g, "f", json!({"raises_exception": 1}));
        function(&mut g, "f", json!({"inserts": ["log"]}));
        let t = trigger(&mut g, "invoice", "f");
        link_and_classify_triggers(&mut g);
        let (c, r) = class_of(&g, t);
        assert_eq!(c, "Mixed");
        assert!(
            r.iter()
                .any(|x| x.contains("Validation") && x.contains("Audit")),
            "{r:?}"
        );
        // No single `Calls` edge is invented for an ambiguous function.
        assert!(
            !g.relationships
                .iter()
                .any(|x| x.from == t && x.kind == RelationshipKind::Calls)
        );
    }

    /// LedgerSMB creates triggers against `RETURN NEW` placeholders and replaces the function
    /// later; a placeholder has no logic, so it does not outvote the real body.
    #[test]
    fn a_pass_through_placeholder_does_not_outvote_the_real_definition() {
        let mut g = KirGraph::new();
        obj(&mut g, "acc_trans", "Table", json!({}));
        function(&mut g, "prevent_closed", json!({"pass_through": true}));
        function(&mut g, "prevent_closed", json!({"raises_exception": 1}));
        let t = trigger(&mut g, "acc_trans", "prevent_closed");
        link_and_classify_triggers(&mut g);
        let (c, r) = class_of(&g, t);
        assert_eq!(c, "Validation", "{r:?}");
        assert!(r.iter().any(|x| x.contains("placeholder")), "{r:?}");

        // Only placeholders: classified on their own terms, never invented.
        let mut g = KirGraph::new();
        obj(&mut g, "acc_trans", "Table", json!({}));
        function(&mut g, "p", json!({"pass_through": true}));
        function(&mut g, "p", json!({"pass_through": true}));
        let t = trigger(&mut g, "acc_trans", "p");
        link_and_classify_triggers(&mut g);
        let (c, r) = class_of(&g, t);
        assert_eq!(c, "Mixed");
        assert!(
            !r.iter().any(|x| x.contains("set aside")),
            "nothing was set aside: {r:?}"
        );
    }

    #[test]
    fn a_trigger_links_to_its_table_and_its_function() {
        let mut g = KirGraph::new();
        let table = obj(&mut g, "invoice", "Table", json!({}));
        let f = function(&mut g, "f", json!({"assigns_new": ["x"]}));
        let t = trigger(&mut g, "public.invoice", "f");
        let stats = link_and_classify_triggers(&mut g);
        let to = |kind: RelationshipKind| {
            g.relationships
                .iter()
                .find(|r| r.from == t && r.kind == kind)
                .map(|r| r.to)
        };
        assert_eq!(to(RelationshipKind::DependsOn), Some(table));
        assert_eq!(to(RelationshipKind::Calls), Some(f));
        assert_eq!(stats.classes["DerivedColumn"], 1);
        // Deterministic: the same graph links to the same edge ids.
        let ids: Vec<_> = g.relationships.iter().map(|r| r.id).collect();
        g.relationships.clear();
        link_and_classify_triggers(&mut g);
        assert_eq!(
            ids,
            g.relationships.iter().map(|r| r.id).collect::<Vec<_>>()
        );
    }
}
