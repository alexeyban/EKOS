//! `GitAnalyzerPass` — converts git commit observation artifacts into KIR.
//!
//! Produces:
//! - `KirObject(kind=Person)` per unique contributor
//! - `KirEvent(kind=Modified)` per commit
//! - `KirRelationship(kind=CoupledWith)` for files changed together in ≥2 commits
//! - RFC 0219: `KirObject(kind=Custom("Ticket"))` per ticket id its commit messages name (for the
//!   configured `[tickets] keys`), with `Touches` edges to the files those commits changed

use std::collections::{BTreeMap, BTreeSet, HashMap};

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

/// Commits touching more than this many files are excluded from co-change
/// coupling analysis (they still produce their `KirEvent` and authorship
/// relationship). Bulk commits — vendoring, formatting sweeps, initial
/// imports — carry no meaningful coupling signal, and the pair count is
/// quadratic in files-per-commit: one 3,500-file import alone yields ~6M
/// pairs. First hit for real scanning a 44-project workspace (devlog 14).
const DEFAULT_MAX_COUPLING_COMMIT_FILES: usize = 50;

/// How many commit SHAs a `CoupledWith` edge's evidence names as examples.
const COUPLING_EVIDENCE_SHAS: usize = 3;

/// RFC 0219: the custom kind of a ticket.
pub const TICKET_KIND: &str = "Ticket";

/// Commits listed on one ticket, newest first.
const TICKET_COMMITS: usize = 50;

/// RFC 0219: the stable id of a ticket object.
pub fn ticket_kir_id(key: &str) -> KirId {
    KirId(Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("ticket:{key}").as_bytes(),
    ))
}

/// What the commits say about one ticket.
#[derive(Default)]
struct TicketCommits {
    /// (date, sha, author, subject)
    commits: Vec<(String, String, String, String)>,
    files: BTreeSet<String>,
    /// Commits too large to link their files (the coupling limit).
    bulk_commits: usize,
}

pub struct GitAnalyzerPass {
    pass_id: String,
    /// Git commit ObservationArtifact IDs to process.
    commit_artifact_ids: Vec<ArtifactId>,
    /// Artifact IDs that are the repository metadata artifacts.
    repo_artifact_id: Option<ArtifactId>,
    min_coupling: usize,
    max_coupling_commit_files: usize,
    /// RFC 0219: `[tickets] keys`.
    ticket_keys: Vec<String>,
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
            max_coupling_commit_files: DEFAULT_MAX_COUPLING_COMMIT_FILES,
            ticket_keys: Vec::new(),
        }
    }

    /// RFC 0219: add one commit to every ticket its message names. Files are linked only for a
    /// commit within the coupling limit (a sweep says nothing about the ticket).
    fn record_tickets(
        &self,
        tickets: &mut BTreeMap<String, TicketCommits>,
        (sha, author, date, message): (&str, &str, &str, &str),
        files_changed: &[String],
        project: Option<&str>,
    ) {
        let subject = message.lines().next().unwrap_or_default().trim();
        for key in ekos_common::tickets::find(message, &self.ticket_keys) {
            let t = tickets.entry(key).or_default();
            t.commits.push((
                date.to_string(),
                sha.to_string(),
                author.to_string(),
                subject.to_string(),
            ));
            if files_changed.len() <= self.max_coupling_commit_files {
                t.files.extend(
                    files_changed
                        .iter()
                        .map(|p| ekos_common::project::project_qualify(p, project)),
                );
            } else {
                t.bulk_commits += 1;
            }
        }
    }

    /// RFC 0219: the issue-tracker keys whose ticket ids become `Ticket` objects.
    pub fn with_ticket_keys(mut self, keys: Vec<String>) -> Self {
        self.ticket_keys = keys;
        self
    }

    pub fn with_min_coupling(mut self, n: usize) -> Self {
        self.min_coupling = n;
        self
    }

    pub fn with_max_coupling_commit_files(mut self, n: usize) -> Self {
        self.max_coupling_commit_files = n;
        self
    }
}

#[async_trait]
impl CompilerPass for GitAnalyzerPass {
    fn name(&self) -> &str {
        &self.pass_id
    }

