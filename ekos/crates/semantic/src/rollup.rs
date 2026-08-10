//! Hierarchical knowledge rollups (RFC 0044) — synthesizes one higher-level `Rollup` object per
//! group of related objects (a directory subtree within one project, or a whole project within a
//! multi-project estate), so an agent can ask about a whole subsystem and get one condensed,
//! evidence-linked object instead of personally synthesizing meaning from dozens of raw facts
//! within its own context window.
//!
//! Deterministic, zero-LLM by default — pure structural aggregation over the already-resolved
//! `KirGraph`, the same "compile, don't guess" rule every other pass in this workspace follows.
//! Runs inside `SemanticCompilerPass`, after identity resolution/merge and before the CKM is
//! built, so a `Rollup` is just an ordinary `KirObject` linked to its members by the ordinary
//! `Contains` relationship by the time `ekos_search`/`ekos_neighborhood`/EKL ever see the graph —
//! no new query surface needed for v1.

use std::collections::HashMap;

use ekos_kir::{
    KirEvidence, KirGraph, KirId, KirObject, KirRelationship, ObjectKind, RelationshipKind,
    SourceLocation,
};
use uuid::Uuid;

/// Leading path components that make up one directory-rollup group for an object with no
/// `"project"` property to group by instead. 3 is a reasonable default for a Cargo workspace
/// (`ekos/crates/kir/src/lib.rs` groups under `ekos/crates/kir` — crate-level, not
/// `ekos/crates` — too coarse — or `ekos` — far too coarse); genuinely project-structure-
/// dependent, so callers may override it.
pub const DEFAULT_DIRECTORY_DEPTH: usize = 3;

fn rollup_kir_id(group_key: &str) -> KirId {
    KirId(Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("rollup:{group_key}").as_bytes(),
    ))
}

/// `Some("project:<value>")` when `obj` carries a `"project"` property (RFC 0044 Phase 1 —
/// multi-project estates), taking priority over directory grouping since it's the coarser,
/// more meaningful boundary in that context. Otherwise `Some("dir:<first N path components>")`
/// when `obj` carries a `"path"` property (every `File` object does). `None` for anything with
/// neither — not every object is groupable, and that's fine; it simply isn't a rollup member.
fn group_key_for(obj: &KirObject, depth: usize) -> Option<String> {
    if let Some(project) = obj.properties.get("project").and_then(|v| v.as_str()) {
        return Some(format!("project:{project}"));
    }
    let path = obj.properties.get("path").and_then(|v| v.as_str())?;
    let prefix: Vec<&str> = path.split('/').take(depth).collect();
    if prefix.is_empty() {
        return None;
    }
    Some(format!("dir:{}", prefix.join("/")))
}

