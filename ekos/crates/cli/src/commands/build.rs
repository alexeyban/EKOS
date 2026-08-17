use super::store::open_store;
use anyhow::Result;
use ekos_artifact::{ArtifactStore, IndexArtifact, PackArtifactStore};
use ekos_compiler_core::EkosConfig;
use ekos_kir::{KirEvidence, KirId, KirObject, ObjectKind, SourceLocation};
use ekos_observation_sdk::{Observer, ScanContext, source_fingerprint};
use ekos_plugin_clickhouse::{ClickHouseHttpClient, ClickHouseObserver};
use ekos_plugin_confluence::{ConfluenceApiClient, ConfluenceObserver};
use ekos_plugin_crypto::{CryptoObserver, ParquetExportReader};
use ekos_plugin_file::FileObserver;
use ekos_plugin_git::GitObserver;
use ekos_plugin_github::{GitHubApiClient, GitHubObserver};
use ekos_plugin_localdocs::{LocalDocsObserver, TesseractOcr};
use ekos_plugin_pentaho::PentahoObserver;
use ekos_plugin_python::PythonObserver;
use ekos_plugin_rust::RustObserver;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use uuid::Uuid;

/// Env var pointing at a DeFi Sentinel export root (see RFC 0017). The crypto
/// observer is only added when this is set — most `ekos build` runs in this
/// repo self-observe and have no crypto export to read, so its absence is a
/// normal state, not a misconfiguration (soft-skip, mirrors how `recover.rs`
/// selects an LLM provider off `ANTHROPIC_API_KEY`).
const CRYPTO_EXPORT_DIR_ENV: &str = "EKOS_CRYPTO_EXPORT_DIR";

/// Env vars naming the `owner/repo` to observe via the GitHub connector (see
/// RFC 0020). Both must be set — the observer is only added when they are;
/// their absence is a normal state (most workspaces have no GitHub repo
/// configured), not a misconfiguration, same soft-skip as the crypto
/// connector above. `EKOS_GITHUB_TOKEN` is optional — unauthenticated
/// requests work against public repos, just with a lower rate limit.
const GITHUB_OWNER_ENV: &str = "EKOS_GITHUB_OWNER";
const GITHUB_REPO_ENV: &str = "EKOS_GITHUB_REPO";
const GITHUB_TOKEN_ENV: &str = "EKOS_GITHUB_TOKEN";

/// Env vars naming the Confluence site/space to observe (see RFC 0022).
/// Both base URL and space key must be set — the observer is only added
/// when they are; their absence is a normal state (most workspaces have no
/// Confluence space configured), same soft-skip as the crypto/GitHub
/// connectors above. `EKOS_CONFLUENCE_TOKEN` is optional.
const CONFLUENCE_BASE_URL_ENV: &str = "EKOS_CONFLUENCE_BASE_URL";
const CONFLUENCE_SPACE_ENV: &str = "EKOS_CONFLUENCE_SPACE";
const CONFLUENCE_TOKEN_ENV: &str = "EKOS_CONFLUENCE_TOKEN";

/// Env vars naming the ClickHouse HTTP endpoint/database to observe (RFC 0056). URL and
/// database must both be set — the observer is only added when they are; their absence is a
/// normal state (most workspaces have no ClickHouse database configured), same soft-skip as the
/// crypto/GitHub/Confluence connectors above. `EKOS_CLICKHOUSE_USER`/`EKOS_CLICKHOUSE_PASSWORD`
/// are optional — a server with no auth configured (common for local/dev ClickHouse) works with
/// empty credentials.
const CLICKHOUSE_URL_ENV: &str = "EKOS_CLICKHOUSE_URL";
const CLICKHOUSE_DATABASE_ENV: &str = "EKOS_CLICKHOUSE_DATABASE";
const CLICKHOUSE_USER_ENV: &str = "EKOS_CLICKHOUSE_USER";
const CLICKHOUSE_PASSWORD_ENV: &str = "EKOS_CLICKHOUSE_PASSWORD";

/// Load the `.ekos/fingerprints.json` map of observe-path → last-seen source fingerprint.
fn load_fingerprints(path: &Path) -> HashMap<String, String> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_fingerprints(path: &Path, fingerprints: &HashMap<String, String>) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(fingerprints)?)?;
    Ok(())
}

