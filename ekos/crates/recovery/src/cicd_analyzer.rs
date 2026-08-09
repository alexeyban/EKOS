//! `CicdAnalyzerPass` — structural extraction of CI/CD pipeline definitions from GitHub Actions
//! workflow files (`.github/workflows/*.yml`/`*.yaml`, RFC 0042). This is the "real deployment/
//! ops infrastructure" half of RFC 0042's gap: EKOS's own repo has no Docker/Kubernetes/Terraform
//! config to model (confirmed by the RFC's own investigation), but it does have real CI workflows
//! (`ci.yml`, `pages.yml`), and until this pass existed nothing compiled them into knowledge at
//! all.
//!
//! Deliberately structural, not a full GitHub Actions schema parser: each workflow file becomes
//! one `ObjectKind::Pipeline` object carrying its triggers and job/step names as plain JSON
//! properties — the same "read what's declared, don't build a typed model of the whole format"
//! spirit `crate_topology_analyzer.rs` and `dependency_analyzer.rs` already use.

use async_trait::async_trait;
use ekos_compiler_core::pass::{CompilerPass, PassContext, PassError};
use ekos_kir::{KirEvidence, KirGraph, KirId, KirObject, ObjectKind, SourceLocation};
use uuid::Uuid;

fn pipeline_kir_id(rel_path: &str) -> KirId {
    KirId(Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("cicd-pipeline:{rel_path}").as_bytes(),
    ))
}

/// Pulls a workflow's trigger names out of its `on:` key, which YAML lets authors write as a
/// bare string, a list of strings, or a map keyed by event name — all three shapes appear across
/// real-world workflows, so all three are handled rather than assuming one.
fn extract_triggers(doc: &serde_yaml::Value) -> Vec<String> {
    let on = match doc
        .get("on")
        .or_else(|| doc.get(serde_yaml::Value::Bool(true)))
    {
        // `on:` unquoted parses as the YAML boolean key `true` under YAML 1.1 rules — both keys
        // are checked so triggers aren't silently dropped depending on how the file quotes it.
        Some(v) => v,
        None => return Vec::new(),
    };
    match on {
        serde_yaml::Value::String(s) => vec![s.clone()],
        serde_yaml::Value::Sequence(seq) => seq
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect(),
        serde_yaml::Value::Mapping(map) => map
            .keys()
            .filter_map(|k| k.as_str().map(str::to_string))
            .collect(),
        _ => Vec::new(),
    }
}

/// Pulls each job's name and step list (`name:` when given, else `run:`/`uses:`, so a step is
/// never rendered as an empty string) out of the workflow's `jobs:` map.
fn extract_jobs(doc: &serde_yaml::Value) -> Vec<serde_json::Value> {
    let jobs = match doc.get("jobs").and_then(|v| v.as_mapping()) {
        Some(m) => m,
        None => return Vec::new(),
    };
    jobs.iter()
        .filter_map(|(job_key, job_val)| {
            let job_name = job_key.as_str()?.to_string();
            let steps: Vec<String> = job_val
                .get("steps")
                .and_then(|v| v.as_sequence())
                .map(|steps| {
                    steps
                        .iter()
                        .map(|step| {
                            step.get("name")
                                .and_then(|v| v.as_str())
                                .or_else(|| step.get("uses").and_then(|v| v.as_str()))
                                .or_else(|| step.get("run").and_then(|v| v.as_str()))
                                .unwrap_or("<unnamed step>")
                                .to_string()
                        })
                        .collect()
                })
                .unwrap_or_default();
            Some(serde_json::json!({ "name": job_name, "steps": steps }))
        })
        .collect()
}

pub struct CicdAnalyzerPass {
    pass_id: String,
    /// (relative workflow-file path, raw YAML content) pairs, one per discovered workflow.
    workflows: Vec<(String, String)>,
}

impl CicdAnalyzerPass {
    pub fn new(workspace_name: impl Into<String>, workflows: Vec<(String, String)>) -> Self {
        Self {
            pass_id: format!("cicd-analyzer:{}", workspace_name.into()),
            workflows,
        }
    }
}

#[async_trait]
impl CompilerPass for CicdAnalyzerPass {
    fn name(&self) -> &str {
        &self.pass_id
    }

    fn cache_inputs(&self) -> Vec<String> {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        let mut paths: Vec<&str> = self.workflows.iter().map(|(p, _)| p.as_str()).collect();
        paths.sort();
        for path in paths {
            let (_, content) = self.workflows.iter().find(|(p, _)| p == path).unwrap();
            hasher.update(path.as_bytes());
            hasher.update(content.as_bytes());
        }
        vec![hex::encode(hasher.finalize())]
    }

