//! `PackageJsonAnalyzerPass` — real npm/yarn/pnpm dependency extraction from `package.json`
//! (RFC 0082, Phase 2 of the source-decomposition plan).
//!
//! Frontend `Technology`/`DependsOn` data previously came only from `dependency_analyzer.rs`'s
//! narrow substring-pattern table (a handful of literal package names like `pg`/`redis`) — it has
//! no generic JS/TS import-awareness at all. `package.json` is plain JSON — no new parser crate,
//! no new language grammar, just `serde_json` (already a workspace dependency) reading a real,
//! already-structured manifest, the same "read what's declared" spirit `crate_topology_analyzer.rs`
//! already has for `Cargo.toml`. Manifests are collected directly by `recover.rs` via `WalkDir`
//! (not through an `Observer`/`ArtifactStore` round-trip), the same second raw-content entry point
//! `crate_topology_analyzer.rs`/`cicd_analyzer.rs` already use.
//!
//! Deliberately does not introduce a JS-equivalent "Crate"/Container concept yet — that's a real
//! design decision left to Phase 3 (the System Decomposition view), which needs to settle what a
//! cross-language Container/layer model looks like before this pass commits to one. Instead,
//! `DependsOn` edges originate from the manifest `File` itself, the exact same "no container
//! concept yet" pattern `dependency_analyzer.rs` already established for its own Technology edges.

use async_trait::async_trait;
use ekos_compiler_core::pass::{CompilerPass, PassContext, PassError};
use ekos_kir::{
    KirEvidence, KirGraph, KirId, KirObject, KirRelationship, ObjectKind, RelationshipKind,
    SourceLocation,
};
use std::collections::HashMap;
use uuid::Uuid;

/// Matches `build.rs`'s own `File`-object id scheme exactly (RFC 0079 project-qualified from the
/// start, not retrofitted after a live bug the way `rust_analyzer.rs` needed one this session) so
/// a `DependsOn` edge lands on the same object `ekos_search`/`ekos_impact` already resolve.
fn file_kir_id(qualified_rel_path: &str) -> KirId {
    KirId(Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        qualified_rel_path.as_bytes(),
    ))
}

/// Same id scheme `dependency_analyzer.rs`'s and `crate_topology_analyzer.rs`'s own
/// `technology_kir_id` use — kept in sync deliberately, the same reasoning both of those already
/// state: a technology detected by more than one analyzer resolves to one real object, not one
/// per detector.
fn technology_kir_id(qualified_name: &str) -> KirId {
    KirId(Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("technology:{qualified_name}").as_bytes(),
    ))
}

/// Deterministic id for a `File`→`Technology` `DependsOn` edge — RFC 0072's own established
/// pattern applied from the start: a file declaring a dependency on a named package is a boolean
/// fact per `(file, package)` pair (unlike `sql_analyzer.rs`'s `ForeignKey`, there is no
/// legitimate multiplicity here — a `package.json` lists each dependency name at most once per
/// field), so this is exactly the shape RFC 0072 proved safe to key on a stable id.
fn depends_on_kir_id(from: KirId, to: KirId) -> KirId {
    KirId(Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("depends-on:{from}:{to}").as_bytes(),
    ))
}

pub struct PackageJsonAnalyzerPass {
    pass_id: String,
    /// (relative path to `package.json`, raw file content, RFC 0079 project qualifier).
    manifests: Vec<(String, String, Option<String>)>,
}

impl PackageJsonAnalyzerPass {
    pub fn new(
        workspace_name: impl Into<String>,
        manifests: Vec<(String, String, Option<String>)>,
    ) -> Self {
        Self {
            pass_id: format!("package-json-analyzer:{}", workspace_name.into()),
            manifests,
        }
    }
}

fn parse_manifests(manifests: &[(String, String, Option<String>)]) -> KirGraph {
    let mut graph = KirGraph::new();
    let mut technology_ids: HashMap<String, KirId> = HashMap::new();

    for (rel_path, content, project) in manifests {
        let doc: serde_json::Value = match content.parse() {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("cannot parse {rel_path} as JSON: {e}");
                continue;
            }
        };

        let id_path = ekos_common::project::project_qualify(rel_path, project.as_deref());
        let file_id = file_kir_id(&id_path);

        for field in ["dependencies", "devDependencies"] {
            let Some(deps) = doc.get(field).and_then(|v| v.as_object()) else {
                continue;
            };
            for (name, version) in deps {
                let version_str = version.as_str().unwrap_or("*");
                let qualified_name =
                    ekos_common::project::project_qualify(name, project.as_deref());
                let tech_id = *technology_ids
                    .entry(qualified_name.clone())
                    .or_insert_with(|| technology_kir_id(&qualified_name));
                if !graph.objects.iter().any(|o| o.id == tech_id) {
                    let mut tech_obj =
                        KirObject::new(name.clone(), ObjectKind::Custom("Technology".to_string()));
                    tech_obj.id = tech_id;
                    tech_obj
                        .properties
                        .insert("ecosystem".into(), serde_json::json!("npm"));
                    graph.add_object(tech_obj);
                }

                let ev = KirEvidence::new(
                    SourceLocation::file(rel_path.clone()),
                    format!("{field}: \"{name}\": \"{version_str}\""),
                );
                let ev_id = graph.add_evidence(ev);
                let mut rel = KirRelationship::new(RelationshipKind::DependsOn, file_id, tech_id);
                rel.id = depends_on_kir_id(file_id, tech_id);
                rel.properties
                    .insert("version_spec".into(), serde_json::json!(version_str));
                rel.properties.insert(
                    "dev_dependency".into(),
                    serde_json::json!(field == "devDependencies"),
                );
                rel.evidence.push(ev_id);
                graph.add_relationship(rel);
            }
        }
    }

    graph
}

