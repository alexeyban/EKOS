//! RFC 0233 Phase 2 — graph-shape findings: dependency cycles between modules, and relations
//! nothing uses.
//!
//! **Cycles** are strongly connected components of the module-level `DependsOn` graph (crates,
//! Perl packages, Elixir modules, Java types) with two or more members — one finding per cycle,
//! never per edge. **Orphans** are file-defined `Table`/`View` objects that no routine, view, code,
//! transformation or foreign key touches: only structural edges (`Contains`, review links,
//! migration history) reach them.

use crate::xref::{
    Candidate, Diagnostic, DiagnosticKind, Options, Severity, SourceRef, TargetSpec, custom_kind,
    is_system_relation, locate_object, looks_like_migration, normalize, str_prop, tail_of,
    to_candidate,
};
use ekos_kir::{KirId, KirObject, ObjectKind, RelationshipKind};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use uuid::Uuid;

/// Module kinds whose `DependsOn` edges form the cycle graph.
const MODULE_KINDS: &[&str] = &["Crate", "PerlPackage", "ElixirModule", "JavaType"];

/// Relationship kinds that say nothing about use: an object reached only by these is an orphan.
const STRUCTURAL: &[&str] = &[
    "Contains",
    "Disputes",
    "SameAs",
    "Changes",
    "TicketFor",
    "Describes",
    "DocumentedBy",
];

/// What the graph checks found.
#[derive(Default)]
pub(crate) struct GraphFindings {
    pub(crate) diagnostics: Vec<Diagnostic>,
    pub(crate) cycles: usize,
    pub(crate) orphans: BTreeMap<String, usize>,
}

fn is_module(o: &KirObject) -> bool {
    custom_kind(o).is_some_and(|k| MODULE_KINDS.contains(&k))
}

fn kind_label(k: &RelationshipKind) -> String {
    match k {
        RelationshipKind::Custom(s) => s.clone(),
        other => format!("{other:?}"),
    }
}

fn fingerprint(kind: DiagnosticKind, key: &str) -> String {
    ekos_common::ContentHash::of_str(&format!("xref/v1|{}|{key}", kind.as_str()))
        .as_str()
        .to_string()
}

fn source_of(
    o: &KirObject,
    opts: &Options<'_>,
    declaring: &HashMap<Uuid, String>,
    extractor: &str,
    access: &str,
) -> SourceRef {
    let located = locate_object(opts, o);
    SourceRef {
        object_id: o.id,
        object_kind: o.kind.to_string(),
        object_name: o.name.clone(),
        file: str_prop(o, "source_path")
            .or_else(|| located.as_ref().map(|l| l.0.clone()))
            .or_else(|| declaring.get(&o.id.0).cloned()),
        line: located.and_then(|l| l.1),
        extractor: extractor.to_string(),
        access: access.to_string(),
    }
}

/// `referenced`: the last name part of every relation some reference names.
pub(crate) fn check(
    objects: &[KirObject],
    opts: &Options<'_>,
    referenced: &BTreeSet<String>,
) -> GraphFindings {
    let mut out = GraphFindings::default();
    let by_id: HashMap<KirId, &KirObject> = objects.iter().map(|o| (o.id, o)).collect();
    let declaring = crate::xref::declaring_files(objects, opts.relationships);
    cycles(&by_id, opts, &declaring, &mut out);
    // Without the relationships nothing can be said about use.
    if !opts.relationships.is_empty() {
        orphans(objects, opts, &declaring, referenced, &mut out);
    }
    out
}

/// Tarjan's strongly connected components, iterative, over ids in a stable order.
fn components(nodes: &[Uuid], edges: &BTreeMap<Uuid, BTreeSet<Uuid>>) -> Vec<Vec<Uuid>> {
    let mut index: HashMap<Uuid, usize> = HashMap::new();
    let mut low: HashMap<Uuid, usize> = HashMap::new();
    let mut on_stack: BTreeSet<Uuid> = BTreeSet::new();
    let mut stack: Vec<Uuid> = Vec::new();
    let mut out = Vec::new();
    let mut next = 0usize;
    // Indexable successor lists: `nth` on a set would make each step linear in the degree.
    let adjacency: HashMap<Uuid, Vec<Uuid>> = edges
        .iter()
        .map(|(k, v)| (*k, v.iter().copied().collect()))
        .collect();
    let empty: Vec<Uuid> = Vec::new();
    for &start in nodes {
        if index.contains_key(&start) {
            continue;
        }
        // (node, iterator position over its successors)
        let mut work: Vec<(Uuid, usize)> = vec![(start, 0)];
        index.insert(start, next);
        low.insert(start, next);
        next += 1;
        stack.push(start);
        on_stack.insert(start);
        while let Some(&(v, pos)) = work.last() {
            let succ = adjacency.get(&v).unwrap_or(&empty);
            if let Some(&w) = succ.get(pos) {
                if let Some(top) = work.last_mut() {
                    top.1 += 1;
                }
                if let std::collections::hash_map::Entry::Vacant(slot) = index.entry(w) {
                    slot.insert(next);
                    low.insert(w, next);
                    next += 1;
                    stack.push(w);
                    on_stack.insert(w);
                    work.push((w, 0));
                } else if on_stack.contains(&w) {
                    let lw = index[&w];
                    let lv = low.get_mut(&v).expect("visited");
                    *lv = (*lv).min(lw);
                }
            } else {
                work.pop();
                if let Some(&(parent, _)) = work.last() {
                    let lv = low[&v];
                    let lp = low.get_mut(&parent).expect("visited");
                    *lp = (*lp).min(lv);
                }
                if low[&v] == index[&v] {
                    let mut comp = Vec::new();
                    while let Some(w) = stack.pop() {
                        on_stack.remove(&w);
                        comp.push(w);
                        if w == v {
                            break;
                        }
                    }
                    out.push(comp);
                }
            }
        }
    }
    out
}

