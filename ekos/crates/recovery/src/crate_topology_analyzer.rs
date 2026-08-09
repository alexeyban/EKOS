//! `CrateTopologyAnalyzerPass` — structural extraction of a Rust workspace's own crate topology
//! from `Cargo.toml` manifests (RFC 0042). Complements `dependency_analyzer.rs`'s pattern-based
//! technology detection (which only sees import statements/connection strings in *other*
//! languages' source text) with a real, parsed view of a Cargo workspace's own architecture: which
//! crate depends on which crate (internal `DependsOn` edges, the "real infrastructure" a hand-
//! written crate map otherwise has to document by hand), and which external crates each one
//! declares (`Custom("Technology")` objects — the exact object kind `dependency_analyzer.rs`
//! already emits, so both analyzers' output lands in the same "Technologies" section).
//!
//! Deliberately structural, not a full Cargo resolver: a manifest's own `[dependencies]` table
//! (plus the root workspace manifest's `[workspace.dependencies]`, which member crates reference
//! via `dep.workspace = true`) is read directly; `Cargo.lock`, transitive dependencies, and
//! `[dev-dependencies]`/`[build-dependencies]` are out of scope for v1 — the same "read what's
//! declared, don't resolve a full graph" spirit as RFC 0019's pattern table.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

use async_trait::async_trait;
use ekos_compiler_core::pass::{CompilerPass, PassContext, PassError};
use ekos_kir::{
    KirEvidence, KirGraph, KirId, KirObject, KirRelationship, ObjectKind, RelationshipKind,
    SourceLocation,
};
use uuid::Uuid;

/// Deterministic id for a `Custom("Crate")` object, keyed by the crate's manifest directory
/// (workspace-unique, unlike crate *name* alone across e.g. `benchmark/` and `tests/integration/`
/// being separate Cargo workspaces per CLAUDE.md).
fn crate_kir_id(dir: &str) -> KirId {
    KirId(Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("crate:{dir}").as_bytes(),
    ))
}

/// Same id scheme `dependency_analyzer.rs`'s private `technology_kir_id` uses — kept in sync
/// deliberately so a technology detected by both analyzers (unlikely in practice, since one scans
/// connection strings and the other parses `Cargo.toml`, but possible) resolves to one object.
fn technology_kir_id(name: &str) -> KirId {
    KirId(Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("technology:{name}").as_bytes(),
    ))
}

/// Lexically joins `base` and `rel` and collapses `.`/`..` components — no filesystem access, no
/// symlink resolution, just enough to turn `crates/cli/../recovery` into `crates/recovery` so it
/// matches another manifest's own directory path.
fn normalize_rel_path(base: &Path, rel: &str) -> String {
    let joined = base.join(rel);
    let mut stack: Vec<Component> = Vec::new();
    for component in joined.components() {
        match component {
            Component::ParentDir => {
                if matches!(stack.last(), Some(Component::Normal(_))) {
                    stack.pop();
                } else {
                    stack.push(component);
                }
            }
            Component::CurDir => {}
            other => stack.push(other),
        }
    }
    let mut out = PathBuf::new();
    for c in stack {
        out.push(c.as_os_str());
    }
    out.to_string_lossy().replace('\\', "/")
}

/// A parsed `[workspace.dependencies]` entry: an internal path or an external version, never
/// both — matches how a real `Cargo.toml` declares one or the other per dependency.
enum WorkspaceDep {
    Path(String),
    Version(Option<String>),
}

/// What a `[dependencies]`-table entry (of any of TOML's three shapes for it — bare version
/// string, `{ path = ... }`, or `{ workspace = true }`) resolves to.
enum DepResolution {
    Path(String),
    Version(Option<String>),
    /// `{ workspace = true }` with no matching root `[workspace.dependencies]` entry, or a git/
    /// registry-index dependency shape not modeled in v1 — skipped, not fabricated.
    Unresolved,
}

fn resolve_dep_entry(value: &toml::Value) -> DepResolution {
    match value {
        toml::Value::String(version) => DepResolution::Version(Some(version.clone())),
        toml::Value::Table(t) => {
            if let Some(toml::Value::String(path)) = t.get("path") {
                return DepResolution::Path(path.clone());
            }
            if t.get("workspace").and_then(|v| v.as_bool()) == Some(true) {
                return DepResolution::Unresolved; // resolved by the caller via `workspace_deps`
            }
            if let Some(toml::Value::String(version)) = t.get("version") {
                return DepResolution::Version(Some(version.clone()));
            }
            DepResolution::Version(None)
        }
        _ => DepResolution::Unresolved,
    }
}