    async fn run(&mut self, ctx: &mut PassContext) -> Result<(), PassError> {
        let mut graph = KirGraph::new();

        for (rel_path, content) in &self.workflows {
            let doc: serde_yaml::Value = match serde_yaml::from_str(content) {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!("cannot parse {rel_path} as YAML: {e}");
                    continue;
                }
            };

            let name = doc
                .get("name")
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .unwrap_or_else(|| {
                    std::path::Path::new(rel_path)
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_else(|| rel_path.clone())
                });

            let triggers = extract_triggers(&doc);
            let jobs = extract_jobs(&doc);

            let mut obj = KirObject::new(name, ObjectKind::Pipeline)
                .with_property("path", serde_json::json!(rel_path))
                .with_property("triggers", serde_json::json!(triggers))
                .with_property("jobs", serde_json::json!(jobs));
            obj.id = pipeline_kir_id(rel_path);

            let ev = KirEvidence::new(
                SourceLocation::file(rel_path),
                format!("workflow definition at {rel_path}"),
            );
            obj.evidence.push(graph.add_evidence(ev));

            graph.add_object(obj);
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
            pipelines = knowledge.content.kir.objects.len(),
            "cicd-analyzer complete"
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

    async fn run_pass(workflows: Vec<(&str, &str)>) -> ekos_kir::KirGraph {
        let (mut c, _dir) = ctx();
        let workflows = workflows
            .into_iter()
            .map(|(p, s)| (p.to_string(), s.to_string()))
            .collect();
        let mut pass = CicdAnalyzerPass::new("test", workflows);
        pass.run(&mut c).await.unwrap();

        let ids = c.artifact_store.list().unwrap();
        assert_eq!(ids.len(), 1, "exactly one KnowledgeArtifact expected");
        let json = c.artifact_store.read(&ids[0]).unwrap().unwrap();
        let knowledge: ekos_artifact::KnowledgeArtifact = serde_json::from_value(json).unwrap();
        knowledge.content.kir
    }

    const CI_YML: &str = r#"
name: CI
on:
  push:
    branches: [main]
  pull_request: {}
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - name: Checkout
        uses: actions/checkout@v4
      - name: Run tests
        run: cargo test --workspace
"#;

    #[tokio::test]
    async fn parses_name_triggers_and_job_steps() {
        let graph = run_pass(vec![(".github/workflows/ci.yml", CI_YML)]).await;
        assert_eq!(graph.objects.len(), 1);
        let pipeline = &graph.objects[0];
        assert_eq!(pipeline.name, "CI");
        assert_eq!(pipeline.kind, ObjectKind::Pipeline);

        let triggers = pipeline.properties["triggers"].as_array().unwrap();
        let trigger_names: Vec<&str> = triggers.iter().map(|v| v.as_str().unwrap()).collect();
        assert!(trigger_names.contains(&"push"));
        assert!(trigger_names.contains(&"pull_request"));

        let jobs = pipeline.properties["jobs"].as_array().unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0]["name"], serde_json::json!("build"));
        let steps = jobs[0]["steps"].as_array().unwrap();
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0], serde_json::json!("Checkout"));
        assert_eq!(steps[1], serde_json::json!("Run tests"));
    }

    #[tokio::test]
    async fn missing_name_key_falls_back_to_file_stem() {
        const NO_NAME: &str = "on: push\njobs:\n  build:\n    steps: []\n";
        let graph = run_pass(vec![(".github/workflows/pages.yml", NO_NAME)]).await;
        assert_eq!(graph.objects[0].name, "pages");
    }

    #[tokio::test]
    async fn malformed_yaml_is_skipped_without_aborting_other_files() {
        const BROKEN: &str = "not: [valid yaml: {{{";
        const CI: &str = "name: CI\non: push\njobs:\n  build:\n    steps: []\n";
        let graph = run_pass(vec![
            (".github/workflows/broken.yml", BROKEN),
            (".github/workflows/ci.yml", CI),
        ])
        .await;
        assert_eq!(graph.objects.len(), 1);
        assert_eq!(graph.objects[0].name, "CI");
    }

    #[tokio::test]
    async fn no_workflows_emits_nothing() {
        let (mut c, _dir) = ctx();
        let mut pass = CicdAnalyzerPass::new("test", vec![]);
        pass.run(&mut c).await.unwrap();
        assert!(
            c.artifact_store.list().unwrap().is_empty(),
            "no KnowledgeArtifact should be written when there are no workflow files"
        );
    }
}
