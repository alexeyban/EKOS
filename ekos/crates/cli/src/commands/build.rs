use anyhow::Result;
use ekos_artifact::{ArtifactStore, FileSystemArtifactStore, IndexArtifact};
use ekos_compiler_core::EkosConfig;
use ekos_kir::{KirEvidence, KirId, KirObject, ObjectKind, SourceLocation};
use ekos_ledger::Ledger;
use ekos_observation_sdk::{Observer, ScanContext};
use ekos_plugin_file::FileObserver;
use ekos_plugin_git::GitObserver;
use std::collections::HashMap;
use std::path::Path;
use uuid::Uuid;

pub async fn run(config: &EkosConfig, cwd: &Path) -> Result<()> {
    let ledger_path = config.ledger_path(cwd);
    let ledger = Ledger::open(&ledger_path)
        .map_err(|e| anyhow::anyhow!("cannot open ledger at {}: {e}", ledger_path.display()))?;

    let artifact_store = FileSystemArtifactStore::new(config.artifact_dir(cwd));

    let observe_paths: Vec<std::path::PathBuf> = if config.observe.paths.is_empty() {
        vec![cwd.to_path_buf()]
    } else {
        config.observe.paths.iter().map(|p| cwd.join(p)).collect()
    };

    let observers: Vec<Box<dyn Observer>> =
        vec![Box::new(FileObserver::new()), Box::new(GitObserver::new())];

    let mut total_observed = 0usize;
    let mut total_skipped = 0usize;
    let mut index_entries: HashMap<String, ekos_artifact::ArtifactId> = HashMap::new();

    for base in &observe_paths {
        let ctx = ScanContext::new(base)
            .with_ignore_patterns(config.observe.ignore_patterns.clone());

        for observer in &observers {
            let package = match observer.scan(&ctx).await {
                Ok(p) => p,
                Err(e) => {
                    tracing::warn!(observer = observer.name(), "scan failed: {e}");
                    continue;
                }
            };

            for artifact in &package.artifacts {
                let artifact_json = serde_json::to_value(artifact)?;
                let key = format!("{}/{}", observer.name(), artifact.content.target);
                artifact_store.write(&artifact.id, &artifact_json)?;
                index_entries.insert(key, artifact.id.clone());
            }

            // Produce KirObjects only for file observations (skeleton behaviour).
            // Git commits will be promoted to KirEvents in Phase 6 by GitAnalyzer.
            if observer.name() == "file" {
                for artifact in &package.artifacts {
                    let rel_str = &artifact.content.target;
                    let obj_id =
                        KirId(Uuid::new_v5(&Uuid::NAMESPACE_URL, rel_str.as_bytes()));
                    let ev_id = KirId(Uuid::new_v5(
                        &Uuid::NAMESPACE_URL,
                        format!("ev:{rel_str}").as_bytes(),
                    ));

                    let size = artifact.content.data["size_bytes"].as_u64().unwrap_or(0);
                    let abs_path = base.join(rel_str);

                    let mut ev = KirEvidence::new(
                        SourceLocation::file(abs_path.to_string_lossy().as_ref()),
                        format!("file: {rel_str} ({size} bytes)"),
                    );
                    ev.id = ev_id;

                    let mut obj = KirObject::new(rel_str, ObjectKind::File)
                        .with_property("path", serde_json::Value::String(rel_str.clone()))
                        .with_property("size_bytes", serde_json::json!(size))
                        .with_property(
                            "artifact_id",
                            serde_json::Value::String(artifact.id.to_string()),
                        )
                        .with_evidence(ev_id);
                    obj.id = obj_id;

                    ledger.append_evidence(&ev)?;
                    let is_new = ledger.append_object(&obj)?;
                    if is_new {
                        total_observed += 1;
                        tracing::debug!(path = %rel_str, "observed file");
                    } else {
                        total_skipped += 1;
                    }
                }
            }
        }
    }

    // ── Write build index (snapshot) ─────────────────────────────────────────
    let build_id = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    let index = IndexArtifact::new(&build_id, index_entries);
    let index_json = serde_json::to_value(&index)?;
    artifact_store.write(&index.id, &index_json)?;

    let snapshot_dir = config.ekos_dir(cwd).join("snapshots");
    std::fs::create_dir_all(&snapshot_dir)?;
    let snapshot_path = snapshot_dir.join(format!("{build_id}.json"));
    std::fs::write(&snapshot_path, serde_json::to_string_pretty(&index_json)?)?;

    let total_objects = ledger.object_count()?;
    println!("Build complete.");
    println!("  Files observed (new): {total_observed}");
    if total_skipped > 0 {
        println!("  Files skipped (cached): {total_skipped}");
    }
    println!("  Total objects in ledger: {total_objects}");
    println!("  Snapshot: .ekos/snapshots/{build_id}.json");
    Ok(())
}
