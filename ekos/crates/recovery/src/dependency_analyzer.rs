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
    /// (relative file path, full file content) pairs to scan.
    files: Vec<(String, String)>,
}

impl DependencyAnalyzerPass {
    pub fn new(workspace_name: impl Into<String>, files: Vec<(String, String)>) -> Self {
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
        let mut paths: Vec<&str> = self.files.iter().map(|(p, _)| p.as_str()).collect();
        paths.sort();
        for path in paths {
            let (_, content) = self.files.iter().find(|(p, _)| p == path).unwrap();
            hasher.update(path.as_bytes());
            hasher.update(content.as_bytes());
        }
        vec![hex::encode(hasher.finalize())]
    }

    async fn run(&mut self, ctx: &mut PassContext) -> Result<(), PassError> {
        let mut graph = KirGraph::new();
        let mut technology_ids: HashMap<&'static str, KirId> = HashMap::new();

        for (rel_path, content) in &self.files {
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

                let file_id = file_kir_id(rel_path);
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
        let (mut c, _dir) = ctx();
        let files = files
            .into_iter()
            .map(|(p, s)| (p.to_string(), s.to_string()))
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
    async fn unrecognized_file_emits_nothing() {
        let (mut c, _dir) = ctx();
        let mut pass = DependencyAnalyzerPass::new(
            "test",
            vec![(
                "readme.md".to_string(),
                "just some prose about the project\n".to_string(),
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
