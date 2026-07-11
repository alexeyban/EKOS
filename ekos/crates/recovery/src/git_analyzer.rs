//! `GitAnalyzerPass` — converts git commit observation artifacts into KIR.
//!
//! Produces:
//! - `KirObject(kind=Entity)` per unique contributor
//! - `KirEvent(kind=Modified)` per commit
//! - `KirRelationship(kind=CoupledWith)` for files changed together in ≥2 commits

use std::collections::HashMap;

use async_trait::async_trait;
use ekos_artifact::ArtifactId;
use ekos_compiler_core::pass::{CompilerPass, PassContext, PassError};
use ekos_kir::{
    EventKind, KirEvent, KirEvidence, KirGraph, KirId, KirObject, KirRelationship, ObjectKind,
    RelationshipKind, SourceLocation,
};
use uuid::Uuid;

/// Minimum number of times two files must co-change before a `CoupledWith`
/// relationship is emitted. Configurable at construction time.
const DEFAULT_MIN_COUPLING: usize = 2;

pub struct GitAnalyzerPass {
    pass_id: String,
    /// Git commit ObservationArtifact IDs to process.
    commit_artifact_ids: Vec<ArtifactId>,
    /// Artifact IDs that are the repository metadata artifacts.
    repo_artifact_id: Option<ArtifactId>,
    min_coupling: usize,
}

impl GitAnalyzerPass {
    pub fn new(
        workspace_name: impl Into<String>,
        commit_artifact_ids: Vec<ArtifactId>,
        repo_artifact_id: Option<ArtifactId>,
    ) -> Self {
        let workspace_name = workspace_name.into();
        Self {
            pass_id: format!("git-analyzer:{workspace_name}"),
            commit_artifact_ids,
            repo_artifact_id,
            min_coupling: DEFAULT_MIN_COUPLING,
        }
    }

    pub fn with_min_coupling(mut self, n: usize) -> Self {
        self.min_coupling = n;
        self
    }
}

#[async_trait]
impl CompilerPass for GitAnalyzerPass {
    fn name(&self) -> &str {
        &self.pass_id
    }

    fn cache_inputs(&self) -> Vec<String> {
        let mut ids: Vec<String> =
            self.commit_artifact_ids.iter().map(|id| id.to_string()).collect();
        ids.sort();
        if let Some(repo_id) = &self.repo_artifact_id {
            ids.push(repo_id.to_string());
        }
        ids
    }