pub async fn run(config: &EkosConfig, cwd: &Path) -> Result<()> {
    let ledger = open_store(config, cwd)?;

    let artifact_store = PackArtifactStore::open(config.artifact_dir(cwd))
        .map_err(|e| anyhow::anyhow!("cannot open artifact store: {e}"))?;

    let observe_paths: Vec<std::path::PathBuf> = if config.observe.paths.is_empty() {
        vec![cwd.to_path_buf()]
    } else {
        config.observe.paths.iter().map(|p| cwd.join(p)).collect()
    };

    let mut observers: Vec<Box<dyn Observer>> = vec![
        Box::new(FileObserver::new()),
        Box::new(GitObserver::new()),
        // RFC 0023: local files, no credential to gate on — runs
        // unconditionally, same as FileObserver/GitObserver.
        Box::new(LocalDocsObserver::with_defaults(Arc::new(TesseractOcr))),
        // RFC 0027 Phase 3: local .ktr/.kjb files, no credential to gate on —
        // runs unconditionally, same as LocalDocsObserver.
        Box::new(PentahoObserver::new()),
        // RFC 0038/0040 Phase 2: local .py files, no credential to gate on —
        // runs unconditionally, same as PentahoObserver.
        Box::new(PythonObserver::new()),
        // RFC 0041: local .rs files, no credential to gate on — runs
        // unconditionally, same as PythonObserver.
        Box::new(RustObserver::new()),
    ];
    if let Ok(export_dir) = std::env::var(CRYPTO_EXPORT_DIR_ENV) {
        observers.push(Box::new(CryptoObserver::new(
            Arc::new(ParquetExportReader),
            export_dir,
        )));
    } else {
        tracing::debug!("{CRYPTO_EXPORT_DIR_ENV} not set — crypto connector skipped (RFC 0017)");
    }
    match (
        std::env::var(GITHUB_OWNER_ENV),
        std::env::var(GITHUB_REPO_ENV),
    ) {
        (Ok(owner), Ok(repo)) => {
            let token = std::env::var(GITHUB_TOKEN_ENV).ok();
            observers.push(Box::new(GitHubObserver::new(
                Arc::new(GitHubApiClient::new(token)),
                owner,
                repo,
            )));
        }
        _ => {
            tracing::debug!(
                "{GITHUB_OWNER_ENV}/{GITHUB_REPO_ENV} not set — github connector skipped (RFC 0020)"
            );
        }
    }
    match (
        std::env::var(CONFLUENCE_BASE_URL_ENV),
        std::env::var(CONFLUENCE_SPACE_ENV),
    ) {
        (Ok(base_url), Ok(space_key)) => {
            let token = std::env::var(CONFLUENCE_TOKEN_ENV).ok();
            observers.push(Box::new(ConfluenceObserver::new(
                Arc::new(ConfluenceApiClient::new(base_url, token)),
                space_key,
            )));
        }
        _ => {
            tracing::debug!(
                "{CONFLUENCE_BASE_URL_ENV}/{CONFLUENCE_SPACE_ENV} not set — confluence connector skipped (RFC 0022)"
            );
        }
    }
    match (
        std::env::var(CLICKHOUSE_URL_ENV),
        std::env::var(CLICKHOUSE_DATABASE_ENV),
    ) {
        (Ok(url), Ok(database)) => {
            let user = std::env::var(CLICKHOUSE_USER_ENV).unwrap_or_default();
            let password = std::env::var(CLICKHOUSE_PASSWORD_ENV).unwrap_or_default();
            observers.push(Box::new(ClickHouseObserver::new(Arc::new(
                ClickHouseHttpClient::new(url, user, password, database),
            ))));
        }
        _ => {
            tracing::debug!(
                "{CLICKHOUSE_URL_ENV}/{CLICKHOUSE_DATABASE_ENV} not set — clickhouse connector skipped (RFC 0056)"
            );
        }
    }

    let fingerprint_path = config.ekos_dir(cwd).join("fingerprints.json");
    let mut fingerprints = load_fingerprints(&fingerprint_path);

    let mut total_observed = 0usize;
    let mut total_skipped = 0usize;
    let mut connectors_rescanned = 0usize;
    let mut connectors_skipped_cached = 0usize;
    let mut index_entries: HashMap<String, ekos_artifact::ArtifactId> = HashMap::new();
    let redaction_config = config.redaction_config();

    for base in &observe_paths {
        // RFC 0044 Phase 1: distinguishes objects from different projects when `[observe] paths`
        // lists more than one entry — empty for the overwhelmingly common single-path case, so
        // existing single-project ledgers keep byte-identical ids (no migration needed there).
        // Without this, two unrelated projects that each happen to have e.g. `src/main.rs` at the
        // same relative path silently collided into one merged `KirObject` — ids below were
        // hashed from the bare within-project relative path only, with no project component. A
        // real bug found designing multi-project/estate-scale support, not a hypothetical.
        let project_key = if observe_paths.len() > 1 {
            base.strip_prefix(cwd)
                .unwrap_or(base)
                .to_string_lossy()
                .replace('\\', "/")
        } else {
            String::new()
        };

        let ctx =
            ScanContext::new(base).with_ignore_patterns(config.observe.ignore_patterns.clone());

        let fp = source_fingerprint(&ctx);
        let fp_key = base.display().to_string();
        if fingerprints.get(&fp_key) == Some(&fp.0) {
            connectors_skipped_cached += observers.len();
            continue;
        }
        fingerprints.insert(fp_key, fp.0);

        for observer in &observers {
            connectors_rescanned += 1;
            let mut package = match observer.scan(&ctx).await {
                Ok(p) => p,
                Err(e) => {
                    tracing::warn!(observer = observer.name(), "scan failed: {e}");
                    continue;
                }
            };

            // RFC 0043: the single central choke point every observer's artifacts pass through
            // before persistence — files matching a built-in/configured secret-file glob (`.env`,
            // `*.pem`, …) are dropped entirely; every other artifact's content gets its matched
            // secret spans redacted in place. This protects every current and future connector
            // without relying on each one remembering to sanitize its own output.
            package.artifacts.retain(|a| {
                !ekos_common::redaction::is_excluded_path(&a.content.target, &redaction_config)
            });
            for artifact in &mut package.artifacts {
                ekos_common::redaction::redact_json(&mut artifact.content.data, &redaction_config);
            }

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
                    // Project-qualify the id hash input only (never `rel_str` itself — that stays
                    // the plain within-project path for `content.target`/display/`abs_path`
                    // below); see the `project_key` comment above the outer loop.
                    let id_key = if project_key.is_empty() {
                        rel_str.clone()
                    } else {
                        format!("{project_key}:{rel_str}")
                    };
                    let obj_id = KirId(Uuid::new_v5(&Uuid::NAMESPACE_URL, id_key.as_bytes()));
                    let ev_id = KirId(Uuid::new_v5(
                        &Uuid::NAMESPACE_URL,
                        format!("ev:{id_key}").as_bytes(),
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
                    // RFC 0044 Phase 1: lets a future rollup/grouping pass (or any query) filter
                    // or group by originating project in a multi-project estate; absent entirely
                    // for the common single-path workspace, matching `project_key`'s emptiness.
                    if !project_key.is_empty() {
                        obj = obj.with_property(
                            "project",
                            serde_json::Value::String(project_key.clone()),
                        );
                    }
                    // RFC 0014: the excerpt rides on the object so the ledger
                    // can index file *content*, not just names.
                    if let Some(excerpt) = artifact.content.data["excerpt"].as_str() {
                        obj = obj.with_property(
                            "excerpt",
                            serde_json::Value::String(excerpt.to_string()),
                        );
                    }
                    // RFC 0019: harvested declaration-line symbols ride
                    // alongside the excerpt so FTS can find e.g.
                    // `fn authenticate_user` even deep in a large file.
                    if let Some(symbols) = artifact.content.data.get("symbols") {
                        obj = obj.with_property("symbols", symbols.clone());
                    }
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

    save_fingerprints(&fingerprint_path, &fingerprints)?;

    // ── Write build index (snapshot) ─────────────────────────────────────────
    let build_id = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    let index = IndexArtifact::new(&build_id, index_entries);
    let index_json = serde_json::to_value(&index)?;
    artifact_store.write(&index.id, &index_json)?;

    let snapshot_dir = config.ekos_dir(cwd).join("snapshots");
    std::fs::create_dir_all(&snapshot_dir)?;
    // RFC 0015: snapshots are compressed and pruned; the full history stays
    // available through the content-addressed IndexArtifacts written above.
    let snapshot_path = snapshot_dir.join(format!("{build_id}.json.zst"));
    ekos_common::compress::write_json_zst(&snapshot_path, &index_json)?;
    prune_snapshots(&snapshot_dir, SNAPSHOT_KEEP);

    let total_objects = ledger.object_count()?;
    println!("Build complete.");
    println!("  Files observed (new): {total_observed}");
    if total_skipped > 0 {
        println!("  Files skipped (cached): {total_skipped}");
    }
    println!("  Total objects in ledger: {total_objects}");
    if connectors_rescanned == 0 {
        println!("  0 connectors re-scanned");
    } else {
        println!("  Connectors re-scanned: {connectors_rescanned}");
    }
    if connectors_skipped_cached > 0 {
        println!("  {connectors_skipped_cached} connector(s) skipped (cached)");
    }
    println!("  Snapshot: .ekos/snapshots/{build_id}.json.zst");
    Ok(())
}

/// Snapshots kept on disk after each build (RFC 0015 retention).
const SNAPSHOT_KEEP: usize = 10;

/// Delete all but the newest `keep` snapshot files. Build ids are UTC
/// timestamps, so lexicographic filename order is chronological order —
/// including legacy uncompressed `.json` snapshots.
fn prune_snapshots(snapshot_dir: &Path, keep: usize) {
    let Ok(entries) = std::fs::read_dir(snapshot_dir) else {
        return;
    };
    let mut names: Vec<_> = entries
        .flatten()
        .filter(|e| e.path().is_file())
        .map(|e| e.path())
        .collect();
    names.sort();
    let excess = names.len().saturating_sub(keep);
    for path in names.into_iter().take(excess) {
        std::fs::remove_file(&path).ok();
    }
}
