//! `RequirementsAnalyzerPass` — real pip dependency extraction from `requirements.txt`.
//!
//! Real gap found running the full pipeline against a real external project (`pdf-reader`: FastAPI
//! Python backend + React/TypeScript frontend): `package_json_analyzer.rs` (npm) and
//! `dependency_analyzer.rs`/`crate_topology_analyzer.rs` (Cargo) both give their ecosystem a real
//! `Custom("Technology")`/`DependsOn` view, but Python's own `requirements.txt` had no analyzer at
//! all — every generated Technology Inventory / System Context view was blind to all of a real
//! FastAPI backend's declared dependencies (`fastapi`, `sqlalchemy`, `pymupdf`, ...) even though the
//! Python *source* itself was fully analyzed by `python_analyzer.rs`.
//!
//! `requirements.txt`'s `pkg==1.2.3` line format is plain text, simpler than `package.json`'s JSON
//! — this mirrors `package_json_analyzer.rs`'s exact shape (same `Custom("Technology")` id scheme,
//! same `File`→`Technology` `DependsOn` edge, same RFC 0079 project qualification, same manifest
//! collection pattern in `recover.rs`) rather than inventing a new one. `pyproject.toml` (PEP 621 /
//! Poetry / Flit each shape dependencies differently) is a deliberately separate, not-yet-attempted
//! follow-on — this pass covers only the concretely-verified `requirements.txt` gap.

use async_trait::async_trait;
use ekos_compiler_core::pass::{CompilerPass, PassContext, PassError};
use ekos_kir::{
    KirEvidence, KirGraph, KirId, KirObject, KirRelationship, ObjectKind, RelationshipKind,
    SourceLocation,
};
use std::collections::HashMap;
use uuid::Uuid;

/// Matches `build.rs`'s own `File`-object id scheme — see `package_json_analyzer.rs::file_kir_id`
/// (kept in sync deliberately, same reasoning).
fn file_kir_id(qualified_rel_path: &str) -> KirId {
    KirId(Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        qualified_rel_path.as_bytes(),
    ))
}

/// Same id scheme `dependency_analyzer.rs`/`crate_topology_analyzer.rs`/`package_json_analyzer.rs`
/// all use — a technology detected by more than one analyzer resolves to one real object.
fn technology_kir_id(qualified_name: &str) -> KirId {
    KirId(Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("technology:{qualified_name}").as_bytes(),
    ))
}

/// Same `depends_on_kir_id` pattern `package_json_analyzer.rs` uses (RFC 0072): a file declaring a
/// dependency on a named package is a boolean fact per `(file, package)` pair.
fn depends_on_kir_id(from: KirId, to: KirId) -> KirId {
    KirId(Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("depends-on:{from}:{to}").as_bytes(),
    ))
}

/// One parsed `requirements.txt` line: the bare package name and the raw version-spec text (empty
/// when the line names a package with no constraint at all, e.g. plain `requests`).
struct ParsedRequirement {
    name: String,
    version_spec: String,
}

/// Parses one `requirements.txt` line per [PEP 508](https://peps.python.org/pep-0508/)'s common
/// subset — deliberately not a full PEP 508 grammar (environment markers, direct URL references),
/// matching this pass's own "read what's declared" scope, same as every sibling analyzer.
/// Returns `None` for anything that isn't a plain `name<spec>` requirement: blank lines, `#`
/// comments, option lines (`-r other.txt`, `-e .`, `--index-url ...`), and VCS/URL requirements
/// (`git+https://...`, `./local-package`) — none of these name an installable package version by
/// itself, and guessing would risk fabricating a `Technology` that was never really declared.
fn parse_requirement_line(line: &str) -> Option<ParsedRequirement> {
    let line = line.split('#').next().unwrap_or("").trim();
    if line.is_empty() || line.starts_with('-') {
        return None;
    }
    if line.contains("://") {
        return None;
    }
    // Stop the name at the first character that isn't part of a valid package name (PEP 508
    // allows letters, digits, `.`, `-`, `_`), an extras marker `[...]`, or a version comparator.
    let name_end = line
        .find(|c: char| !(c.is_alphanumeric() || matches!(c, '.' | '-' | '_')))
        .unwrap_or(line.len());
    let name = line[..name_end].trim();
    if name.is_empty() {
        return None;
    }
    // Environment markers (`; python_version >= "3.8"`) are metadata about *when* the dependency
    // applies, not part of the version spec itself — dropped rather than folded into the property.
    let rest = line[name_end..].split(';').next().unwrap_or("").trim();
    // Extras (`requests[security]==2.0`) aren't a separate dependency — folded away, only the
    // version spec itself is kept.
    let version_spec = if let Some(close) = rest.find(']') {
        rest[close + 1..].trim()
    } else {
        rest
    };
    Some(ParsedRequirement {
        name: name.to_string(),
        version_spec: version_spec.to_string(),
    })
}