    async fn run(&mut self, ctx: &mut PassContext) -> Result<(), PassError> {
        let mut graph = KirGraph::new();

        // ── Gather contributor names (from repo artifact if present) ─────────
        let mut contributor_ids: HashMap<String, KirId> = HashMap::new();

        if let Some(ref repo_id) = self.repo_artifact_id
            && let Ok(Some(json)) = ctx.artifact_store.read(repo_id)
            && let Some(contributors) = json["data"]["contributors"].as_array()
        {
            for c in contributors {
                if let Some(name) = c["name"].as_str() {
                    let id = contributor_kir_id(name);
                    let ev = KirEvidence::new(
                        SourceLocation::file("git:contributors"),
                        format!("contributor: {name}"),
                    );
                    let ev_id = graph.add_evidence(ev);
                    let mut obj =
                        KirObject::new(name, ObjectKind::Entity).with_evidence(ev_id);
                    obj.id = id;
                    obj.properties.insert("role".into(), serde_json::json!("contributor"));
                    if let Some(commits) = c["commits"].as_u64() {
                        obj.properties
                            .insert("commit_count".into(), serde_json::json!(commits));
                    }
                    graph.objects.push(obj);
                    contributor_ids.insert(name.to_string(), id);
                }
            }
        }

        // ── Process each commit artifact → KirEvent ──────────────────────────
        let mut file_co_changes: HashMap<(String, String), usize> = HashMap::new();

        for artifact_id in &self.commit_artifact_ids {
            let json = match ctx.artifact_store.read(artifact_id) {
                Ok(Some(j)) => j,
                Ok(None) => {
                    ctx.diagnostics.lock().unwrap().warning(
                        "GIT001",
                        format!("commit artifact {artifact_id} not found in store"),
                    );
                    continue;
                }
                Err(e) => {
                    ctx.diagnostics
                        .lock()
                        .unwrap()
                        .warning("GIT002", format!("failed to read artifact {artifact_id}: {e}"));
                    continue;
                }
            };

            // Commit fields live under `data` — `ObservationArtifact`'s
            // `#[serde(flatten)]` merges `connector_name`/`target`/`data`/`input_ids`
            // into the top-level JSON object, but `data` itself stays a nested object
            // (flatten doesn't recurse into it). Indexing `json["sha"]` directly here
            // was a real bug: it silently resolved to `Value::Null` for every real
            // commit artifact, so this pass never actually read commit metadata or
            // produced `CoupledWith` relationships against any real repository —
            // caught by an integration test asserting on the pipeline's actual output
            // instead of just "no error thrown."
            let data = &json["data"];
            let sha = data["sha"].as_str().unwrap_or("unknown").to_string();
            let author = data["author_name"].as_str().unwrap_or("unknown").to_string();
            let message = data["message"].as_str().unwrap_or("").to_string();
            let date = data["date"].as_str().unwrap_or("").to_string();

            let files_changed: Vec<String> = data["files_changed"]
                .as_array()
                .unwrap_or(&vec![])
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect();

            // Stable subject KirId from commit SHA.
            let subject_id = KirId(Uuid::new_v5(&Uuid::NAMESPACE_URL, sha.as_bytes()));

            let ev = KirEvidence::new(
                SourceLocation::file(format!("git:commit:{sha}")),
                format!("{sha}: {message}"),
            );
            let ev_id = graph.add_evidence(ev);

            let event = KirEvent {
                id: KirId(Uuid::new_v5(
                    &Uuid::NAMESPACE_URL,
                    format!("event:{sha}").as_bytes(),
                )),
                kind: EventKind::Modified,
                subject: subject_id,
                payload: serde_json::json!({
                    "sha": sha,
                    "author": author,
                    "date": date,
                    "message": message,
                    "files_changed": files_changed,
                }),
                evidence: vec![ev_id],
                occurred_at: chrono::Utc::now(),
            };
            graph.events.push(event);

            // Count co-changes for coupling analysis.
            let sorted_files: Vec<String> = {
                let mut f = files_changed.clone();
                f.sort();
                f
            };
            for i in 0..sorted_files.len() {
                for j in (i + 1)..sorted_files.len() {
                    let pair =
                        (sorted_files[i].clone(), sorted_files[j].clone());
                    *file_co_changes.entry(pair).or_insert(0) += 1;
                }
            }

            // Authorship relationship: contributor → commit event.
            if let Some(&contrib_id) = contributor_ids.get(&author) {
                let rel = KirRelationship::new(RelationshipKind::OwnedBy, subject_id, contrib_id);
                graph.add_relationship(rel);
            }
        }

        // ── Emit CoupledWith relationships ────────────────────────────────────
        for ((file_a, file_b), count) in &file_co_changes {
            if *count < self.min_coupling {
                continue;
            }
            let id_a = KirId(Uuid::new_v5(&Uuid::NAMESPACE_URL, file_a.as_bytes()));
            let id_b = KirId(Uuid::new_v5(&Uuid::NAMESPACE_URL, file_b.as_bytes()));
            let mut rel = KirRelationship::new(RelationshipKind::CoupledWith, id_a, id_b);
            rel.properties.insert("co_change_count".into(), serde_json::json!(count));
            graph.add_relationship(rel);
        }

        // ── Write KnowledgeArtifact ─────────────────────────────────────────
        let knowledge = ekos_artifact::KnowledgeArtifact::new(
            &self.pass_id,
            self.commit_artifact_ids.clone(),
            graph,
        );
        let json = serde_json::to_value(&knowledge)
            .map_err(|e| PassError::failed(format!("serialize: {e}")))?;
        ctx.artifact_store
            .write(&knowledge.id, &json)
            .map_err(|e| PassError::failed(format!("write artifact: {e}")))?;

        tracing::info!(
            pass = %self.pass_id,
            events = knowledge.content.kir.events.len(),
            contributors = contributor_ids.len(),
            "git-analyzer complete"
        );
        Ok(())
    }
}