fn cycles(
    by_id: &HashMap<KirId, &KirObject>,
    opts: &Options<'_>,
    declaring: &HashMap<Uuid, String>,
    out: &mut GraphFindings,
) {
    let mut edges: BTreeMap<Uuid, BTreeSet<Uuid>> = BTreeMap::new();
    for r in opts.relationships {
        if r.kind != RelationshipKind::DependsOn || r.from == r.to {
            continue;
        }
        let (Some(a), Some(b)) = (by_id.get(&r.from), by_id.get(&r.to)) else {
            continue;
        };
        if is_module(a) && is_module(b) {
            edges.entry(r.from.0).or_default().insert(r.to.0);
        }
    }
    let nodes: Vec<Uuid> = edges.keys().copied().collect();
    for comp in components(&nodes, &edges) {
        if comp.len() < 2 {
            continue;
        }
        let mut members: Vec<&KirObject> = comp
            .iter()
            .filter_map(|u| by_id.get(&KirId(*u)).copied())
            .collect();
        members.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.0.cmp(&b.id.0)));
        let Some(first) = members.first() else {
            continue;
        };
        out.cycles += 1;
        let names: Vec<&str> = members.iter().map(|o| o.name.as_str()).collect();
        let shown = if names.len() > 8 {
            format!("{} and {} more", names[..8].join(" → "), names.len() - 8)
        } else {
            names.join(" → ")
        };
        let key = format!("{}|{}", first.kind, names.join("|").to_lowercase());
        out.diagnostics.push(Diagnostic {
            kind: DiagnosticKind::ReferenceCycle,
            severity: Severity::Warning,
            confidence: 0.9,
            source: source_of(first, opts, declaring, "module-dependencies", "depends"),
            target_spec: TargetSpec {
                written: shown.clone(),
                normalized: names.join("|").to_lowercase(),
                expected: "acyclic".to_string(),
                namespace: None,
            },
            candidates: members.iter().skip(1).map(|o| to_candidate(o)).collect(),
            evidence: first.evidence.clone(),
            coverage_gap: false,
            cross_system: false,
            reason: format!(
                "{} {}s depend on each other in a cycle: {shown}",
                members.len(),
                first.kind
            ),
            fingerprint: fingerprint(DiagnosticKind::ReferenceCycle, &key),
        });
    }
}

