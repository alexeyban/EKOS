//! RFC 0163 — links recovered routines to the tables they read and write and the routines they
//! call.
//!
//! `PlPgSqlAnalyzerPass` records, per `ProcedureStatement` and per `Procedure`, the names its SQL
//! reads, writes and calls — from a real parse, but as names: a per-file pass cannot see another
//! file's tables. This runs over the whole committed graph, the way RFC 0075's
//! [`crate::data_lineage`] links `TransformNode`s, and turns names into edges:
//!
//! | Edge | From → to | Why this kind |
//! |---|---|---|
//! | `ReadsFrom` / `WritesTo` | `ProcedureStatement` → `Table`/`Dataset` | the precise citation: this statement, this line |
//! | `DependsOn` | `Procedure` → `Table`/`Dataset` | what `ekos_dependents`/`ekos_impact` traverse, so "what breaks if I change this table" finds the routine |
//! | `Calls` | `Procedure` → `Procedure` | the call graph `callers` traverses |
//!
//! RFC 0169: a `View` is both a target (routines and views read it) and a source (it `DependsOn`
//! what its query reads and `Calls` the routines its query calls).
//!
//! **A name links only when it names exactly one object.** Two `customers` tables in two schemas,
//! or one routine overloaded three ways, is not something a bare name can choose between, and a
//! guessed edge is a fabricated fact (the RFC 0060/0075 judgment). An unqualified name may match a
//! schema-qualified object and vice versa — still only when the match is unique — and the edge
//! records which (`match: "exact" | "unqualified"`). Built-in functions (`coalesce`, `now`) match no
//! routine and produce nothing.

use ekos_kir::{KirGraph, KirId, KirObject, KirRelationship, ObjectKind, RelationshipKind};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use uuid::Uuid;

/// What one linking run did, for `ekos commit`'s summary.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProcedureLinkStats {
    pub reads_from: usize,
    pub writes_to: usize,
    pub depends_on: usize,
    pub calls: usize,
    /// Distinct names that matched more than one object and were therefore not linked.
    pub ambiguous: usize,
}

impl ProcedureLinkStats {
    pub fn total(&self) -> usize {
        self.reads_from + self.writes_to + self.depends_on + self.calls
    }
}

/// Names → object ids, exact (lower-cased full name) and by last dotted segment.
#[derive(Default)]
struct NameIndex {
    // Keyed by the inner `Uuid` (`KirId` is not `Ord`), so iteration — and edge order — is stable.
    exact: HashMap<String, BTreeSet<Uuid>>,
    tail: HashMap<String, BTreeSet<Uuid>>,
}

impl NameIndex {
    fn add(&mut self, name: &str, id: KirId) {
        let n = name.to_lowercase();
        let tail = n.rsplit('.').next().unwrap_or(&n).to_string();
        self.exact.entry(n).or_default().insert(id.0);
        self.tail.entry(tail).or_default().insert(id.0);
    }

    /// The single object `name` refers to, and how it matched — or `Err(true)` when several
    /// objects match (ambiguous), `Err(false)` when none does.
    fn resolve(&self, name: &str) -> Result<(KirId, &'static str), bool> {
        let n = name.to_lowercase();
        let unique = |s: Option<&BTreeSet<Uuid>>| match s {
            Some(ids) if ids.len() == 1 => Ok(KirId(*ids.iter().next().unwrap())),
            Some(ids) if ids.len() > 1 => Err(true),
            _ => Err(false),
        };
        match unique(self.exact.get(&n)) {
            Ok(id) => return Ok((id, "exact")),
            Err(true) => return Err(true),
            Err(false) => {}
        }
        // `public.t` written, `t` compiled — or `t` written, `public.t` compiled.
        let tail = n.rsplit('.').next().unwrap_or(&n);
        let fallback = if n.contains('.') {
            self.exact.get(tail)
        } else {
            self.tail.get(tail)
        };
        unique(fallback).map(|id| (id, "unqualified"))
    }
}

