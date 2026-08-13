//! `world.sources` document ingestion (RFC 0055, source document §15).
//! Reuses the real `ekos-plugin-localdocs` connector and
//! `LocalDocAnalyzerPass` (deterministic, no LLM) rather than reimplementing
//! document parsing — the exact "Observer → recovery → KIR" shape the main
//! compiler pipeline already uses, scoped to exactly one connector and one
//! pass, not the full multi-source, incrementally-cached machinery
//! `ekos build`/`ekos recover` run over a whole workspace.

use ekos_artifact::{ArtifactStore, ArtifactType, FileSystemArtifactStore, KnowledgeArtifact};
use ekos_common::redaction::{self, RedactionConfig};
use ekos_compiler_core::{
    EkosConfig,
    pass::{CompilerPass, PassContext, PassError},
};
use ekos_kir::KirObject;
use ekos_ledger::{KnowledgeStore, LedgerError};
use ekos_observation_sdk::{Observer, ScanContext};
use ekos_plugin_localdocs::{LocalDocsObserver, TesseractOcr};
use ekos_recovery::LocalDocAnalyzerPass;
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum IngestError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("cannot scan {0}: {1}")]
    Observe(String, String),
    #[error(
        "world.sources entry '{0}' was not found (or has an unsupported extension) under the scenario directory"
    )]
    SourceNotFound(String),
    #[error("document analysis pass failed: {0}")]
    Pass(String),
    #[error("artifact store error: {0}")]
    Store(#[from] ekos_artifact::StoreError),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("ledger error: {0}")]
    Ledger(#[from] LedgerError),
}

/// Ingests `sources` (paths relative to `scenario_dir`, matching `agents:`'s
/// own convention) and appends every resulting `Document`/`Section`/`Table`
/// object — plus their evidence and `Contains` relationships — directly to
/// `store`. Returns the objects, for the caller to register their names in
/// the scenario's own name registry (`load_scenario`).
///
/// Bridges into async territory internally (`LocalDocsObserver::scan` and
/// `CompilerPass::run` are both `async fn`) — see the RFC's Alternatives
/// Considered for why `load_scenario`/`load_scenario_from_path` stay
/// synchronous rather than propagating `async` through their entire
/// existing call graph for a capability only exercised when `world.sources`
/// is non-empty. Two call shapes are both real: a plain `#[test]` function
/// (no Tokio runtime active) and `ekos simulate`'s own `#[tokio::main]`
/// entry point (a runtime already driving the current thread) — calling
/// `Runtime::block_on` from inside an already-running runtime panics
/// ("Cannot start a runtime from within a runtime"), confirmed live against
/// the real CLI before this fix, not assumed. `Handle::try_current` picks
/// the right bridge for whichever context actually called this.
pub(crate) fn ingest_sources(
    store: &dyn KnowledgeStore,
    scenario_dir: &Path,
    sources: &[String],
) -> Result<Vec<KirObject>, IngestError> {
    if sources.is_empty() {
        return Ok(Vec::new());
    }
    match tokio::runtime::Handle::try_current() {
        // Already inside a (multi-thread, per #[tokio::main]'s default
        // flavor) runtime — block_in_place hands this thread's async work
        // to another worker instead of trying to nest a second runtime.
        Ok(handle) => tokio::task::block_in_place(|| {
            handle.block_on(ingest_sources_async(store, scenario_dir, sources))
        }),
        // No runtime active (e.g. a plain #[test] function) — spin up a
        // throwaway one for just this call.
        Err(_) => {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            runtime.block_on(ingest_sources_async(store, scenario_dir, sources))
        }
    }
}

async fn ingest_sources_async(
    store: &dyn KnowledgeStore,
    scenario_dir: &Path,
    sources: &[String],
) -> Result<Vec<KirObject>, IngestError> {
    // One scan of the whole scenario directory, not one per source:
    // LocalDocsObserver::scan computes each artifact's path via
    // `abs_path.strip_prefix(root)`, so pointing it at a single source file
    // directly (root == the file) would strip to an empty relative path —
    // a real, silently-corrupting bug found before writing this, not after.
    // The explicit `world.sources` allowlist is instead recovered by
    // filtering the scan's results down to exactly the requested paths.
    let observer = LocalDocsObserver::with_defaults(Arc::new(TesseractOcr));
    let scan_ctx = ScanContext::new(scenario_dir);
    let mut package = observer
        .scan(&scan_ctx)
        .await
        .map_err(|e| IngestError::Observe(scenario_dir.display().to_string(), e.to_string()))?;

    let wanted: HashSet<&str> = sources.iter().map(String::as_str).collect();
    package
        .artifacts
        .retain(|a| wanted.contains(a.content.target.as_str()));

    let found: HashSet<&str> = package
        .artifacts
        .iter()
        .map(|a| a.content.target.as_str())
        .collect();
    for source in sources {
        if !found.contains(source.as_str()) {
            return Err(IngestError::SourceNotFound(source.clone()));
        }
    }

    // RFC 0043: the same choke-point treatment build.rs's own observation
    // loop gives every artifact, applied here as this ingestion's own
    // independent raw-content entry point — excluded-path filtering, then
    // in-place redaction, before anything reaches an artifact store.
    let redaction_config = RedactionConfig::default();
    package
        .artifacts
        .retain(|a| !redaction::is_excluded_path(&a.content.target, &redaction_config));
    for artifact in &mut package.artifacts {
        redaction::redact_json(&mut artifact.content.data, &redaction_config);
    }

    let artifact_dir = tempfile::tempdir()?;
    let artifact_store: Arc<dyn ArtifactStore> = Arc::new(FileSystemArtifactStore::new(
        artifact_dir.path().to_path_buf(),
    ));

    let mut doc_artifact_ids = Vec::with_capacity(package.artifacts.len());
    for artifact in &package.artifacts {
        let json = serde_json::to_value(artifact)?;
        artifact_store.write(&artifact.id, &json)?;
        doc_artifact_ids.push(artifact.id.clone());
    }

    let config = Arc::new(EkosConfig::default());
    let mut ctx = PassContext::new(config, scenario_dir.to_path_buf())
        .with_artifact_store(artifact_store.clone());
    let mut pass = LocalDocAnalyzerPass::new("world-sources", doc_artifact_ids);
    pass.run(&mut ctx)
        .await
        .map_err(|e: PassError| IngestError::Pass(e.to_string()))?;

    let mut objects = Vec::new();
    let mut relationships = Vec::new();
    let mut evidence = Vec::new();
    for id in artifact_store.list()? {
        let Some(json) = artifact_store.read(&id)? else {
            continue;
        };
        let Ok(knowledge) = serde_json::from_value::<KnowledgeArtifact>(json) else {
            continue;
        };
        if knowledge.artifact_type != ArtifactType::Knowledge {
            continue;
        }
        evidence.extend(knowledge.content.kir.evidence);
        objects.extend(knowledge.content.kir.objects);
        relationships.extend(knowledge.content.kir.relationships);
    }

    for ev in &evidence {
        store.append_evidence(ev)?;
    }
    for obj in &objects {
        store.append_object(obj)?;
    }
    for rel in &relationships {
        store.append_relationship(rel)?;
    }

    Ok(objects)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_ledger::Ledger;
    use tempfile::tempdir;

    #[test]
    fn ingests_a_real_markdown_file_into_a_document_object() {
        let scenario_dir = tempdir().unwrap();
        std::fs::write(
            scenario_dir.path().join("report_01.md"),
            "# Incident Report\n\nAlice observed money missing from the till.",
        )
        .unwrap();

        let ledger_dir = tempdir().unwrap();
        let ledger = Ledger::open(&ledger_dir.path().join("ledger.db")).unwrap();

        let objects =
            ingest_sources(&ledger, scenario_dir.path(), &["report_01.md".to_string()]).unwrap();

        let doc = objects
            .iter()
            .find(|o| o.name == "report_01.md")
            .expect("a Document object named after the source path must exist");
        assert_eq!(
            doc.kind,
            ekos_kir::ObjectKind::Custom("Document".to_string())
        );

        // Persisted to the store, not just returned in memory.
        let reloaded = ledger.get_object(&doc.id).unwrap().unwrap();
        assert_eq!(reloaded.name, "report_01.md");
    }

    #[test]
    fn missing_source_is_a_real_error_not_a_silent_skip() {
        let scenario_dir = tempdir().unwrap();
        let ledger_dir = tempdir().unwrap();
        let ledger = Ledger::open(&ledger_dir.path().join("ledger.db")).unwrap();

        let result = ingest_sources(
            &ledger,
            scenario_dir.path(),
            &["does_not_exist.md".to_string()],
        );
        let Err(err) = result else {
            panic!("expected SourceNotFound");
        };
        assert!(matches!(err, IngestError::SourceNotFound(s) if s == "does_not_exist.md"));
    }

    #[test]
    fn only_listed_sources_are_ingested_not_every_document_in_the_directory() {
        let scenario_dir = tempdir().unwrap();
        std::fs::write(scenario_dir.path().join("wanted.md"), "wanted content").unwrap();
        std::fs::write(scenario_dir.path().join("unwanted.md"), "unwanted content").unwrap();

        let ledger_dir = tempdir().unwrap();
        let ledger = Ledger::open(&ledger_dir.path().join("ledger.db")).unwrap();

        let objects =
            ingest_sources(&ledger, scenario_dir.path(), &["wanted.md".to_string()]).unwrap();

        assert!(objects.iter().any(|o| o.name == "wanted.md"));
        assert!(
            !objects.iter().any(|o| o.name == "unwanted.md"),
            "a sibling document not listed in world.sources must never be ingested"
        );
    }

    #[test]
    fn redaction_strips_a_recognizable_secret_before_it_reaches_the_store() {
        let scenario_dir = tempdir().unwrap();
        std::fs::write(
            scenario_dir.path().join("leaky.md"),
            "Our AWS key is AKIAABCDEFGHIJKLMNOP, keep it safe.",
        )
        .unwrap();

        let ledger_dir = tempdir().unwrap();
        let ledger = Ledger::open(&ledger_dir.path().join("ledger.db")).unwrap();

        let objects =
            ingest_sources(&ledger, scenario_dir.path(), &["leaky.md".to_string()]).unwrap();

        let full_json = serde_json::to_string(&objects).unwrap();
        assert!(
            !full_json.contains("AKIAABCDEFGHIJKLMNOP"),
            "the raw secret must never reach an ingested object's properties"
        );
        assert!(
            full_json.contains("REDACTED"),
            "a redaction marker must be present in its place"
        );
    }
}