pub struct CrateTopologyAnalyzerPass {
    pass_id: String,
    /// (relative manifest path, raw `Cargo.toml` content) pairs, one per discovered manifest.
    manifests: Vec<(String, String)>,
}

impl CrateTopologyAnalyzerPass {
    pub fn new(workspace_name: impl Into<String>, manifests: Vec<(String, String)>) -> Self {
        Self {
            pass_id: format!("crate-topology-analyzer:{}", workspace_name.into()),
            manifests,
        }
    }
}

#[async_trait]
impl CompilerPass for CrateTopologyAnalyzerPass {
    fn name(&self) -> &str {
        &self.pass_id
    }

    fn cache_inputs(&self) -> Vec<String> {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        let mut paths: Vec<&str> = self.manifests.iter().map(|(p, _)| p.as_str()).collect();
        paths.sort();
        for path in paths {
            let (_, content) = self.manifests.iter().find(|(p, _)| p == path).unwrap();
            hasher.update(path.as_bytes());
            hasher.update(content.as_bytes());
        }
        vec![hex::encode(hasher.finalize())]
    }

    async fn run(&mut self, ctx: &mut PassContext) -> Result<(), PassError> {
        let mut graph = KirGraph::new();

        // Pass 1: parse every manifest, collect `[workspace.dependencies]` (root manifest only —
        // there is exactly one per Cargo workspace, but any manifest may carry one if it happens
        // to be a workspace root) and every `[package]` manifest's own directory, so path
        // dependencies (declared relative to the *dependent* crate, or to the workspace root for
        // `workspace = true` entries) can be resolved in pass 2.
        let mut workspace_deps: HashMap<String, WorkspaceDep> = HashMap::new();
        struct ParsedCrate {
            dir: String,
            name: String,
            version: Option<String>,
            description: String,
            deps: toml::value::Table,
        }
        let mut crates: Vec<ParsedCrate> = Vec::new();

        for (rel_path, content) in &self.manifests {
            let doc: toml::Value = match content.parse() {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!("cannot parse {rel_path} as TOML: {e}");
                    continue;
                }
            };
            let manifest_dir = Path::new(rel_path).parent().unwrap_or(Path::new(""));

            if let Some(deps) = doc
                .get("workspace")
                .and_then(|v| v.as_table())
                .and_then(|ws| ws.get("dependencies"))
                .and_then(|v| v.as_table())
            {
                for (name, value) in deps {
                    let entry = match value {
                        toml::Value::Table(t) => match t.get("path").and_then(|v| v.as_str()) {
                            Some(path) => {
                                WorkspaceDep::Path(normalize_rel_path(manifest_dir, path))
                            }
                            None => WorkspaceDep::Version(
                                t.get("version")
                                    .and_then(|v| v.as_str())
                                    .map(|s| s.to_string()),
                            ),
                        },
                        toml::Value::String(v) => WorkspaceDep::Version(Some(v.clone())),
                        _ => WorkspaceDep::Version(None),
                    };
                    workspace_deps.insert(name.clone(), entry);
                }
            }

            if let Some(pkg) = doc.get("package").and_then(|v| v.as_table()) {
                let name = match pkg.get("name").and_then(|v| v.as_str()) {
                    Some(n) => n.to_string(),
                    None => continue,
                };
                let version = pkg
                    .get("version")
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                let description = pkg
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                let deps = doc
                    .get("dependencies")
                    .and_then(|v| v.as_table())
                    .cloned()
                    .unwrap_or_default();
                crates.push(ParsedCrate {
                    dir: manifest_dir.to_string_lossy().replace('\\', "/"),
                    name,
                    version,
                    description,
                    deps,
                });
            }
        }

        if crates.is_empty() {
            return Ok(());
        }

        let dir_to_id: HashMap<String, KirId> = crates
            .iter()
            .map(|c| (c.dir.clone(), crate_kir_id(&c.dir)))
            .collect();

        let mut technology_ids: HashMap<String, KirId> = HashMap::new();

