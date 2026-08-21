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

/// Deterministic id for a `Custom("Claim")` object (RFC 0065 Phase 1), keyed by the
/// (subject crate dir, object id) pair the claim was synthesized from — the same claim
/// re-derived on a re-run must resolve to the same object, not a duplicate.
fn claim_kir_id(subject_dir: &str, object_id: KirId) -> KirId {
    KirId(Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("claim:{subject_dir}:{object_id}").as_bytes(),
    ))
}

/// Deterministic id for a `Custom("ArchitectureGap")` object (RFC 0065 Phase 1), keyed by
/// (crate manifest dir, unresolved dependency name).
fn architecture_gap_kir_id(crate_dir: &str, dep_name: &str) -> KirId {
    KirId(Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("architecture-gap:{crate_dir}:{dep_name}").as_bytes(),
    ))
}

/// Adds one Fact-type `Custom("Claim")` object for a `DependsOn` relationship already added to
/// `graph` (RFC 0065 §12-13 — "Facts should preferably come from deterministic extraction").
/// Reuses the relationship's own evidence rather than duplicating it. `Inference`/`Assumption`/
/// `Recommendation`-type claims require the reasoning layer and are out of scope for this phase —
/// see RFC 0065's own Phase 1 status note.
///
/// Takes `subject_name`/`object_name` directly rather than looking them up in `graph.objects`:
/// crates are added to the graph progressively as `crates` is iterated, so a target crate that
/// appears later in that list wouldn't be findable there yet when its dependent is processed.
#[allow(clippy::too_many_arguments)]
fn add_depends_on_claim(
    graph: &mut KirGraph,
    subject_dir: &str,
    subject_id: KirId,
    subject_name: &str,
    object_id: KirId,
    object_name: &str,
    evidence_id: KirId,
) {
    let mut claim = KirObject::new(
        format!("{subject_name} depends_on {object_name}"),
        ObjectKind::Custom("Claim".to_string()),
    )
    .with_property("subject_id", serde_json::json!(subject_id.to_string()))
    .with_property("predicate", serde_json::json!("depends_on"))
    .with_property("object_id", serde_json::json!(object_id.to_string()))
    .with_property("claim_type", serde_json::json!("fact"))
    .with_evidence(evidence_id);
    claim.id = claim_kir_id(subject_dir, object_id);
    graph.add_object(claim);
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

        // Built upfront (independent of `graph.objects`' progressive insertion order) so
        // `add_depends_on_claim` can always name a claim's subject/object crate even when the
        // target crate appears later in `crates` and hasn't been added to `graph` yet.
        let crate_name_by_id: HashMap<KirId, String> = crates
            .iter()
            .map(|c| (dir_to_id[&c.dir], c.name.clone()))
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
                    DepResolution::Unresolved => {
                        match workspace_deps.get(dep_name) {
                            Some(WorkspaceDep::Path(p)) => DepResolution::Path(p.clone()),
                            Some(WorkspaceDep::Version(v)) => DepResolution::Version(v.clone()),
                            // RFC 0065 Phase 1: previously a silent `continue` — a `{ workspace = true
                            // }` entry with no matching root `[workspace.dependencies]` key, or a
                            // git/registry-index dependency shape not modeled in v1 (see
                            // `DepResolution::Unresolved`'s doc comment), is a real knowledge gap, not
                            // a non-event. Recorded as an evidence-backed `ArchitectureGap` instead of
                            // dropped, matching this project's own "Unmapped is deliberate, not a gap
                            // swept under the rug" philosophy (Transformation IR, RFC 0027).
                            None => {
                                let gap_id = architecture_gap_kir_id(&c.dir, dep_name);
                                let ev = KirEvidence::new(
                                    SourceLocation::file(&manifest_path),
                                    format!(
                                        "{} declares a dependency on '{dep_name}' that could not be \
                                     resolved (workspace = true with no matching \
                                     [workspace.dependencies] entry, or a git/registry-index \
                                     dependency shape not modeled in v1)",
                                        c.name
                                    ),
                                );
                                let ev_id = graph.add_evidence(ev);
                                let mut gap = KirObject::new(
                                format!("unresolved dependency '{dep_name}' for {}", c.name),
                                ObjectKind::Custom("ArchitectureGap".to_string()),
                            )
                            .with_property("question", serde_json::json!(format!(
                                "What does '{dep_name}' resolve to for {}?", c.name
                            )))
                            .with_property("affected_crate", serde_json::json!(c.name))
                            .with_property(
                                "reason",
                                serde_json::json!(
                                    "workspace = true with no matching [workspace.dependencies] \
                                     entry, or a dependency shape not modeled in v1"
                                ),
                            )
                            .with_evidence(ev_id);
                                gap.id = gap_id;
                                graph.add_object(gap);
                                continue;
                            }
                        }
                    }
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

                        // RFC 0065 Phase 1: "X depends_on Y" is a deterministic Fact-type Claim —
                        // reuses the same evidence the relationship above already carries.
                        let target_name = crate_name_by_id
                            .get(&target_id)
                            .map(String::as_str)
                            .unwrap_or(dep_name);
                        add_depends_on_claim(
                            &mut graph,
                            &c.dir,
                            crate_id,
                            &c.name,
                            target_id,
                            target_name,
                            ev_id,
                        );
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

                        add_depends_on_claim(
                            &mut graph, &c.dir, crate_id, &c.name, tech_id, dep_name, ev_id,
                        );
                    }
                    // Unreachable: `resolution` is bound above only via `DepResolution::Path(_)`
                    // (first match arm), the `Unresolved` arm (which converts to `Path`/`Version`
                    // or `continue`s before reaching this second match), or `other => other`
                    // (which `resolve_dep_entry`'s own return type limits to `Version(_)`) — kept
                    // only because the match must stay exhaustive over `DepResolution`.
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

    // ── RFC 0065 Phase 1: Claim / ArchitectureGap ──────────────────────────────

    #[tokio::test]
    async fn internal_depends_on_edge_emits_a_matching_fact_claim() {
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

        let claim = graph
            .objects
            .iter()
            .find(|o| o.name == "ekos-consumer depends_on ekos-kir")
            .expect("a Claim for the consumer -> kir DependsOn edge");
        assert_eq!(claim.kind, ObjectKind::Custom("Claim".to_string()));
        assert_eq!(claim.properties["claim_type"], serde_json::json!("fact"));
        assert_eq!(
            claim.properties["predicate"],
            serde_json::json!("depends_on")
        );
        assert_eq!(
            claim.properties["subject_id"],
            serde_json::json!(consumer.id.to_string())
        );
        assert_eq!(
            claim.properties["object_id"],
            serde_json::json!(kir.id.to_string())
        );
        assert!(!claim.evidence.is_empty(), "claim must carry evidence");
    }

    #[tokio::test]
    async fn external_dependency_edge_emits_a_matching_fact_claim() {
        let graph = run_pass(vec![
            ("Cargo.toml", ROOT),
            ("crates/consumer/Cargo.toml", CONSUMER),
        ])
        .await;

        let claim = graph
            .objects
            .iter()
            .find(|o| o.name == "ekos-consumer depends_on tokio")
            .expect("a Claim for the consumer -> tokio DependsOn edge");
        assert_eq!(claim.kind, ObjectKind::Custom("Claim".to_string()));
        assert_eq!(claim.properties["claim_type"], serde_json::json!("fact"));
    }

    #[tokio::test]
    async fn unresolvable_workspace_dependency_emits_an_architecture_gap_instead_of_being_dropped()
    {
        // `orphan.workspace = true` with no matching `[workspace.dependencies]` entry in ROOT —
        // previously a silent `continue`, now a real, evidence-backed knowledge gap (RFC 0065 §17).
        const ORPHAN_DEP: &str = r#"
[package]
name = "ekos-orphan-dep"
version = "0.1.0"

[dependencies]
orphan.workspace = true
"#;
        let graph = run_pass(vec![
            ("Cargo.toml", ROOT),
            ("crates/orphan/Cargo.toml", ORPHAN_DEP),
        ])
        .await;

        let gap = graph
            .objects
            .iter()
            .find(|o| o.kind == ObjectKind::Custom("ArchitectureGap".to_string()))
            .expect("an ArchitectureGap for the unresolvable 'orphan' dependency");
        assert!(gap.name.contains("orphan"));
        assert!(gap.name.contains("ekos-orphan-dep"));
        assert_eq!(
            gap.properties["affected_crate"],
            serde_json::json!("ekos-orphan-dep")
        );
        assert!(!gap.evidence.is_empty(), "gap must carry evidence");

        // And, crucially, no DependsOn edge or Claim was fabricated for it.
        assert!(
            !graph
                .relationships
                .iter()
                .any(|r| r.kind == RelationshipKind::DependsOn),
            "an unresolved dependency must not produce a fabricated DependsOn edge"
        );
    }
}
