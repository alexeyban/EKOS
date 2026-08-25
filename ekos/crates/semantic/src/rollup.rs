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

/// `Some("project:<project>/dir:<first N path components>")` when `obj` carries both a
/// `"project"` property (RFC 0044 Phase 1 — multi-project estates) and a `"path"` property —
/// the project component keeps two different real projects from ever colliding even when they
/// happen to share an identical sub-path (e.g. both have `src/main.rs`), while the depth-limited
/// path component still gives real subdirectory-level grouping *within* one project, instead of
/// every file under one `[observe] paths` entry collapsing into a single group regardless of its
/// own real subdirectory structure.
///
/// `"project"` alone (no further path grouping) used to be the terminal group key — real bug
/// found live, 2026-08-24, against a real project (`pdf-reader`, `[observe] paths = ["backend",
/// "frontend", "README.md"]` — one real project split across multiple observe-path entries, not
/// a multi-project estate): every file under `backend/` got `project = "backend"`
/// (`build.rs`'s own `project_key_for_base`, non-empty whenever an entry's `base != cwd`), and
/// `group_key_for` returning `project:backend` unconditionally collapsed 17 real files across
/// `app/api/`, `app/services/`, `app/db/`, `app/core/` into one flat "backend" rollup — `##
/// System Decomposition`/`## Subsystems`/`## Component View` could never show real subsystem
/// granularity for any workspace with more than one `[observe] paths` entry, which is also
/// exactly the shape a genuine multi-project estate has. Combining both is a strict improvement
/// for that real use case too: a multi-project estate now gets real subdirectory rollups within
/// each project, not one blob per project.
///
/// Falls back to `Some("dir:<first N path components>")` when only `"path"` exists (no
/// `"project"` — the common single-`[observe]`-entry case, unaffected by this fix). `None` for
/// anything with neither — not every object is groupable, and that's fine; it simply isn't a
/// rollup member.
fn group_key_for(obj: &KirObject, depth: usize) -> Option<String> {
    let path = obj.properties.get("path").and_then(|v| v.as_str());
    if let Some(project) = obj.properties.get("project").and_then(|v| v.as_str()) {
        // `depth - 1`, not `depth`: `depth` (default 3) is calibrated for a *workspace-root*-
        // relative path, where it represents "3 real directory levels of typical project
        // structure before reaching file-level granularity" (`ekos/crates/kir/src/lib.rs` — 5
        // segments — stops at `ekos/crates/kir`, 2 segments short of the file). `path` here is
        // already *project*-relative (one level shallower to begin with — `project` itself
        // already identifies the top-level grouping boundary a root-relative path would still
        // need to discover), so the remaining budget for finding a real subsystem *within* it is
        // one less. Confirmed live, 2026-08-25: without this adjustment, a real 3-segment
        // project-relative path (`"app/api/ai.py"`) at `depth=3` grabbed the filename itself
        // (`take(3)` on 3 segments takes all of them), giving every file in `api/` its own
        // distinct group — zero rollups compiled at all, not the intended richer grouping.
        let sub_depth = depth.saturating_sub(1);
        return match path {
            Some(p) => {
                let prefix: Vec<&str> = p.split('/').take(sub_depth).collect();
                if prefix.is_empty() {
                    Some(format!("project:{project}"))
                } else {
                    Some(format!("project:{project}/dir:{}", prefix.join("/")))
                }
            }
            None => Some(format!("project:{project}")),
        };
    }
    let path = path?;
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

        // A combined key (`"project:backend/dir:app/api"`, the common case now that `project`
        // and `dir` grouping combine — see `group_key_for`) gets its embedded `"/dir:"` marker
        // collapsed into a plain `/`, producing a clean `"backend/app/api"` label instead of a
        // literal `"backend/dir:app/api"`.
        let label: String = if let Some(rest) = group_key.strip_prefix("project:") {
            rest.replacen("/dir:", "/", 1)
        } else if let Some(rest) = group_key.strip_prefix("dir:") {
            rest.to_string()
        } else {
            group_key.clone()
        };
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
    fn project_and_directory_grouping_combine_without_cross_project_collision() {
        // Real bug, found live 2026-08-24: `project` alone used to be the terminal group key, so
        // an entire `[observe] paths` entry (one real project, potentially with many real
        // subdirectories) always collapsed into one flat rollup — real subsystem structure inside
        // it was invisible. Fixed: `project` and depth-limited `path` now combine, so two
        // different real projects still never collide (even when they happen to share an
        // identical sub-path) *and* each project's own real subdirectory structure still produces
        // real, separate rollups.
        let mut graph = KirGraph::new();
        let mk = |project: &str, path: &str| {
            let mut f = file(path);
            f.properties
                .insert("project".to_string(), serde_json::json!(project));
            f
        };
        // Two real subdirectories inside proj-a, 2 files each.
        graph.add_object(mk("proj-a", "api/sub/a.rs"));
        graph.add_object(mk("proj-a", "api/sub/b.rs"));
        graph.add_object(mk("proj-a", "db/sub/c.rs"));
        graph.add_object(mk("proj-a", "db/sub/d.rs"));
        // proj-b has the exact same sub-path as one of proj-a's groups.
        graph.add_object(mk("proj-b", "api/sub/a.rs"));
        graph.add_object(mk("proj-b", "api/sub/b.rs"));

        // The real depth `commit.rs` actually calls this with — not an arbitrary smaller test
        // value. Using a different depth here previously let this exact bug (see the
        // `group_key_for` doc comment) pass its own test while still failing live: `depth=2`
        // (not `DEFAULT_DIRECTORY_DEPTH=3`) happened to sidestep the off-by-one this fix
        // addresses.
        synthesize_rollups(&mut graph, DEFAULT_DIRECTORY_DEPTH);

        let names: std::collections::HashSet<&str> = graph
            .objects
            .iter()
            .filter(|o| o.kind == ObjectKind::Custom("Rollup".to_string()))
            .map(|o| o.name.as_str())
            .collect();
        assert_eq!(
            names,
            std::collections::HashSet::from(["proj-a/api/sub", "proj-a/db/sub", "proj-b/api/sub"]),
            "each project's own real subdirectory structure must produce distinct rollups, and \
             identical sub-paths across different projects must never collide into one"
        );
    }

    #[test]
    fn project_relative_grouping_excludes_the_filename_at_the_real_default_depth() {
        // Direct regression for the real bug found live, 2026-08-25, against a real project
        // (`pdf-reader`, `[observe] paths = ["backend", "frontend", "README.md"]`): every real
        // `File.path` under the `backend` entry is project-relative and 3 segments deep
        // (`"app/api/ai.py"`, `"app/services/pdf_service.py"`, ...) — exactly
        // `DEFAULT_DIRECTORY_DEPTH` segments, so `take(depth)` without the `-1` adjustment grabbed
        // the filename itself, putting every file in its own singleton group — zero rollups
        // compiled at all, not the intended per-directory grouping.
        let mut graph = KirGraph::new();
        let mk = |path: &str| {
            let mut f = file(path);
            f.properties
                .insert("project".to_string(), serde_json::json!("backend"));
            f
        };
        graph.add_object(mk("app/api/ai.py"));
        graph.add_object(mk("app/api/library.py"));
        graph.add_object(mk("app/services/pdf_service.py"));
        graph.add_object(mk("app/services/ocr_service.py"));

        synthesize_rollups(&mut graph, DEFAULT_DIRECTORY_DEPTH);

        let names: std::collections::HashSet<&str> = graph
            .objects
            .iter()
            .filter(|o| o.kind == ObjectKind::Custom("Rollup".to_string()))
            .map(|o| o.name.as_str())
            .collect();
        assert_eq!(
            names,
            std::collections::HashSet::from(["backend/app/api", "backend/app/services"]),
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