    // v2: bulk commits excluded from coupling analysis — bumping the version
    // invalidates cached v1 outputs so the fix actually re-runs.
    // v3: evidence on the `OwnedBy` and `CoupledWith` edges.
    // v4: RFC 0219 `Ticket` objects; output supersedes earlier runs.
    fn version(&self) -> &str {
        "v4"
    }

    fn cache_inputs(&self) -> Vec<String> {
        let mut ids: Vec<String> = self
            .commit_artifact_ids
            .iter()
            .map(std::string::ToString::to_string)
            .collect();
        ids.sort();
        if let Some(repo_id) = &self.repo_artifact_id {
            ids.push(repo_id.to_string());
        }
        ids.push(format!("tickets:{}", self.ticket_keys.join(",")));
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
                    let mut obj = KirObject::new(name, ObjectKind::Person).with_evidence(ev_id);
                    obj.id = id;
                    obj.properties
                        .insert("role".into(), serde_json::json!("contributor"));
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
        // Per file pair: how many commits changed both, and the first few of their SHAs (what
        // the `CoupledWith` edge's evidence cites).
        let mut file_co_changes: HashMap<(String, String), (usize, Vec<String>)> = HashMap::new();
        let mut tickets: BTreeMap<String, TicketCommits> = BTreeMap::new();

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
                    ctx.diagnostics.lock().unwrap().warning(
                        "GIT002",
                        format!("failed to read artifact {artifact_id}: {e}"),
                    );
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
            let author = data["author_name"]
                .as_str()
                .unwrap_or("unknown")
                .to_string();
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

            // RFC 0219: the tickets this commit names.
            self.record_tickets(
                &mut tickets,
                (&sha, &author, &date, &message),
                &files_changed,
                data["project"].as_str(),
            );

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

            // Count co-changes for coupling analysis. Bulk commits are
            // excluded: the pair count is quadratic in files-per-commit, and
            // a vendoring/formatting commit says nothing about coupling.
            if files_changed.len() <= self.max_coupling_commit_files {
                // RFC 0079: `id_a`/`id_b` below hash these path strings directly — qualify them
                // here (this commit's own `data["project"]`, present only in a multi-`[observe]
                // paths` workspace, `build.rs`'s own choke point) so `CoupledWith` lands on the
                // same collision-safe ids `build.rs`'s own `File` objects use. The event payload
                // above already captured the unqualified `files_changed` for display — this is a
                // separate copy, not a mutation of it.
                let project = data["project"].as_str();
                let sorted_files: Vec<String> = {
                    let mut f: Vec<String> = files_changed
                        .iter()
                        .map(|p| ekos_common::project::project_qualify(p, project))
                        .collect();
                    f.sort();
                    f
                };
                for i in 0..sorted_files.len() {
                    for j in (i + 1)..sorted_files.len() {
                        let pair = (sorted_files[i].clone(), sorted_files[j].clone());
                        let (count, shas) = file_co_changes.entry(pair).or_default();
                        *count += 1;
                        if shas.len() < COUPLING_EVIDENCE_SHAS {
                            shas.push(sha.clone());
                        }
                    }
                }
            } else {
                tracing::debug!(
                    sha = %sha,
                    files = files_changed.len(),
                    "skipping bulk commit for coupling analysis"
                );
            }

            // Authorship relationship: contributor → commit event.
            if let Some(&contrib_id) = contributor_ids.get(&author) {
                let mut rel = KirRelationship::deterministic(
                    RelationshipKind::OwnedBy,
                    subject_id,
                    contrib_id,
                    "",
                );
                // The commit itself is what says who authored it.
                rel.evidence.push(ev_id);
                graph.add_relationship(rel);
            }
        }

