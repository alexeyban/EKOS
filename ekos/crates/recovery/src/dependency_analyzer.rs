//! `DependencyAnalyzerPass` — pattern-based detection of known technology
//! dependencies (import statements, connection-string prefixes) across
//! source files, emitting `DependsOn` edges to synthetic Technology objects
//! (RFC 0019). Deliberately not an import-statement parser or AST walk: a
//! fixed table of literal substrings, matched case-insensitively — cheap,
//! transparent, and easy to extend by adding a row.

use std::collections::HashMap;

use async_trait::async_trait;
use ekos_compiler_core::pass::{CompilerPass, PassContext, PassError};
use ekos_kir::{
    KirEvidence, KirGraph, KirId, KirObject, KirRelationship, ObjectKind, RelationshipKind,
    SourceLocation,
};
use uuid::Uuid;

/// (pattern, technology name) rules, checked as case-insensitive substring
/// matches against a file's full content. Not exhaustive — a documented v1
/// limitation (RFC 0019): this answers "what obviously depends on X", not
/// "prove no dependency exists."
const PATTERNS: &[(&str, &str)] = &[
    // PostgreSQL
    ("postgres://", "PostgreSQL"),
    ("postgresql://", "PostgreSQL"),
    ("psycopg2", "PostgreSQL"),
    ("pg8000", "PostgreSQL"),
    ("org.postgresql", "PostgreSQL"),
    ("require('pg')", "PostgreSQL"),
    ("require(\"pg\")", "PostgreSQL"),
    ("from 'pg'", "PostgreSQL"),
    ("from \"pg\"", "PostgreSQL"),
    // MySQL
    ("mysql://", "MySQL"),
    ("pymysql", "MySQL"),
    ("mysql2", "MySQL"),
    ("com.mysql", "MySQL"),
    // MongoDB
    ("mongodb://", "MongoDB"),
    ("mongodb+srv://", "MongoDB"),
    ("pymongo", "MongoDB"),
    ("mongoose", "MongoDB"),
    // Redis
    ("redis://", "Redis"),
    ("ioredis", "Redis"),
    ("require('redis')", "Redis"),
    ("require(\"redis\")", "Redis"),
    ("import redis", "Redis"),
    // Kafka
    ("kafka-python", "Kafka"),
    ("org.apache.kafka", "Kafka"),
    ("kafkajs", "Kafka"),
    // OpenAI API (also covers Azure OpenAI — real Python usage imports the same `openai`
    // package's `AzureOpenAI`/`OpenAI` client classes for either backend). Found live,
    // 2026-08-24: a real project (`pdf-reader`'s `services/ai_service.py`) has this as its one
    // genuine C4 External System dependency (a real network-addressable API, unlike the
    // framework/ORM libraries this table deliberately doesn't cover), but `## Technology
    // Inventory` rendered empty for it — this table had no row for any AI-provider SDK at all.
    // Named "OpenAI API", not bare "OpenAI": the literal Python import (`import openai`) also
    // produces a real `PythonModule` object named `openai` — a bare-"OpenAI" Technology name
    // case-insensitively collides with it, and `ekos resolve` correctly refuses to silently
    // merge a `Technology` and a `PythonModule` sharing a normalized name (found live: this
    // exact real project hit the conflict on first use). Every other row in this table happens
    // to avoid the same risk only by chance (e.g. "Kafka" vs. `import kafka`'s real module name
    // would collide identically) — not fixed workspace-wide here, just avoided for this row.
    ("import openai", "OpenAI API"),
    ("from openai", "OpenAI API"),
];

/// Deterministic id for a Technology object, stable across passes and
/// `ekos recover` runs so the same technology detected in many files always
/// resolves to the same object (RFC 0019).
fn technology_kir_id(name: &str) -> KirId {
    KirId(Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("technology:{name}").as_bytes(),
    ))
}

/// Deterministic id for a file object — matches `build.rs`'s scheme exactly
/// so `DependsOn` edges land on the same object `ekos_search`/`ekos_impact`
/// already resolve.
fn file_kir_id(rel_path: &str) -> KirId {
    KirId(Uuid::new_v5(&Uuid::NAMESPACE_URL, rel_path.as_bytes()))
}

pub struct DependencyAnalyzerPass {
    pass_id: String,
    /// (relative file path — to its own `[observe] paths` entry, RFC 0079's own convention, not
    /// the workspace root — full file content, RFC 0079 project qualifier: empty string means
    /// "no qualification needed", matching `ekos_common::project::project_qualify`'s own
    /// `None`/`Some("")` equivalence).
    files: Vec<(String, String, String)>,
}

impl DependencyAnalyzerPass {
    pub fn new(workspace_name: impl Into<String>, files: Vec<(String, String, String)>) -> Self {
        Self {
            pass_id: format!("dependency-analyzer:{}", workspace_name.into()),
            files,
        }
    }
}