/// Stable UUIDv5 for a contributor, so the same name always maps to the same KirId.
fn contributor_kir_id(name: &str) -> KirId {
    KirId(Uuid::new_v5(&Uuid::NAMESPACE_URL, format!("contributor:{name}").as_bytes()))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_artifact::{ArtifactStore, FileSystemArtifactStore, ObservationArtifact};
    use ekos_compiler_core::pass::PassContext;
    use std::sync::Arc;
    use tempfile::TempDir;

    fn make_commit_artifact(sha: &str, author: &str, files: &[&str]) -> ObservationArtifact {
        ObservationArtifact::new(
            "git",
            sha,
            serde_json::json!({
                "sha": sha,
                "author_name": author,
                "author_email": format!("{author}@example.com"),
                "date": "2026-07-02T10:00:00Z",
                "message": format!("commit by {author}"),
                "files_changed": files,
                "insertions": 5,
                "deletions": 2,
            }),
        )
    }

    fn make_ctx_with_store(dir: &TempDir) -> PassContext {
        let config = ekos_compiler_core::EkosConfig::default();
        let cwd = dir.path().to_path_buf();
        std::fs::create_dir_all(cwd.join(".ekos/artifacts")).unwrap();
        PassContext::new(Arc::new(config), cwd)
    }

    fn seed_artifact(store: &FileSystemArtifactStore, artifact: &ObservationArtifact) -> ArtifactId {
        let json = serde_json::to_value(artifact).unwrap();
        store.write(&artifact.id, &json).unwrap();
        artifact.id.clone()
    }

    /// Read back the single `KnowledgeArtifact` this pass wrote and decode its `KirGraph`.
    fn read_knowledge_graph(store: &FileSystemArtifactStore) -> KirGraph {
        for id in store.list().unwrap() {
            let json = store.read(&id).unwrap().unwrap();
            if json["artifact_type"] == "knowledge" {
                return serde_json::from_value(json["kir"].clone()).unwrap();
            }
        }
        panic!("no knowledge artifact found in store");
    }

    #[tokio::test]
    async fn git_analyzer_produces_events_per_commit() {
        let dir = TempDir::new().unwrap();
        let store = FileSystemArtifactStore::new(dir.path().join(".ekos/artifacts"));
        std::fs::create_dir_all(dir.path().join(".ekos/artifacts")).unwrap();

        let a1 = make_commit_artifact("sha1abc", "Alice", &["main.rs", "lib.rs"]);
        let a2 = make_commit_artifact("sha2def", "Bob", &["main.rs", "tests.rs"]);
        let id1 = seed_artifact(&store, &a1);
        let id2 = seed_artifact(&store, &a2);

        let mut pass =
            GitAnalyzerPass::new("test-repo", vec![id1, id2], None).with_min_coupling(1);
        let mut ctx = make_ctx_with_store(&dir);
        pass.run(&mut ctx).await.unwrap();

        assert!(!ctx.diagnostics.lock().unwrap().has_errors());

        // Assert the actual extracted values, not just "no error" — commit metadata
        // must be read from the real `sha`/`author_name` fields, not silently default
        // to "unknown".
        let graph = read_knowledge_graph(&store);
        assert_eq!(graph.events.len(), 2, "one event per commit artifact");
        let shas: Vec<String> = graph
            .events
            .iter()
            .map(|e| e.payload["sha"].as_str().unwrap().to_string())
            .collect();
        assert!(shas.contains(&"sha1abc".to_string()));
        assert!(shas.contains(&"sha2def".to_string()));
        assert!(
            graph.events.iter().all(|e| e.payload["author"] != "unknown"),
            "author must be read from the real commit data, not default to 'unknown'"
        );
    }

    #[tokio::test]
    async fn git_analyzer_detects_coupling() {
        let dir = TempDir::new().unwrap();
        let store = FileSystemArtifactStore::new(dir.path().join(".ekos/artifacts"));
        std::fs::create_dir_all(dir.path().join(".ekos/artifacts")).unwrap();

        // a.rs and b.rs change together in 3 commits
        let mut ids = vec![];
        for i in 0u32..3 {
            let a = make_commit_artifact(&format!("sha{i}"), "Alice", &["a.rs", "b.rs"]);
            ids.push(seed_artifact(&store, &a));
        }

        let mut pass = GitAnalyzerPass::new("repo", ids, None).with_min_coupling(2);
        let mut ctx = make_ctx_with_store(&dir);
        pass.run(&mut ctx).await.unwrap();

        assert!(!ctx.diagnostics.lock().unwrap().has_errors());

        // Regression test: `files_changed` must actually be read from the commit
        // artifact's real data, not silently resolve to an empty array — that bug
        // meant this pass never produced a single `CoupledWith` relationship against
        // any real repository.
        let graph = read_knowledge_graph(&store);
        assert_eq!(graph.relationships.len(), 1, "a.rs and b.rs co-changed 3 times (>= min_coupling 2)");
        assert_eq!(graph.relationships[0].kind, RelationshipKind::CoupledWith);
        assert_eq!(graph.relationships[0].properties["co_change_count"], 3);
    }

    #[test]
    fn contributor_kir_id_is_stable() {
        let id1 = contributor_kir_id("Alice");
        let id2 = contributor_kir_id("Alice");
        let id3 = contributor_kir_id("Bob");
        assert_eq!(id1, id2);
        assert_ne!(id1, id3);
    }
}