fn strings(o: &KirObject, key: &str) -> Vec<String> {
    o.properties
        .get(key)
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn is_custom(o: &KirObject, kind: &str) -> bool {
    matches!(&o.kind, ObjectKind::Custom(k) if k == kind)
}

fn edge(
    kind: RelationshipKind,
    from: &KirObject,
    to: KirId,
    how: &str,
    extra: &[(&str, serde_json::Value)],
) -> KirRelationship {
    // Deterministic: re-linking unchanged input appends nothing (RFC 0135 Part C).
    let mut rel = KirRelationship::deterministic(kind, from.id, to, "procedure-lineage");
    rel.properties.insert("match".into(), json!(how));
    for (k, v) in extra {
        rel.properties.insert((*k).into(), v.clone());
    }
    // The statement or routine whose parsed SQL named the target is the evidence for the edge.
    rel.evidence = from.evidence.clone();
    rel
}

/// Append routine → table and routine → routine edges to `graph`.
pub fn link_procedures(graph: &mut KirGraph) -> ProcedureLinkStats {
    let mut tables = NameIndex::default();
    let mut routines = NameIndex::default();
    for o in &graph.objects {
        // RFC 0169: views are relations a routine reads, so they resolve alongside tables.
        if matches!(o.kind, ObjectKind::Table | ObjectKind::Dataset) || is_custom(o, "View") {
            tables.add(&o.name, o.id);
        } else if is_custom(o, "Procedure") {
            routines.add(&o.name, o.id);
        }
    }

    let mut ambiguous: BTreeSet<String> = BTreeSet::new();
    // One edge per id — the id is a function of (kind, from, to). Two spellings of one target
    // (`defaults` and `[% slschema %].defaults`) would otherwise emit the same id twice with
    // different `match` values, and the ledger would flip between them on every commit.
    let mut new: BTreeMap<Uuid, KirRelationship> = BTreeMap::new();
    let mut put = |rel: KirRelationship| match new.get(&rel.id.0) {
        Some(existing) if existing.properties["match"] == json!("exact") => {}
        _ => {
            new.insert(rel.id.0, rel);
        }
    };
    for o in &graph.objects {
        if is_custom(o, "ProcedureStatement") {
            for (key, kind) in [("reads", "ReadsFrom"), ("writes", "WritesTo")] {
                for name in strings(o, key) {
                    match tables.resolve(&name) {
                        Ok((t, how)) => {
                            put(edge(RelationshipKind::Custom(kind.into()), o, t, how, &[]))
                        }
                        Err(true) => {
                            ambiguous.insert(name);
                        }
                        Err(false) => {}
                    }
                }
            }
        } else if is_custom(o, "Procedure") || is_custom(o, "View") {
            // A view links from its own query's footprint exactly as a routine does — reads only.
            // read / write / read_write per table, and the strongest way any spelling matched it.
            let mut access: BTreeMap<Uuid, (bool, bool, &'static str)> = BTreeMap::new();
            for (key, write) in [("reads", false), ("writes", true)] {
                for name in strings(o, key) {
                    match tables.resolve(&name) {
                        Ok((t, how)) => {
                            let e = access.entry(t.0).or_insert((false, false, how));
                            if how == "exact" {
                                e.2 = how;
                            }
                            if write {
                                e.1 = true;
                            } else {
                                e.0 = true;
                            }
                        }
                        Err(true) => {
                            ambiguous.insert(name);
                        }
                        Err(false) => {}
                    }
                }
            }
            for (t, (r, w, how)) in access {
                let mode = match (r, w) {
                    (true, true) => "read_write",
                    (false, true) => "write",
                    _ => "read",
                };
                put(edge(
                    RelationshipKind::DependsOn,
                    o,
                    KirId(t),
                    how,
                    &[("access", json!(mode))],
                ));
            }
            for name in strings(o, "calls") {
                match routines.resolve(&name) {
                    Ok((callee, how)) => put(edge(RelationshipKind::Calls, o, callee, how, &[])),
                    Err(true) => {
                        ambiguous.insert(name);
                    }
                    Err(false) => {}
                }
            }
        }
    }

    let mut stats = ProcedureLinkStats {
        ambiguous: ambiguous.len(),
        ..Default::default()
    };
    for rel in new.values() {
        match &rel.kind {
            RelationshipKind::DependsOn => stats.depends_on += 1,
            RelationshipKind::Calls => stats.calls += 1,
            RelationshipKind::Custom(k) if k == "ReadsFrom" => stats.reads_from += 1,
            _ => stats.writes_to += 1,
        }
    }
    graph.relationships.extend(new.into_values());
    stats
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_kir::KirEvidence;
    use ekos_kir::SourceLocation;

    fn obj(g: &mut KirGraph, name: &str, kind: ObjectKind, props: &[(&str, &[&str])]) -> KirId {
        let mut o = KirObject::new(name, kind);
        for (k, v) in props {
            o.properties.insert((*k).into(), json!(v));
        }
        let ev = g.add_evidence(KirEvidence::new(SourceLocation::file("f.sql"), name));
        o.evidence.push(ev);
        g.add_object(o)
    }

    fn proc_kind() -> ObjectKind {
        ObjectKind::Custom("Procedure".into())
    }
    fn stmt_kind() -> ObjectKind {
        ObjectKind::Custom("ProcedureStatement".into())
    }

    fn edges(g: &KirGraph, from: KirId) -> Vec<(String, KirId)> {
        let mut v: Vec<(String, KirId)> = g
            .relationships
            .iter()
            .filter(|r| r.from == from)
            .map(|r| (r.kind.to_string(), r.to))
            .collect();
        v.sort_by_key(|(k, id)| (k.clone(), id.0));
        v
    }

    #[test]
    fn statements_and_routines_link_to_the_tables_and_routines_they_name() {
        let mut g = KirGraph::new();
        let acc = obj(&mut g, "acc_trans", ObjectKind::Table, &[]);
        let inv = obj(&mut g, "invoice", ObjectKind::Table, &[]);
        let callee = obj(&mut g, "setting_get", proc_kind(), &[]);
        let caller = obj(
            &mut g,
            "post",
            proc_kind(),
            &[
                ("reads", &["acc_trans", "invoice"]),
                ("writes", &["invoice"]),
                ("calls", &["setting_get", "coalesce"]),
            ],
        );
        let stmt = obj(
            &mut g,
            "post#0",
            stmt_kind(),
            &[("reads", &["public.acc_trans"]), ("writes", &["invoice"])],
        );

        let stats = link_procedures(&mut g);
        assert_eq!(
            stats,
            ProcedureLinkStats {
                reads_from: 1,
                writes_to: 1,
                depends_on: 2,
                calls: 1,
                ambiguous: 0,
            }
        );
        let mut want = vec![
            ("Calls".to_string(), callee),
            ("DependsOn".to_string(), acc),
            ("DependsOn".to_string(), inv),
        ];
        want.sort_by_key(|(k, id)| (k.clone(), id.0));
        assert_eq!(edges(&g, caller), want);
        let mut want = vec![
            ("ReadsFrom".to_string(), acc),
            ("WritesTo".to_string(), inv),
        ];
        want.sort_by_key(|(k, id)| (k.clone(), id.0));
        assert_eq!(edges(&g, stmt), want);

        let dep = |t: KirId| {
            g.relationships
                .iter()
                .find(|r| r.from == caller && r.to == t)
                .unwrap()
        };
        assert_eq!(dep(inv).properties["access"], json!("read_write"));
        assert_eq!(dep(acc).properties["access"], json!("read"));
        // `public.acc_trans` matched the unqualified table, and says so.
        let read = g
            .relationships
            .iter()
            .find(|r| r.from == stmt && r.to == acc)
            .unwrap();
        assert_eq!(read.properties["match"], json!("unqualified"));
        // Every edge cites the evidence of the object whose SQL named the target.
        assert!(g.relationships.iter().all(|r| !r.evidence.is_empty()));
    }

    /// Two spellings of one target (`defaults` and `[% slschema %].defaults` in LedgerSMB) are one
    /// edge. Emitting both — same id, `match: exact` vs `unqualified` — made the ledger flip between
    /// two versions of that edge on every commit of unchanged input.
    #[test]
    fn two_spellings_of_one_target_are_one_edge_with_the_strongest_match() {
        let mut g = KirGraph::new();
        let t = obj(&mut g, "defaults", ObjectKind::Table, &[]);
        let f = obj(&mut g, "f", proc_kind(), &[]);
        let s = obj(
            &mut g,
            "p#0",
            stmt_kind(),
            &[("reads", &["[% slschema %].defaults", "defaults"])],
        );
        let p = obj(&mut g, "p", proc_kind(), &[("calls", &["public.f", "f"])]);
        let stats = link_procedures(&mut g);
        assert_eq!(edges(&g, s), vec![("ReadsFrom".to_string(), t)]);
        assert_eq!(edges(&g, p), vec![("Calls".to_string(), f)]);
        assert_eq!((stats.reads_from, stats.calls), (1, 1));
        for r in &g.relationships {
            assert_eq!(r.properties["match"], json!("exact"));
        }
        // And no id appears twice.
        let ids: std::collections::HashSet<_> = g.relationships.iter().map(|r| r.id.0).collect();
        assert_eq!(ids.len(), g.relationships.len());
    }

    /// RFC 0169: a view is a relation routines can read, and is linked to what its own query reads
    /// and calls — so impact on a table reaches the views over it, and the routines over those.
    #[test]
    fn views_are_link_targets_and_link_to_their_own_dependencies() {
        let mut g = KirGraph::new();
        let view_kind = || ObjectKind::Custom("View".into());
        let acc = obj(&mut g, "acc_trans", ObjectKind::Table, &[]);
        let in_tree = obj(&mut g, "in_tree", proc_kind(), &[]);
        let base = obj(
            &mut g,
            "account_heading_tree",
            view_kind(),
            &[("reads", &["acc_trans"]), ("calls", &["in_tree"])],
        );
        let over = obj(
            &mut g,
            "account_heading_descendant",
            view_kind(),
            &[("reads", &["account_heading_tree"])],
        );
        let routine = obj(
            &mut g,
            "report",
            proc_kind(),
            &[("reads", &["account_heading_descendant"])],
        );
        let stmt = obj(
            &mut g,
            "report#0",
            stmt_kind(),
            &[("reads", &["account_heading_descendant"])],
        );
        // Defined in two files: two views, one bare name — not linked.
        obj(&mut g, "cash_impact", view_kind(), &[]);
        obj(&mut g, "cash_impact", view_kind(), &[]);
        let ambiguous_reader = obj(&mut g, "r2", proc_kind(), &[("reads", &["cash_impact"])]);

        link_procedures(&mut g);
        assert_eq!(edges(&g, base), {
            let mut w = vec![
                ("Calls".to_string(), in_tree),
                ("DependsOn".to_string(), acc),
            ];
            w.sort_by_key(|(k, id)| (k.clone(), id.0));
            w
        });
        assert_eq!(edges(&g, over), vec![("DependsOn".to_string(), base)]);
        assert_eq!(edges(&g, routine), vec![("DependsOn".to_string(), over)]);
        assert_eq!(edges(&g, stmt), vec![("ReadsFrom".to_string(), over)]);
        assert!(edges(&g, ambiguous_reader).is_empty());
        let dep = g
            .relationships
            .iter()
            .find(|r| r.from == base && r.to == acc)
            .unwrap();
        assert_eq!(dep.properties["access"], json!("read"));
    }

    /// A name that matches two objects is not guessed at.
    #[test]
    fn an_ambiguous_name_links_nothing() {
        let mut g = KirGraph::new();
        obj(&mut g, "sales.customers", ObjectKind::Table, &[]);
        obj(&mut g, "crm.customers", ObjectKind::Table, &[]);
        obj(&mut g, "f", proc_kind(), &[]);
        obj(&mut g, "f", proc_kind(), &[]);
        let p = obj(
            &mut g,
            "p",
            proc_kind(),
            &[("reads", &["customers"]), ("calls", &["f"])],
        );
        let stats = link_procedures(&mut g);
        assert!(edges(&g, p).is_empty());
        assert_eq!(stats.ambiguous, 2);
        assert_eq!(stats.total(), 0);
    }

    /// Same graph, same edges, same ids: re-linking unchanged input appends nothing new.
    #[test]
    fn linking_is_deterministic() {
        let build = || {
            let mut g = KirGraph::new();
            let t = obj(&mut g, "t", ObjectKind::Table, &[]);
            let mut p = KirObject::new("p", proc_kind());
            p.id = KirId(uuid::Uuid::nil());
            p.properties.insert("reads".into(), json!(["t"]));
            g.add_object(p);
            (g, t)
        };
        let (mut a, _) = build();
        let (mut b, _) = build();
        link_procedures(&mut a);
        link_procedures(&mut b);
        // Table ids are random per build, so compare by shape: one edge each, ids differ only
        // through the table id they point at.
        assert_eq!(a.relationships.len(), 1);
        assert_eq!(b.relationships.len(), 1);
        let mut c = a.clone();
        c.relationships.clear();
        link_procedures(&mut c);
        assert_eq!(c.relationships[0].id, a.relationships[0].id);
    }
}