        // ── Emit CoupledWith relationships ────────────────────────────────────
        for ((file_a, file_b), (count, shas)) in &file_co_changes {
            if *count < self.min_coupling {
                continue;
            }
            // Without evidence `ekos coverage` attributed these edges to no source kind and
            // reported Git history as "0 relationships" (found live 2026-10-13).
            let ev_id = graph.add_evidence(KirEvidence::new(
                SourceLocation::file("git:co-changes"),
                format!(
                    "{file_a} and {file_b} changed together in {count} commit(s), e.g. {}",
                    shas.join(", ")
                ),
            ));
            let id_a = KirId(Uuid::new_v5(&Uuid::NAMESPACE_URL, file_a.as_bytes()));
            let id_b = KirId(Uuid::new_v5(&Uuid::NAMESPACE_URL, file_b.as_bytes()));
            let mut rel =
                KirRelationship::deterministic(RelationshipKind::CoupledWith, id_a, id_b, "");
            rel.properties
                .insert("co_change_count".into(), serde_json::json!(count));
            rel.evidence.push(ev_id);
            graph.add_relationship(rel);
        }

        let ticket_count = add_tickets(&mut graph, tickets);

        // ── Write KnowledgeArtifact ─────────────────────────────────────────
        // Every run reads every commit, so its output replaces the earlier runs' (RFC 0211):
        // otherwise a ticket from an older run, with fewer commits, could win the merge.
        let knowledge = ekos_artifact::KnowledgeArtifact::new(
            &self.pass_id,
            self.commit_artifact_ids.clone(),
            graph,
        )
        .superseding_earlier_runs();
        let json = serde_json::to_value(&knowledge)
            .map_err(|e| PassError::failed(format!("serialize: {e}")))?;
        ctx.artifact_store
            .write(&knowledge.id, &json)
            .map_err(|e| PassError::failed(format!("write artifact: {e}")))?;

        tracing::info!(
            pass = %self.pass_id,
            events = knowledge.content.kir.events.len(),
            contributors = contributor_ids.len(),
            tickets = ticket_count,
            "git-analyzer complete"
        );
        ekos_common::run_log::emit(
            "git.history",
            "commits read",
            &serde_json::json!({
                "pass": self.pass_id,
                "commits": knowledge.content.kir.events.len(),
                "contributors": contributor_ids.len(),
                "tickets": ticket_count,
            }),
        );
        Ok(())
    }
}

/// RFC 0219: every ticket's object and edges; how many tickets.
fn add_tickets(graph: &mut KirGraph, tickets: BTreeMap<String, TicketCommits>) -> usize {
    let n = tickets.len();
    for (key, t) in tickets {
        add_ticket(graph, &key, t);
    }
    n
}

/// RFC 0219: one `Ticket` object, its commits newest first, and `Touches` edges to its files.
fn add_ticket(graph: &mut KirGraph, key: &str, mut t: TicketCommits) {
    t.commits.sort_by(|a, b| (&b.0, &b.1).cmp(&(&a.0, &a.1)));
    let first = t.commits.last().cloned().unwrap_or_default();
    let last = t.commits.first().cloned().unwrap_or_default();
    // The earliest commit's subject, without the key: the commit says it, not the tracker.
    let title = first
        .3
        .replacen(key, "", 1)
        .trim_matches(|c: char| c.is_whitespace() || matches!(c, ':' | '-' | '#' | '[' | ']'))
        .to_string();
    let ev = graph.add_evidence(KirEvidence::new(
        SourceLocation::file(format!("git:commit:{}", first.1)),
        format!("{}: {}", first.1, first.3),
    ));
    let listed: Vec<serde_json::Value> = t
        .commits
        .iter()
        .take(TICKET_COMMITS)
        .map(|(date, sha, author, subject)| {
            serde_json::json!({ "sha": sha, "date": date, "author": author, "subject": subject })
        })
        .collect();
    let mut o = KirObject::new(key, ObjectKind::Custom(TICKET_KIND.into())).with_evidence(ev);
    o.id = ticket_kir_id(key);
    for (k, v) in [
        ("key", serde_json::json!(key)),
        ("title", serde_json::json!(title)),
        ("title_source", serde_json::json!("first_commit_subject")),
        ("commit_count", serde_json::json!(t.commits.len())),
        ("first_commit_at", serde_json::json!(first.0)),
        ("last_commit_at", serde_json::json!(last.0)),
        ("commits", serde_json::json!(listed)),
        ("files_touched", serde_json::json!(t.files.len())),
        ("bulk_commits", serde_json::json!(t.bulk_commits)),
    ] {
        o.properties.insert(k.into(), v);
    }
    graph.objects.push(o);
    for f in &t.files {
        let mut rel = KirRelationship::deterministic(
            RelationshipKind::Custom("Touches".into()),
            ticket_kir_id(key),
            KirId(Uuid::new_v5(&Uuid::NAMESPACE_URL, f.as_bytes())),
            "",
        );
        rel.evidence.push(ev);
        graph.add_relationship(rel);
    }
}