fn orphans(
    objects: &[KirObject],
    opts: &Options<'_>,
    declaring: &HashMap<Uuid, String>,
    referenced: &BTreeSet<String>,
    out: &mut GraphFindings,
) {
    // The application model's generated class for a table is code that uses it (RFC 0209).
    let modelled: BTreeSet<String> = objects
        .iter()
        .filter(|o| custom_kind(o) == Some("ModelTable"))
        .map(|o| tail_of(&normalize(&o.name)).to_string())
        .collect();
    let mut used: BTreeSet<Uuid> = BTreeSet::new();
    for r in opts.relationships {
        if STRUCTURAL.contains(&kind_label(&r.kind).as_str()) || r.from == r.to {
            continue;
        }
        used.insert(r.from.0);
        used.insert(r.to.0);
    }
    for o in objects {
        let relation = o.kind == ObjectKind::Table || custom_kind(o) == Some("View");
        // A live catalog's or the application model's view of a table is not its definition.
        if !relation
            || crate::procedure_lineage::is_live(o)
            || o.properties.contains_key("dbt_kind")
        {
            continue;
        }
        let normalized = normalize(&o.name);
        let tail = tail_of(&normalized);
        if used.contains(&o.id.0)
            || referenced.contains(tail)
            || modelled.contains(tail)
            || is_system_relation(&normalized)
        {
            continue;
        }
        let label = if o.kind == ObjectKind::Table {
            "Table"
        } else {
            "View"
        };
        let source = source_of(o, opts, declaring, "usage", "defines");
        // A table a migration script makes is often its own work table, dropped again later.
        let in_migration = source.file.as_deref().is_some_and(looks_like_migration);
        *out.orphans.entry(label.to_string()).or_insert(0) += 1;
        out.diagnostics.push(Diagnostic {
            kind: DiagnosticKind::OrphanArtifact,
            severity: Severity::Info,
            confidence: if in_migration { 0.3 } else { 0.5 },
            source,
            target_spec: TargetSpec {
                written: o.name.clone(),
                normalized: normalized.clone(),
                expected: "a reader or writer".to_string(),
                namespace: None,
            },
            candidates: Vec::<Candidate>::new(),
            evidence: o.evidence.clone(),
            coverage_gap: false,
            cross_system: false,
            reason: format!(
                "no compiled routine, view, code, transformation or foreign key uses this {}{}",
                label.to_lowercase(),
                if in_migration {
                    "; a migration script defines it"
                } else {
                    ""
                }
            ),
            fingerprint: fingerprint(
                DiagnosticKind::OrphanArtifact,
                &format!("{label}|{normalized}"),
            ),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xref;
    use ekos_kir::KirRelationship;

    fn m(name: &str) -> KirObject {
        KirObject::new(name, ObjectKind::Custom("PerlPackage".into()))
    }

    fn rel(kind: RelationshipKind, a: &KirObject, b: &KirObject) -> KirRelationship {
        KirRelationship::deterministic(kind, a.id, b.id, "t")
    }

    fn run(objects: &[KirObject], rels: &[KirRelationship]) -> xref::Report {
        xref::check(
            objects,
            &Options {
                relationships: rels,
                ..Default::default()
            },
        )
    }

    #[test]
    fn one_finding_per_cycle_whatever_its_length_and_order() {
        let (a, b, c, d) = (m("A"), m("B"), m("C"), m("D"));
        let mut rels = vec![
            rel(RelationshipKind::DependsOn, &a, &b),
            rel(RelationshipKind::DependsOn, &b, &c),
            rel(RelationshipKind::DependsOn, &c, &a),
            rel(RelationshipKind::DependsOn, &c, &d),
        ];
        let objects = [a, b, c, d];
        let r1 = run(&objects, &rels);
        let cycles: Vec<_> = r1
            .diagnostics
            .iter()
            .filter(|x| x.kind == DiagnosticKind::ReferenceCycle)
            .collect();
        assert_eq!(cycles.len(), 1);
        assert_eq!(cycles[0].target_spec.written, "A → B → C");
        assert_eq!(r1.metrics.cycles, 1);
        rels.reverse();
        let r2 = run(&objects, &rels);
        assert_eq!(
            r2.diagnostics
                .iter()
                .find(|x| x.kind == DiagnosticKind::ReferenceCycle)
                .unwrap()
                .fingerprint,
            cycles[0].fingerprint
        );
    }

    #[test]
    fn a_table_only_structural_edges_reach_is_an_orphan() {
        let file = KirObject::new("schema.sql", ObjectKind::File);
        let used = KirObject::new("orders", ObjectKind::Table);
        let lonely = KirObject::new("old_log", ObjectKind::Table);
        let routine = KirObject::new("post", ObjectKind::Custom("Procedure".into()));
        let rels = [
            rel(RelationshipKind::Contains, &file, &lonely),
            rel(RelationshipKind::DependsOn, &routine, &used),
        ];
        let named = KirObject::new("audit", ObjectKind::Table);
        let reader = KirObject::new("report", ObjectKind::Custom("View".into()))
            .with_property("reads", serde_json::json!(["public.audit"]));
        let modelled = KirObject::new("C_Order", ObjectKind::Table);
        let model = KirObject::new("C_Order", ObjectKind::Custom("ModelTable".into()));
        let r = run(
            &[file, used, lonely, routine, named, reader, modelled, model],
            &rels,
        );
        let orphans: BTreeSet<_> = r
            .diagnostics
            .iter()
            .filter(|x| x.kind == DiagnosticKind::OrphanArtifact)
            .map(|x| x.target_spec.written.as_str())
            .collect();
        // `report` itself is a view nothing reads: an orphan too.
        assert_eq!(orphans, ["old_log", "report"].into());
        assert_eq!(r.metrics.orphans["Table"], 1);
        assert_eq!(r.metrics.orphans["View"], 1);
    }

    #[test]
    fn a_long_chain_without_a_back_edge_is_no_cycle() {
        let nodes: Vec<KirObject> = (0..2000).map(|i| m(&format!("P{i}"))).collect();
        let rels: Vec<_> = nodes
            .windows(2)
            .map(|w| rel(RelationshipKind::DependsOn, &w[0], &w[1]))
            .collect();
        assert_eq!(run(&nodes, &rels).metrics.cycles, 0);
    }
}