pub struct RequirementsAnalyzerPass {
    pass_id: String,
    /// (relative path to `requirements.txt`, raw file content, RFC 0079 project qualifier).
    manifests: Vec<(String, String, Option<String>)>,
}

impl RequirementsAnalyzerPass {
    pub fn new(
        workspace_name: impl Into<String>,
        manifests: Vec<(String, String, Option<String>)>,
    ) -> Self {
        Self {
            pass_id: format!("requirements-analyzer:{}", workspace_name.into()),
            manifests,
        }
    }
}

fn parse_manifests(manifests: &[(String, String, Option<String>)]) -> KirGraph {
    let mut graph = KirGraph::new();
    let mut technology_ids: HashMap<String, KirId> = HashMap::new();

    for (rel_path, content, project) in manifests {
        let id_path = ekos_common::project::project_qualify(rel_path, project.as_deref());
        let file_id = file_kir_id(&id_path);

        for raw_line in content.lines() {
            let Some(req) = parse_requirement_line(raw_line) else {
                continue;
            };
            let qualified_name =
                ekos_common::project::project_qualify(&req.name, project.as_deref());
            let tech_id = *technology_ids
                .entry(qualified_name.clone())
                .or_insert_with(|| technology_kir_id(&qualified_name));
            if !graph.objects.iter().any(|o| o.id == tech_id) {
                let mut tech_obj = KirObject::new(
                    req.name.clone(),
                    ObjectKind::Custom("Technology".to_string()),
                );
                tech_obj.id = tech_id;
                tech_obj
                    .properties
                    .insert("ecosystem".into(), serde_json::json!("pip"));
                graph.add_object(tech_obj);
            }

            let ev = KirEvidence::new(SourceLocation::file(rel_path.clone()), raw_line.to_string());
            let ev_id = graph.add_evidence(ev);
            let mut rel = KirRelationship::new(RelationshipKind::DependsOn, file_id, tech_id);
            rel.id = depends_on_kir_id(file_id, tech_id);
            rel.properties
                .insert("version_spec".into(), serde_json::json!(req.version_spec));
            rel.evidence.push(ev_id);
            graph.add_relationship(rel);
        }
    }

    graph
}

#[async_trait]
impl CompilerPass for RequirementsAnalyzerPass {
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
            "requirements-analyzer complete"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_one(content: &str) -> KirGraph {
        parse_manifests(&[("requirements.txt".to_string(), content.to_string(), None)])
    }

    #[test]
    fn extracts_real_pinned_and_ranged_dependencies() {
        let graph = run_one("fastapi==0.109.0\nsqlalchemy>=2.0,<3.0\nrequests\n");
        let names: Vec<&str> = graph.objects.iter().map(|o| o.name.as_str()).collect();
        assert!(names.contains(&"fastapi"));
        assert!(names.contains(&"sqlalchemy"));
        assert!(names.contains(&"requests"));

        let fastapi = graph.objects.iter().find(|o| o.name == "fastapi").unwrap();
        assert_eq!(fastapi.kind, ObjectKind::Custom("Technology".to_string()));
        assert_eq!(fastapi.properties["ecosystem"], "pip");

        let fastapi_rel = graph
            .relationships
            .iter()
            .find(|r| r.to == fastapi.id)
            .unwrap();
        assert_eq!(fastapi_rel.kind, RelationshipKind::DependsOn);
        assert_eq!(fastapi_rel.properties["version_spec"], "==0.109.0");

        let sqlalchemy = graph
            .objects
            .iter()
            .find(|o| o.name == "sqlalchemy")
            .unwrap();
        let sqlalchemy_rel = graph
            .relationships
            .iter()
            .find(|r| r.to == sqlalchemy.id)
            .unwrap();
        assert_eq!(sqlalchemy_rel.properties["version_spec"], ">=2.0,<3.0");

        let requests = graph.objects.iter().find(|o| o.name == "requests").unwrap();
        let requests_rel = graph
            .relationships
            .iter()
            .find(|r| r.to == requests.id)
            .unwrap();
        assert_eq!(
            requests_rel.properties["version_spec"], "",
            "a bare package name with no constraint has an empty version spec, not a fabricated one"
        );
    }