/// Stable UUIDv5 for a contributor, so the same name always maps to the same KirId.
fn contributor_kir_id(name: &str) -> KirId {
    KirId(Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("contributor:{name}").as_bytes(),
    ))
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

    fn seed_artifact(
        store: &FileSystemArtifactStore,
        artifact: &ObservationArtifact,
    ) -> ArtifactId {
        let json = serde_json::to_value(artifact).unwrap();
        store.write(&artifact.id, &json).unwrap();
        artifact.id.clone()
    }

    /// Read back the single `KnowledgeArtifact` this pass wrote and decode its
    /// `KirGraph`. Must read through the same store the pass wrote to — the
    /// pass context's store is a pack store (RFC 0015), and its segments are
    /// not visible to a plain loose-file store.
    fn read_knowledge_graph(store: &dyn ArtifactStore) -> KirGraph {
        for id in store.list().unwrap() {
            let json = store.read(&id).unwrap().unwrap();
            if json["artifact_type"] == "knowledge" {
                return serde_json::from_value(json["kir"].clone()).unwrap();
            }
        }
        panic!("no knowledge artifact found in store");
    }

    fn make_repo_artifact(contributors: &[(&str, u64)]) -> ObservationArtifact {
        let contributors: Vec<serde_json::Value> = contributors
            .iter()
            .map(|(name, commits)| serde_json::json!({"name": name, "commits": commits}))
            .collect();
        ObservationArtifact::new(
            "git",
            "repo",
            serde_json::json!({ "contributors": contributors }),
        )
    }

    /// Regression (devlog 14): a bulk commit must not contribute to co-change
    /// coupling — the pair count is quadratic in files-per-commit, and one
    /// real 3,500-file import produced ~6M `CoupledWith` relationships.
    #[tokio::test]
    async fn bulk_commits_are_excluded_from_coupling() {
        let dir = TempDir::new().unwrap();
        let ctx = make_ctx_with_store(&dir);
        let store = FileSystemArtifactStore::new(dir.path().join(".ekos/artifacts"));

        // Two identical bulk commits (11 files > threshold of 10): every pair
        // co-changes twice, which would pass min_coupling if it were counted.
        let bulk_files: Vec<String> = (0..11).map(|i| format!("src/file_{i}.rs")).collect();
        let bulk_refs: Vec<&str> = bulk_files.iter().map(String::as_str).collect();
        // Two small commits sharing the same two files: real coupling signal.
        let ids = vec![
            seed_artifact(&store, &make_commit_artifact("aaa", "alice", &bulk_refs)),
            seed_artifact(&store, &make_commit_artifact("bbb", "alice", &bulk_refs)),
            seed_artifact(
                &store,
                &make_commit_artifact("ccc", "alice", &["a.rs", "b.rs"]),
            ),
            seed_artifact(
                &store,
                &make_commit_artifact("ddd", "alice", &["a.rs", "b.rs"]),
            ),
        ];

        let mut pass = GitAnalyzerPass::new("ws", ids, None).with_max_coupling_commit_files(10);
        let mut ctx = ctx;
        pass.run(&mut ctx).await.unwrap();

        let graph = read_knowledge_graph(ctx.artifact_store.as_ref());
        let coupled: Vec<_> = graph
            .relationships
            .iter()
            .filter(|r| r.kind == RelationshipKind::CoupledWith)
            .collect();
        // Only the small commits' single pair survives; the bulk commits'
        // 55 pairs are excluded. All four commits still produced events.
        assert_eq!(coupled.len(), 1, "only the small-commit pair may couple");
        assert_eq!(graph.events.len(), 4, "bulk commits still produce events");
    }

    /// RFC 0219: one `Ticket` per configured key a message names, commits newest first, the
    /// earliest commit's subject as its title, `Touches` to the files of non-bulk commits.
    #[tokio::test]
    async fn commits_naming_a_ticket_become_one_ticket_object() {
        let dir = TempDir::new().unwrap();
        let ctx = make_ctx_with_store(&dir);
        let store = FileSystemArtifactStore::new(dir.path().join(".ekos/artifacts"));
        let commit = |sha: &str, date: &str, msg: &str, files: &[&str]| {
            ObservationArtifact::new(
                "git",
                sha,
                serde_json::json!({ "sha": sha, "author_name": "alice", "date": date,
                                    "message": msg, "files_changed": files }),
            )
        };
        let big: Vec<String> = (0..11).map(|i| format!("f{i}.java")).collect();
        let big: Vec<&str> = big.iter().map(String::as_str).collect();
        let ids = vec![
            seed_artifact(
                &store,
                &commit(
                    "a1",
                    "2024-08-05T10:00:00Z",
                    "IDEMPIERE-6203 Back-date costing\n\nlong text",
                    &["X_M_CostDetail.java"],
                ),
            ),
            seed_artifact(
                &store,
                &commit(
                    "b2",
                    "2024-09-03T10:00:00Z",
                    "IDEMPIERE-6203 follow-up, see UTF-8 note",
                    &["MCost.java"],
                ),
            ),
            seed_artifact(
                &store,
                &commit("c3", "2024-10-01T10:00:00Z", "IDEMPIERE-6203 sweep", &big),
            ),
            seed_artifact(
                &store,
                &commit("d4", "2024-10-02T10:00:00Z", "no ticket here", &["a.java"]),
            ),
        ];
        let mut pass = GitAnalyzerPass::new("ws", ids, None)
            .with_max_coupling_commit_files(10)
            .with_ticket_keys(vec!["IDEMPIERE".into()]);
        let mut ctx = ctx;
        pass.run(&mut ctx).await.unwrap();
        let graph = read_knowledge_graph(ctx.artifact_store.as_ref());
        let tickets: Vec<&KirObject> = graph
            .objects
            .iter()
            .filter(|o| matches!(&o.kind, ObjectKind::Custom(k) if k == TICKET_KIND))
            .collect();
        assert_eq!(tickets.len(), 1, "UTF-8 is not a configured key");
        let t = tickets[0];
        assert_eq!(t.id, ticket_kir_id("IDEMPIERE-6203"));
        assert_eq!(t.properties["commit_count"], 3);
        assert_eq!(t.properties["title"], "Back-date costing");
        assert_eq!(t.properties["first_commit_at"], "2024-08-05T10:00:00Z");
        assert_eq!(t.properties["commits"][0]["sha"], "c3", "newest first");
        assert_eq!(t.properties["bulk_commits"], 1);
        let touches = graph
            .relationships
            .iter()
            .filter(|r| matches!(&r.kind, RelationshipKind::Custom(k) if k == "Touches"))
            .count();
        assert_eq!(touches, 2, "the sweep's files are not linked");
    }

    #[tokio::test]
    async fn contributors_are_classified_as_person() {
        // AD-001: contributors were reclassified from ObjectKind::Entity to the
        // dedicated ObjectKind::Person variant.
        let dir = TempDir::new().unwrap();
        let store = FileSystemArtifactStore::new(dir.path().join(".ekos/artifacts"));
        std::fs::create_dir_all(dir.path().join(".ekos/artifacts")).unwrap();

        let repo = make_repo_artifact(&[("Alice", 5)]);
        let repo_id = seed_artifact(&store, &repo);
        let a1 = make_commit_artifact("sha1abc", "Alice", &["main.rs"]);
        let id1 = seed_artifact(&store, &a1);

        let mut pass = GitAnalyzerPass::new("repo", vec![id1], Some(repo_id));
        let mut ctx = make_ctx_with_store(&dir);
        pass.run(&mut ctx).await.unwrap();

        let graph = read_knowledge_graph(ctx.artifact_store.as_ref());
        let alice = graph
            .objects
            .iter()
            .find(|o| o.name == "Alice")
            .expect("Alice must be present");
        assert_eq!(alice.kind, ObjectKind::Person);
        assert_eq!(alice.properties["role"], "contributor");
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

        let mut pass = GitAnalyzerPass::new("test-repo", vec![id1, id2], None).with_min_coupling(1);
        let mut ctx = make_ctx_with_store(&dir);
        pass.run(&mut ctx).await.unwrap();

        assert!(!ctx.diagnostics.lock().unwrap().has_errors());

        // Assert the actual extracted values, not just "no error" — commit metadata
        // must be read from the real `sha`/`author_name` fields, not silently default
        // to "unknown".
        let graph = read_knowledge_graph(ctx.artifact_store.as_ref());
        assert_eq!(graph.events.len(), 2, "one event per commit artifact");
        let shas: Vec<String> = graph
            .events
            .iter()
            .map(|e| e.payload["sha"].as_str().unwrap().to_string())
            .collect();
        assert!(shas.contains(&"sha1abc".to_string()));
        assert!(shas.contains(&"sha2def".to_string()));
        assert!(
            graph
                .events
                .iter()
                .all(|e| e.payload["author"] != "unknown"),
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
        let graph = read_knowledge_graph(ctx.artifact_store.as_ref());
        assert_eq!(
            graph.relationships.len(),
            1,
            "a.rs and b.rs co-changed 3 times (>= min_coupling 2)"
        );
        assert_eq!(graph.relationships[0].kind, RelationshipKind::CoupledWith);
        assert_eq!(graph.relationships[0].properties["co_change_count"], 3);
    }

    /// Regression (2026-10-13): both edge kinds were emitted with no evidence, so `ekos coverage`
    /// counted Git history as "0 relationships" and neither edge could be cited.
    #[tokio::test]
    async fn ownership_and_coupling_edges_carry_git_evidence() {
        let dir = TempDir::new().unwrap();
        let store = FileSystemArtifactStore::new(dir.path().join(".ekos/artifacts"));
        std::fs::create_dir_all(dir.path().join(".ekos/artifacts")).unwrap();

        let repo_id = seed_artifact(&store, &make_repo_artifact(&[("Alice", 2)]));
        let mut ids = vec![];
        for i in 0u32..2 {
            let a = make_commit_artifact(&format!("sha{i}"), "Alice", &["a.rs", "b.rs"]);
            ids.push(seed_artifact(&store, &a));
        }

        let mut pass = GitAnalyzerPass::new("repo", ids, Some(repo_id)).with_min_coupling(2);
        let mut ctx = make_ctx_with_store(&dir);
        pass.run(&mut ctx).await.unwrap();

        let graph = read_knowledge_graph(ctx.artifact_store.as_ref());
        let evidence_of = |rel: &KirRelationship| {
            assert_eq!(rel.evidence.len(), 1, "{:?} edge has no evidence", rel.kind);
            graph
                .evidence
                .iter()
                .find(|e| e.id == rel.evidence[0])
                .expect("the edge's evidence is on the graph")
        };

        let owned: Vec<&KirRelationship> = graph
            .relationships
            .iter()
            .filter(|r| r.kind == RelationshipKind::OwnedBy)
            .collect();
        assert_eq!(owned.len(), 2);
        for rel in owned {
            assert!(
                evidence_of(rel).location.path.starts_with("git:commit:sha"),
                "authorship is proven by the commit"
            );
        }

        let coupled = graph
            .relationships
            .iter()
            .find(|r| r.kind == RelationshipKind::CoupledWith)
            .expect("a.rs and b.rs co-changed twice");
        let ev = evidence_of(coupled);
        assert_eq!(ev.location.path, "git:co-changes");
        assert!(
            ev.fragment
                .contains("a.rs and b.rs changed together in 2 commit(s)")
        );
        assert!(ev.fragment.contains("sha0") && ev.fragment.contains("sha1"));
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