#[async_trait]
impl CompilerPass for PackageJsonAnalyzerPass {
    fn name(&self) -> &str {
        &self.pass_id
    }

    fn cache_inputs(&self) -> Vec<String> {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        let mut sorted: Vec<&(String, String, Option<String>)> = self.manifests.iter().collect();
        sorted.sort_by(|a, b| a.0.cmp(&b.0));
        for (path, content, project) in sorted {
            hasher.update(path.as_bytes());
            hasher.update(content.as_bytes());
            hasher.update(project.as_deref().unwrap_or("").as_bytes());
        }
        vec![hex::encode(hasher.finalize())]
    }

    async fn run(&mut self, ctx: &mut PassContext) -> Result<(), PassError> {
        let graph = parse_manifests(&self.manifests);

        if graph.objects.is_empty() {
            return Ok(());
        }

        let knowledge = ekos_artifact::KnowledgeArtifact::new(&self.pass_id, vec![], graph);
        let json = serde_json::to_value(&knowledge)
            .map_err(|e| PassError::failed(format!("serialize KnowledgeArtifact: {e}")))?;
        ctx.artifact_store
            .write(&knowledge.id, &json)
            .map_err(|e| PassError::failed(format!("write artifact: {e}")))?;

        tracing::info!(
            pass = %self.pass_id,
            technologies = knowledge.content.kir.objects.iter().filter(|o| o.kind == ObjectKind::Custom("Technology".to_string())).count(),
            edges = knowledge.content.kir.relationships.len(),
            "package-json-analyzer complete"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_one(content: &str) -> KirGraph {
        parse_manifests(&[("package.json".to_string(), content.to_string(), None)])
    }

    #[test]
    fn extracts_real_dependencies_and_dev_dependencies() {
        let graph = run_one(
            r#"{
              "name": "dashboard",
              "version": "1.0.0",
              "dependencies": { "react": "^18.2.0" },
              "devDependencies": { "typescript": "^5.0.0" }
            }"#,
        );
        let names: Vec<&str> = graph.objects.iter().map(|o| o.name.as_str()).collect();
        assert!(names.contains(&"react"));
        assert!(names.contains(&"typescript"));

        let react = graph.objects.iter().find(|o| o.name == "react").unwrap();
        assert_eq!(react.kind, ObjectKind::Custom("Technology".to_string()));
        assert_eq!(react.properties["ecosystem"], "npm");

        let react_rel = graph
            .relationships
            .iter()
            .find(|r| r.to == react.id)
            .unwrap();
        assert_eq!(react_rel.kind, RelationshipKind::DependsOn);
        assert_eq!(react_rel.properties["dev_dependency"], false);

        let ts = graph
            .objects
            .iter()
            .find(|o| o.name == "typescript")
            .unwrap();
        let ts_rel = graph.relationships.iter().find(|r| r.to == ts.id).unwrap();
        assert_eq!(ts_rel.properties["dev_dependency"], true);
    }

    #[test]
    fn version_spec_is_captured_as_a_real_property() {
        let graph = run_one(r#"{"dependencies": {"react": "^18.2.0"}}"#);
        let rel = graph
            .relationships
            .iter()
            .find(|r| r.kind == RelationshipKind::DependsOn)
            .unwrap();
        assert_eq!(rel.properties["version_spec"], "^18.2.0");
    }

    #[test]
    fn same_dependency_across_two_manifests_dedupes_to_one_technology_object() {
        let graph = parse_manifests(&[
            (
                "assets/package.json".to_string(),
                r#"{"dependencies": {"react": "^18.2.0"}}"#.to_string(),
                None,
            ),
            (
                "tracker/package.json".to_string(),
                r#"{"dependencies": {"react": "^18.2.0"}}"#.to_string(),
                None,
            ),
        ]);
        let react_objects: Vec<_> = graph.objects.iter().filter(|o| o.name == "react").collect();
        assert_eq!(react_objects.len(), 1);
        assert_eq!(
            graph
                .relationships
                .iter()
                .filter(|r| r.to == react_objects[0].id)
                .count(),
            2,
            "two real, distinct files each declaring the dependency"
        );
    }

    #[test]
    fn depends_on_ids_are_deterministic_across_two_independent_runs() {
        let content = r#"{"dependencies": {"react": "^18.2.0"}}"#;
        let g1 = run_one(content);
        let g2 = run_one(content);
        assert_eq!(g1.relationships[0].id, g2.relationships[0].id);
    }

    #[test]
    fn malformed_json_is_skipped_not_fatal() {
        let graph = run_one("{ not valid json");
        assert!(graph.objects.is_empty());
        assert!(graph.relationships.is_empty());
    }

    #[test]
    fn a_manifest_with_no_dependency_fields_produces_nothing() {
        let graph = run_one(r#"{"name": "dashboard", "version": "1.0.0"}"#);
        assert!(graph.objects.is_empty());
    }

    #[test]
    fn a_project_field_qualifies_the_file_id_and_technology_id() {
        let ga = parse_manifests(&[(
            "package.json".to_string(),
            r#"{"dependencies": {"react": "^18.2.0"}}"#.to_string(),
            Some("frontend-a".to_string()),
        )]);
        let gb = parse_manifests(&[(
            "package.json".to_string(),
            r#"{"dependencies": {"react": "^18.2.0"}}"#.to_string(),
            Some("frontend-b".to_string()),
        )]);
        let react_a = ga.objects.iter().find(|o| o.name == "react").unwrap();
        let react_b = gb.objects.iter().find(|o| o.name == "react").unwrap();
        assert_eq!(react_a.name, "react");
        assert_eq!(react_b.name, "react");
        assert_ne!(
            react_a.id, react_b.id,
            "the same package name in two different projects must not collide"
        );
    }
}