    #[test]
    fn comments_and_blank_lines_are_skipped() {
        let graph = run_one("# a comment\n\n   \nfastapi==0.109.0  # inline comment too\n");
        assert_eq!(graph.objects.len(), 1);
        assert_eq!(graph.objects[0].name, "fastapi");
        let rel = graph.relationships.first().unwrap();
        assert_eq!(rel.properties["version_spec"], "==0.109.0");
    }

    #[test]
    fn option_and_vcs_lines_are_skipped_not_fabricated() {
        let graph = run_one(
            "-r base.txt\n-e .\n--index-url https://example.com/simple\n\
             git+https://github.com/example/pkg.git\nfastapi==0.109.0\n",
        );
        assert_eq!(
            graph.objects.len(),
            1,
            "only the one real, plain requirement must produce a Technology"
        );
        assert_eq!(graph.objects[0].name, "fastapi");
    }

    #[test]
    fn extras_and_environment_markers_are_stripped_from_the_name_and_version_spec() {
        let graph = run_one("requests[security]>=2.0; python_version >= \"3.8\"\n");
        assert_eq!(graph.objects.len(), 1);
        assert_eq!(graph.objects[0].name, "requests");
        let rel = graph.relationships.first().unwrap();
        assert_eq!(rel.properties["version_spec"], ">=2.0");
    }

    #[test]
    fn same_dependency_across_two_manifests_dedupes_to_one_technology_object() {
        let graph = parse_manifests(&[
            (
                "backend/requirements.txt".to_string(),
                "fastapi==0.109.0\n".to_string(),
                None,
            ),
            (
                "worker/requirements.txt".to_string(),
                "fastapi==0.109.0\n".to_string(),
                None,
            ),
        ]);
        let fastapi_objects: Vec<_> = graph
            .objects
            .iter()
            .filter(|o| o.name == "fastapi")
            .collect();
        assert_eq!(fastapi_objects.len(), 1);
        assert_eq!(
            graph
                .relationships
                .iter()
                .filter(|r| r.to == fastapi_objects[0].id)
                .count(),
            2,
            "two real, distinct files each declaring the dependency"
        );
    }

    #[test]
    fn depends_on_ids_are_deterministic_across_two_independent_runs() {
        let content = "fastapi==0.109.0\n";
        let g1 = run_one(content);
        let g2 = run_one(content);
        assert_eq!(g1.relationships[0].id, g2.relationships[0].id);
    }

    #[test]
    fn a_project_field_qualifies_the_file_id_and_technology_id() {
        let ga = parse_manifests(&[(
            "requirements.txt".to_string(),
            "fastapi==0.109.0\n".to_string(),
            Some("backend-a".to_string()),
        )]);
        let gb = parse_manifests(&[(
            "requirements.txt".to_string(),
            "fastapi==0.109.0\n".to_string(),
            Some("backend-b".to_string()),
        )]);
        let a = ga.objects.iter().find(|o| o.name == "fastapi").unwrap();
        let b = gb.objects.iter().find(|o| o.name == "fastapi").unwrap();
        assert_ne!(
            a.id, b.id,
            "the same package name in two different projects must not collide"
        );
    }

    #[test]
    fn an_empty_manifest_produces_nothing() {
        let graph = run_one("");
        assert!(graph.objects.is_empty());
        assert!(graph.relationships.is_empty());
    }
}