#[async_trait]
impl CompilerPass for DependencyAnalyzerPass {
    fn name(&self) -> &str {
        &self.pass_id
    }

    fn cache_inputs(&self) -> Vec<String> {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        let mut paths: Vec<&str> = self.files.iter().map(|(p, _, _)| p.as_str()).collect();
        paths.sort();
        for path in paths {
            let (_, content, _) = self.files.iter().find(|(p, _, _)| p == path).unwrap();
            hasher.update(path.as_bytes());
            hasher.update(content.as_bytes());
        }
        vec![hex::encode(hasher.finalize())]
    }

    async fn run(&mut self, ctx: &mut PassContext) -> Result<(), PassError> {
        let mut graph = KirGraph::new();
        let mut technology_ids: HashMap<&'static str, KirId> = HashMap::new();

        for (rel_path, content, project) in &self.files {
            let lower = content.to_lowercase();
            // A file may reference several technologies; dedup per file so
            // one file doesn't emit the same edge twice for two patterns
            // matching the same technology (e.g. both "postgres://" and
            // "psycopg2" in one file).
            let mut matched: Vec<&'static str> = Vec::new();
            for (pattern, technology) in PATTERNS {
                if lower.contains(&pattern.to_lowercase()) && !matched.contains(technology) {
                    matched.push(technology);
                }
            }

            for technology in matched {
                let tech_id = *technology_ids
                    .entry(technology)
                    .or_insert_with(|| technology_kir_id(technology));
                if !graph.objects.iter().any(|o| o.id == tech_id) {
                    let mut tech_obj =
                        KirObject::new(technology, ObjectKind::Custom("Technology".to_string()));
                    tech_obj.id = tech_id;
                    graph.objects.push(tech_obj);
                }

                let ev = KirEvidence::new(
                    SourceLocation::file(rel_path),
                    format!("{rel_path} references {technology}"),
                );
                let ev_id = graph.add_evidence(ev);

                // RFC 0079: the hash input is project-qualified (`"{project}:{rel_path}"`) so
                // this lands on the exact same `File` object id `build.rs` already wrote — the
                // *displayed* `rel_path` above (evidence text, `SourceLocation`) stays
                // unqualified and human-readable, per that RFC's own stated principle. Found
                // live, 2026-08-24: every edge here used to hash the bare `rel_path` with no
                // qualification at all, silently pointing at a `File` id that only ever existed
                // in a single-project (`paths = ["."]`) workspace.
                let qualified = ekos_common::project::project_qualify(
                    rel_path,
                    if project.is_empty() {
                        None
                    } else {
                        Some(project.as_str())
                    },
                );
                let file_id = file_kir_id(&qualified);
                let mut rel = KirRelationship::new(RelationshipKind::DependsOn, file_id, tech_id);
                rel.evidence.push(ev_id);
                graph.relationships.push(rel);
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
            technologies = knowledge.content.kir.objects.len(),
            edges = knowledge.content.kir.relationships.len(),
            "dependency-analyzer complete"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_compiler_core::{EkosConfig, pass::PassContext};
    use std::sync::Arc;

    fn ctx() -> (PassContext, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        (
            PassContext::new(Arc::new(EkosConfig::default()), dir.path().to_path_buf()),
            dir,
        )
    }

    async fn run_pass(files: Vec<(&str, &str)>) -> ekos_kir::KirGraph {
        run_pass_qualified(files.into_iter().map(|(p, s)| (p, s, "")).collect()).await
    }

    async fn run_pass_qualified(files: Vec<(&str, &str, &str)>) -> ekos_kir::KirGraph {
        let (mut c, _dir) = ctx();
        let files = files
            .into_iter()
            .map(|(p, s, proj)| (p.to_string(), s.to_string(), proj.to_string()))
            .collect();
        let mut pass = DependencyAnalyzerPass::new("test", files);
        pass.run(&mut c).await.unwrap();

        // Read back the single KnowledgeArtifact this pass wrote.
        let ids = c.artifact_store.list().unwrap();
        assert_eq!(ids.len(), 1, "exactly one KnowledgeArtifact expected");
        let json = c.artifact_store.read(&ids[0]).unwrap().unwrap();
        let knowledge: ekos_artifact::KnowledgeArtifact = serde_json::from_value(json).unwrap();
        knowledge.content.kir
    }

    #[tokio::test]
    async fn python_postgres_import_emits_depends_on() {
        let graph = run_pass(vec![("app/db.py", "import psycopg2\n")]).await;
        assert_eq!(graph.objects.len(), 1);
        assert_eq!(graph.objects[0].name, "PostgreSQL");
        assert_eq!(graph.relationships.len(), 1);
        assert_eq!(graph.relationships[0].kind, RelationshipKind::DependsOn);
        assert_eq!(graph.relationships[0].to, graph.objects[0].id);
        assert_eq!(graph.relationships[0].from, file_kir_id("app/db.py"));
    }

    #[tokio::test]
    async fn javascript_connection_string_emits_depends_on() {
        let graph = run_pass(vec![("db.js", "const url = 'postgres://localhost/app';\n")]).await;
        assert_eq!(graph.objects.len(), 1);
        assert_eq!(graph.objects[0].name, "PostgreSQL");
    }

    #[tokio::test]
    async fn java_import_emits_depends_on() {
        let graph = run_pass(vec![("Db.java", "import org.postgresql.Driver;\n")]).await;
        assert_eq!(graph.objects.len(), 1);
        assert_eq!(graph.objects[0].name, "PostgreSQL");
    }

    #[tokio::test]
    async fn python_openai_import_emits_depends_on() {
        // Real regression: `services/ai_service.py`'s real
        // `from openai import AzureOpenAI, OpenAI` produced nothing here before this pattern
        // existed, leaving `## Technology Inventory` empty despite a real external API dependency.
        let graph = run_pass(vec![(
            "services/ai_service.py",
            "from openai import AzureOpenAI, OpenAI\n",
        )])
        .await;
        assert_eq!(graph.objects.len(), 1);
        // "OpenAI API", not bare "OpenAI" — avoids a real identity conflict with the
        // `PythonModule` named `openai` the same import produces elsewhere (`python_analyzer.rs`),
        // found live when `ekos resolve` refused to silently merge across kinds.
        assert_eq!(graph.objects[0].name, "OpenAI API");
    }

    #[tokio::test]
    async fn project_qualified_edge_lands_on_the_same_file_id_build_rs_writes() {
        // Real bug, found live 2026-08-24 against a real project (`pdf-reader`, `[observe] paths
        // = ["backend/app/api"]`): this pass used to hash the bare `rel_path` with no project
        // qualification at all, so its `DependsOn` edges pointed at a `File` id that never
        // matched the real, project-qualified id `build.rs` actually writes for a single
        // non-`"."` `[observe] paths` entry — `SEM002: unknown from-id` on every edge, and
        // `## Technology Inventory` could never resolve which file used a detected technology.
        let graph = run_pass_qualified(vec![(
            "ai.py",
            "from openai import AzureOpenAI, OpenAI\n",
            "backend/app/api",
        )])
        .await;
        assert_eq!(graph.relationships.len(), 1);
        // Matches `ekos_common::project::project_qualify`'s own convention exactly, the same one
        // `build.rs` uses for its real `File`-object ids.
        let expected_file_id = file_kir_id(&ekos_common::project::project_qualify(
            "ai.py",
            Some("backend/app/api"),
        ));
        assert_eq!(graph.relationships[0].from, expected_file_id);
        // Never accidentally the unqualified id — the exact failure mode this regresses against.
        assert_ne!(graph.relationships[0].from, file_kir_id("ai.py"));
    }

    #[tokio::test]
    async fn unrecognized_file_emits_nothing() {
        let (mut c, _dir) = ctx();
        let mut pass = DependencyAnalyzerPass::new(
            "test",
            vec![(
                "readme.md".to_string(),
                "just some prose about the project\n".to_string(),
                String::new(),
            )],
        );
        pass.run(&mut c).await.unwrap();
        assert!(
            c.artifact_store.list().unwrap().is_empty(),
            "no KnowledgeArtifact should be written when nothing matches"
        );
    }

    #[tokio::test]
    async fn same_technology_across_files_dedupes_to_one_object() {
        let graph = run_pass(vec![
            ("a.py", "import psycopg2\n"),
            ("b.py", "import psycopg2\n"),
        ])
        .await;
        assert_eq!(graph.objects.len(), 1, "PostgreSQL object must be shared");
        assert_eq!(
            graph.relationships.len(),
            2,
            "one edge per referencing file"
        );
    }

    #[tokio::test]
    async fn distinct_technologies_produce_distinct_objects() {
        let graph = run_pass(vec![
            ("a.py", "import psycopg2\n"),
            ("b.py", "import redis\n"),
        ])
        .await;
        assert_eq!(graph.objects.len(), 2);
        let names: Vec<&str> = graph.objects.iter().map(|o| o.name.as_str()).collect();
        assert!(names.contains(&"PostgreSQL"));
        assert!(names.contains(&"Redis"));
    }
}

#[cfg(test)]
mod scratch_id_check {
    #[test]
    fn print_candidate_ids() {
        let candidates = vec![
            "ai.py",
            "backend/app/api/ai.py",
            "app/api/ai.py",
            "api/ai.py",
            "./backend/app/api/ai.py",
        ];
        for c in candidates {
            let id = uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, c.as_bytes());
            println!("{c:50} -> {id}");
        }
    }
}