/// Adds one `Rollup` `KirObject` per group to `graph`, in place. Only `File` objects are
/// evaluated as direct rollup members — every other kind (`RustSymbol`, `PythonSymbol`, …) is
/// reachable transitively through the `File` → symbol `Contains` edges recovery passes already
/// emit, so it isn't double-counted as a *direct* member here; a rollup's `components` property
/// still reflects the file-level shape of its group (how many files, from which kinds of
/// artifacts), which is the right altitude for a "what lives in this subsystem" answer.
///
/// A group of size 1 (the whole graph collapsing into a single directory/project) produces no
/// rollup — nothing would distinguish it from the graph itself, so it isn't a useful summary.
pub fn synthesize_rollups(graph: &mut KirGraph, depth: usize) {
    let mut groups: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, obj) in graph.objects.iter().enumerate() {
        if obj.kind != ObjectKind::File {
            continue;
        }
        if let Some(key) = group_key_for(obj, depth) {
            groups.entry(key).or_default().push(i);
        }
    }

    if groups.len() < 2 {
        return;
    }

    let mut new_objects = Vec::new();
    let mut new_relationships = Vec::new();
    let mut new_evidence = Vec::new();

    let mut sorted_keys: Vec<&String> = groups.keys().collect();
    sorted_keys.sort();

    for group_key in sorted_keys {
        let indices = &groups[group_key];
        // A group of one file is just that file — not worth a summary object distinct from it.
        if indices.len() < 2 {
            continue;
        }
        let rollup_id = rollup_kir_id(group_key);
        let group_member_ids: std::collections::HashSet<KirId> =
            indices.iter().map(|&i| graph.objects[i].id).collect();

        let mut kind_counts: HashMap<String, usize> = HashMap::new();
        let mut boundary_counts: HashMap<String, usize> = HashMap::new();

        for &i in indices {
            let member = &graph.objects[i];
            *kind_counts.entry(member.kind.to_string()).or_default() += 1;

            for rel in &graph.relationships {
                let other = if rel.from == member.id {
                    Some(rel.to)
                } else if rel.to == member.id {
                    Some(rel.from)
                } else {
                    None
                };
                if let Some(other_id) = other
                    && !group_member_ids.contains(&other_id)
                {
                    *boundary_counts.entry(rel.kind.to_string()).or_default() += 1;
                }
            }

            let ev = KirEvidence::new(
                SourceLocation::file(group_key.clone()),
                format!("{} is a member of {group_key}", member.name),
            );
            let ev_id = ev.id;
            new_evidence.push(ev);

            let mut contains =
                KirRelationship::new(RelationshipKind::Contains, rollup_id, member.id);
            contains.evidence.push(ev_id);
            new_relationships.push(contains);
        }

        let label = group_key
            .strip_prefix("project:")
            .or_else(|| group_key.strip_prefix("dir:"))
            .unwrap_or(group_key);
        let mut rollup = KirObject::new(label, ObjectKind::Custom("Rollup".to_string()))
            .with_property("group_key", serde_json::json!(group_key))
            .with_property("member_count", serde_json::json!(indices.len()))
            .with_property("components", serde_json::to_value(&kind_counts).unwrap())
            .with_property(
                "boundary_relationships",
                serde_json::to_value(&boundary_counts).unwrap(),
            );
        rollup.id = rollup_id;
        new_objects.push(rollup);
    }

    graph.objects.extend(new_objects);
    graph.relationships.extend(new_relationships);
    graph.evidence.extend(new_evidence);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_kir::RelationshipKind;

    fn file(path: &str) -> KirObject {
        KirObject::new(path, ObjectKind::File).with_property("path", serde_json::json!(path))
    }

    #[test]
    fn groups_files_by_directory_prefix_at_given_depth() {
        let mut graph = KirGraph::new();
        graph.add_object(file("ekos/crates/kir/src/lib.rs"));
        graph.add_object(file("ekos/crates/kir/src/config.rs"));
        graph.add_object(file("ekos/crates/recovery/src/lib.rs"));
        graph.add_object(file("ekos/crates/recovery/src/other.rs"));

        synthesize_rollups(&mut graph, 3);

        let rollups: Vec<&KirObject> = graph
            .objects
            .iter()
            .filter(|o| o.kind == ObjectKind::Custom("Rollup".to_string()))
            .collect();
        assert_eq!(rollups.len(), 2, "expected one rollup per crate directory");

        let kir_rollup = rollups
            .iter()
            .find(|r| r.name == "ekos/crates/kir")
            .expect("kir crate rollup");
        assert_eq!(kir_rollup.properties["member_count"], serde_json::json!(2));
    }

    #[test]
    fn single_group_produces_no_rollup() {
        let mut graph = KirGraph::new();
        graph.add_object(file("ekos/crates/kir/src/lib.rs"));
        graph.add_object(file("ekos/crates/kir/src/config.rs"));

        synthesize_rollups(&mut graph, 3);

        assert!(
            graph
                .objects
                .iter()
                .all(|o| o.kind != ObjectKind::Custom("Rollup".to_string())),
            "a single group covering everything isn't a useful rollup"
        );
    }

    #[test]
    fn project_property_takes_priority_over_directory_grouping() {
        let mut graph = KirGraph::new();
        let mut a1 = file("src/main.rs");
        a1.properties
            .insert("project".to_string(), serde_json::json!("proj-a"));
        let mut a2 = file("src/lib.rs");
        a2.properties
            .insert("project".to_string(), serde_json::json!("proj-a"));
        let mut b1 = file("src/main.rs");
        b1.properties
            .insert("project".to_string(), serde_json::json!("proj-b"));
        let mut b2 = file("src/lib.rs");
        b2.properties
            .insert("project".to_string(), serde_json::json!("proj-b"));
        graph.add_object(a1);
        graph.add_object(a2);
        graph.add_object(b1);
        graph.add_object(b2);

        synthesize_rollups(&mut graph, 3);

        let names: std::collections::HashSet<&str> = graph
            .objects
            .iter()
            .filter(|o| o.kind == ObjectKind::Custom("Rollup".to_string()))
            .map(|o| o.name.as_str())
            .collect();
        assert_eq!(
            names,
            std::collections::HashSet::from(["proj-a", "proj-b"]),
            "objects with a `project` property must group by project, not by their shared path"
        );
    }

    #[test]
    fn rollup_links_to_every_member_via_contains() {
        let mut graph = KirGraph::new();
        let f1 = file("ekos/crates/kir/src/lib.rs");
        let f1_id = f1.id;
        let f2 = file("ekos/crates/recovery/src/lib.rs");
        let f2_id = f2.id;
        graph.add_object(f1);
        graph.add_object(f2);
        graph.add_object(file("ekos/crates/kir/src/config.rs"));

        synthesize_rollups(&mut graph, 3);

        let kir_rollup = graph
            .objects
            .iter()
            .find(|o| o.name == "ekos/crates/kir")
            .unwrap();
        assert!(
            graph
                .relationships
                .iter()
                .any(|r| r.kind == RelationshipKind::Contains
                    && r.from == kir_rollup.id
                    && r.to == f1_id)
        );
        // f2 belongs to a different (single-member, so no-rollup) group and must not be linked.
        assert!(
            !graph
                .relationships
                .iter()
                .any(|r| r.kind == RelationshipKind::Contains && r.to == f2_id)
        );
    }

    #[test]
    fn boundary_relationships_count_edges_crossing_the_group() {
        let mut graph = KirGraph::new();
        let kir_file = file("ekos/crates/kir/src/lib.rs");
        let kir_id = kir_file.id;
        let kir_file2 = file("ekos/crates/kir/src/config.rs");
        let recovery_file = file("ekos/crates/recovery/src/lib.rs");
        let recovery_id = recovery_file.id;
        let recovery_file2 = file("ekos/crates/recovery/src/other.rs");
        graph.add_object(kir_file);
        graph.add_object(kir_file2);
        graph.add_object(recovery_file);
        graph.add_object(recovery_file2);
        graph.add_relationship(KirRelationship::new(
            RelationshipKind::DependsOn,
            recovery_id,
            kir_id,
        ));

        synthesize_rollups(&mut graph, 3);

        let recovery_rollup = graph
            .objects
            .iter()
            .find(|o| o.name == "ekos/crates/recovery")
            .unwrap();
        let boundary = &recovery_rollup.properties["boundary_relationships"];
        assert_eq!(boundary["DependsOn"], serde_json::json!(1));
    }
}