        for c in &crates {
            let crate_id = dir_to_id[&c.dir];
            let mut obj = KirObject::new(c.name.clone(), ObjectKind::Custom("Crate".to_string()))
                .with_property("path", serde_json::json!(c.dir))
                .with_property("description", serde_json::json!(c.description));
            obj.id = crate_id;
            if let Some(version) = &c.version {
                obj = obj.with_property("version", serde_json::json!(version));
            }
            graph.add_object(obj);

            let manifest_dir = Path::new(&c.dir);
            let manifest_path = format!("{}/Cargo.toml", c.dir);

            for (dep_name, value) in &c.deps {
                // `Path` here is always a fully-resolved, repo-root-relative directory by the
                // time this `match` sees it: `resolve_dep_entry`'s own `path = "..."` case is
                // relative to *this* crate's manifest and is normalized immediately below; the
                // `workspace_deps` fallback was already normalized relative to the *root*
                // manifest's directory back in pass 1 (`WorkspaceDep::Path` construction) and
                // must not be re-joined against this crate's own directory a second time.
                let resolution = match resolve_dep_entry(value) {
                    DepResolution::Path(raw) => {
                        DepResolution::Path(normalize_rel_path(manifest_dir, &raw))
                    }
                    DepResolution::Unresolved => match workspace_deps.get(dep_name) {
                        Some(WorkspaceDep::Path(p)) => DepResolution::Path(p.clone()),
                        Some(WorkspaceDep::Version(v)) => DepResolution::Version(v.clone()),
                        None => continue,
                    },
                    other => other,
                };

                match resolution {
                    DepResolution::Path(target_dir) => {
                        let Some(&target_id) = dir_to_id.get(&target_dir) else {
                            continue;
                        };
                        if target_id == crate_id {
                            continue;
                        }
                        let ev = KirEvidence::new(
                            SourceLocation::file(&manifest_path),
                            format!("{} depends on {} (path dependency)", c.name, dep_name),
                        );
                        let ev_id = graph.add_evidence(ev);
                        let mut rel =
                            KirRelationship::new(RelationshipKind::DependsOn, crate_id, target_id);
                        rel.evidence.push(ev_id);
                        graph.add_relationship(rel);
                    }
                    DepResolution::Version(version) => {
                        let tech_id = *technology_ids
                            .entry(dep_name.clone())
                            .or_insert_with(|| technology_kir_id(dep_name));
                        if !graph.objects.iter().any(|o| o.id == tech_id) {
                            let mut tech_obj = KirObject::new(
                                dep_name.clone(),
                                ObjectKind::Custom("Technology".to_string()),
                            );
                            tech_obj.id = tech_id;
                            graph.add_object(tech_obj);
                        }
                        let fragment = match &version {
                            Some(v) => format!("{} depends on {dep_name} {v}", c.name),
                            None => format!("{} depends on {dep_name}", c.name),
                        };
                        let ev = KirEvidence::new(SourceLocation::file(&manifest_path), fragment);
                        let ev_id = graph.add_evidence(ev);
                        let mut rel =
                            KirRelationship::new(RelationshipKind::DependsOn, crate_id, tech_id);
                        rel.evidence.push(ev_id);
                        graph.add_relationship(rel);
                    }
                    DepResolution::Unresolved => {}
                }
            }
        }

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
            crates = knowledge.content.kir.objects.iter().filter(|o| o.kind == ObjectKind::Custom("Crate".to_string())).count(),
            technologies = knowledge.content.kir.objects.iter().filter(|o| o.kind == ObjectKind::Custom("Technology".to_string())).count(),
            edges = knowledge.content.kir.relationships.len(),
            "crate-topology-analyzer complete"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_compiler_core::EkosConfig;
    use std::sync::Arc;

    fn ctx() -> (PassContext, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        (
            PassContext::new(Arc::new(EkosConfig::default()), dir.path().to_path_buf()),
            dir,
        )
    }

    async fn run_pass(manifests: Vec<(&str, &str)>) -> ekos_kir::KirGraph {
        let (mut c, _dir) = ctx();
        let manifests = manifests
            .into_iter()
            .map(|(p, s)| (p.to_string(), s.to_string()))
            .collect();
        let mut pass = CrateTopologyAnalyzerPass::new("test", manifests);
        pass.run(&mut c).await.unwrap();

        let ids = c.artifact_store.list().unwrap();
        assert_eq!(ids.len(), 1, "exactly one KnowledgeArtifact expected");
        let json = c.artifact_store.read(&ids[0]).unwrap().unwrap();
        let knowledge: ekos_artifact::KnowledgeArtifact = serde_json::from_value(json).unwrap();
        knowledge.content.kir
    }

    const ROOT: &str = r#"
[workspace]
members = ["crates/kir", "crates/consumer"]

[workspace.dependencies]
ekos-kir = { path = "crates/kir" }
serde = { version = "1", features = ["derive"] }
"#;

    const KIR: &str = r#"
[package]
name = "ekos-kir"
version = "0.1.0"
description = "Knowledge IR types"

[dependencies]
serde.workspace = true
"#;

    const CONSUMER: &str = r#"
[package]
name = "ekos-consumer"
version = "0.1.0"

[dependencies]
ekos-kir.workspace = true
tokio = "1"
"#;

    #[tokio::test]
    async fn parses_package_metadata_into_crate_objects() {
        let graph = run_pass(vec![
            ("Cargo.toml", ROOT),
            ("crates/kir/Cargo.toml", KIR),
            ("crates/consumer/Cargo.toml", CONSUMER),
        ])
        .await;

        let kir_crate = graph
            .objects
            .iter()
            .find(|o| o.name == "ekos-kir")
            .expect("ekos-kir crate object");
        assert_eq!(kir_crate.kind, ObjectKind::Custom("Crate".to_string()));
        assert_eq!(
            kir_crate.properties["description"],
            serde_json::json!("Knowledge IR types")
        );
        assert_eq!(
            kir_crate.properties["path"],
            serde_json::json!("crates/kir")
        );
    }

    #[tokio::test]
    async fn workspace_true_path_dependency_resolves_to_an_internal_depends_on_edge() {
        let graph = run_pass(vec![
            ("Cargo.toml", ROOT),
            ("crates/kir/Cargo.toml", KIR),
            ("crates/consumer/Cargo.toml", CONSUMER),
        ])
        .await;

        let consumer = graph
            .objects
            .iter()
            .find(|o| o.name == "ekos-consumer")
            .unwrap();
        let kir = graph.objects.iter().find(|o| o.name == "ekos-kir").unwrap();
        assert!(
            graph
                .relationships
                .iter()
                .any(|r| r.kind == RelationshipKind::DependsOn
                    && r.from == consumer.id
                    && r.to == kir.id)
        );
    }

    #[tokio::test]
    async fn external_dependency_emits_a_technology_object() {
        let graph = run_pass(vec![
            ("Cargo.toml", ROOT),
            ("crates/consumer/Cargo.toml", CONSUMER),
        ])
        .await;

        let tokio_tech = graph
            .objects
            .iter()
            .find(|o| o.name == "tokio")
            .expect("tokio Technology object");
        assert_eq!(
            tokio_tech.kind,
            ObjectKind::Custom("Technology".to_string())
        );

        let consumer = graph
            .objects
            .iter()
            .find(|o| o.name == "ekos-consumer")
            .unwrap();
        assert!(
            graph
                .relationships
                .iter()
                .any(|r| r.kind == RelationshipKind::DependsOn
                    && r.from == consumer.id
                    && r.to == tokio_tech.id)
        );
    }

    #[tokio::test]
    async fn workspace_true_external_dependency_resolves_via_root_manifest() {
        let graph = run_pass(vec![("Cargo.toml", ROOT), ("crates/kir/Cargo.toml", KIR)]).await;

        let serde_tech = graph
            .objects
            .iter()
            .find(|o| o.name == "serde")
            .expect("serde Technology object resolved through workspace.dependencies");
        assert_eq!(
            serde_tech.kind,
            ObjectKind::Custom("Technology".to_string())
        );
    }

    #[tokio::test]
    async fn same_external_dependency_across_crates_dedupes_to_one_object() {
        const OTHER: &str = r#"
[package]
name = "ekos-other"
version = "0.1.0"

[dependencies]
tokio = "1"
"#;
        let graph = run_pass(vec![
            ("Cargo.toml", ROOT),
            ("crates/consumer/Cargo.toml", CONSUMER),
            ("crates/other/Cargo.toml", OTHER),
        ])
        .await;

        let tokio_objects: Vec<_> = graph.objects.iter().filter(|o| o.name == "tokio").collect();
        assert_eq!(
            tokio_objects.len(),
            1,
            "tokio Technology object must be shared"
        );
    }

    #[tokio::test]
    async fn manifest_with_no_package_table_emits_nothing_for_itself() {
        let (mut c, _dir) = ctx();
        let mut pass = CrateTopologyAnalyzerPass::new(
            "test",
            vec![("Cargo.toml".to_string(), ROOT.to_string())],
        );
        pass.run(&mut c).await.unwrap();
        assert!(
            c.artifact_store.list().unwrap().is_empty(),
            "no KnowledgeArtifact should be written when no [package] manifest exists"
        );
    }
}
