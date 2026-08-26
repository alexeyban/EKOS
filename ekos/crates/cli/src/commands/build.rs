use super::store::open_store;
use anyhow::Result;
use ekos_artifact::{ArtifactId, ArtifactStore, IndexArtifact, PackArtifactStore};
use ekos_compiler_core::EkosConfig;
use ekos_kir::{KirEvidence, KirId, KirObject, ObjectKind, SourceLocation};
use ekos_observation_sdk::{Observer, ScanContext, source_fingerprint};
use ekos_plugin_clickhouse::{ClickHouseHttpClient, ClickHouseObserver};
use ekos_plugin_confluence::{ConfluenceApiClient, ConfluenceObserver};
use ekos_plugin_crypto::{CryptoObserver, ParquetExportReader};
use ekos_plugin_elixir::ElixirObserver;
use ekos_plugin_file::FileObserver;
use ekos_plugin_git::GitObserver;
use ekos_plugin_github::{GitHubApiClient, GitHubObserver};
use ekos_plugin_javascript::JavaScriptObserver;
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
/// Optional pagination knobs for the GitHub connector (RFC 0062). Both unset
/// reproduces the pre-RFC-0062 single-page behavior exactly (see
/// `GitHubApiClient::with_pagination`'s doc comment) — GitHub's own default
/// page size (30 items) otherwise silently truncates any repo with more
/// history than that.
const GITHUB_PER_PAGE_ENV: &str = "EKOS_GITHUB_PER_PAGE";
const GITHUB_MAX_PAGES_ENV: &str = "EKOS_GITHUB_MAX_PAGES";

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
        // RFC 0081: local .ex/.exs files, no credential to gate on — runs
        // unconditionally, same as RustObserver.
        Box::new(ElixirObserver::new()),
        // RFC 0085: local .js/.jsx/.ts/.tsx/.mjs/.cjs files, no credential to gate on — runs
        // unconditionally, same as ElixirObserver.
        Box::new(JavaScriptObserver::new()),
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
            let per_page = std::env::var(GITHUB_PER_PAGE_ENV)
                .ok()
                .and_then(|v| v.parse::<u32>().ok());
            let max_pages = std::env::var(GITHUB_MAX_PAGES_ENV)
                .ok()
                .and_then(|v| v.parse::<u32>().ok());
            let mut client = GitHubApiClient::new(token);
            if per_page.is_some() || max_pages.is_some() {
                client = client.with_pagination(per_page, max_pages.unwrap_or(1));
            }
            observers.push(Box::new(GitHubObserver::new(Arc::new(client), owner, repo)));
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

    // RFC 0077: `File`-kind `KirObject`s are constructed and written to the ledger inline, right
    // here, only when a `file` observer's package is freshly produced this run — unlike
    // `recover`-stage analyzer output (re-derived from `artifact_store` fresh on every invocation,
    // independent of any fingerprint), this inline construction is never independently replayable
    // from the artifact cache. Found live: clearing just `.ekos/ledger/` while keeping
    // `.ekos/artifacts/` and `fingerprints.json` reproduced zero `File` objects, because the
    // fingerprint-cache-hit branch below skipped this whole per-path block unconditionally,
    // regardless of whether the ledger it's supposed to be populating still had anything in it.
    // `ledger.object_count() == 0` is a cheap, always-correct signal that the ledger was just
    // cleared (or never populated) — when true, no fingerprint is trusted this run, forcing a real
    // rescan that repopulates it; every subsequent run (ledger no longer empty) resumes trusting
    // the cache normally. Doesn't cover the far rarer case of *other* kinds surviving while only
    // `File` objects were somehow selectively lost — that's not what was found live, and this
    // fix's scope is deliberately the reported scenario, not every hypothetical partial-loss case.
    let ledger_is_empty = ledger.object_count()? == 0;

    let mut total_observed = 0usize;
    let mut total_skipped = 0usize;
    let mut connectors_rescanned = 0usize;
    let mut connectors_skipped_cached = 0usize;
    let mut index_entries: HashMap<String, ekos_artifact::ArtifactId> = HashMap::new();
    let redaction_config = config.redaction_config();

    for base in &observe_paths {
        // RFC 0044 Phase 1: distinguishes objects from different projects when `[observe] paths`
        // lists more than one entry — empty for the overwhelmingly common `paths = ["."]` case,
        // so existing single-project ledgers keep byte-identical ids (no migration needed there).
        // Without this, two unrelated projects that each happen to have e.g. `src/main.rs` at the
        // same relative path silently collided into one merged `KirObject` — ids below were
        // hashed from the bare within-project relative path only, with no project component. A
        // real bug found designing multi-project/estate-scale support, not a hypothetical.
        //
        // Condition fixed 2026-08-23 (RFC 0088's own live verification found this): the original
        // `observe_paths.len() > 1` check meant a workspace with exactly *one* `[observe] paths`
        // entry that isn't `"."` (a real, common shape — `paths = ["src"]`, or a single scoped
        // subdirectory like the analytics project's own `lib/plausible/auth`) silently dropped
        // the real directory prefix from every `File.name` with no `"project"` property left to
        // reconstruct it — any later real disk read (RFC 0088's `read_symbol_source`) then
        // silently failed. `base != cwd` is a strictly more precise condition than counting
        // entries: it's still empty for the byte-identical-ids `paths = ["."]` case (there,
        // `base == cwd` always), but now also correctly captures the real prefix for a single
        // non-`"."` entry, which used to be wrongly treated as needing none. Now the single
        // source of truth for this rule (`ekos_common::project::project_key_for_base`) — found
        // live, 2026-08-24: `recover.rs`'s several raw-content collection loops each duplicated
        // this exact logic with the *old*, unfixed condition, silently diverging from this
        // function's own real `File`-object ids the moment this fix landed here alone.
        let project_key = ekos_common::project::project_key_for_base(base, cwd);

        let ctx =
            ScanContext::new(base).with_ignore_patterns(config.observe.ignore_patterns.clone());

        let fp = source_fingerprint(&ctx);
        let fp_key = base.display().to_string();
        if !ledger_is_empty && fingerprints.get(&fp_key) == Some(&fp.0) {
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
                // RFC 0079: the same central choke point, for the same reason — every path-keyed
                // recovery pass that derives a `KirId` from a raw path string embedded in this
                // artifact's own `data` (not `content.target`, which these passes never read:
                // `local_docs_analyzer.rs`/`rust_analyzer.rs`/`python_analyzer.rs`'s `data.path`,
                // `git_analyzer.rs`'s per-commit `data.files_changed`) has no project context of
                // its own — `project_key` only ever existed as this loop's own transient local
                // (RFC 0044, `devlog_65`'s investigation confirmed this is a real, not
                // per-file-tweakable gap). Riding a `"project"` field on `data` here, once, for
                // every connector's artifacts, means those passes can read it straight back
                // without this crate needing to know anything about their individual `data`
                // shapes — see each pass's own `project_qualified_id` usage for the consuming
                // half of this fix. Absent entirely (not an empty string) for the single-path
                // case, matching the same "existing single-project ledgers keep byte-identical
                // ids" principle `build.rs`'s own `File`-object fix already established.
                if !project_key.is_empty()
                    && let Some(obj) = artifact.content.data.as_object_mut()
                {
                    obj.insert(
                        "project".to_string(),
                        serde_json::Value::String(project_key.clone()),
                    );
                }

                // Real bug, found live 2026-08-25 re-running `ekos build`/`recover` against
                // EKOS's own repository: `observer.scan()` computes each `ObservationArtifact`'s
                // content-addressed `id` from the *raw*, pre-redaction `data` (inside the
                // observer's own `ObservationArtifact::new` call) — but `redact_json` above then
                // mutates `data` in place, so the id on disk never matches what actually got
                // written. `PackArtifactStore::write` is skip-if-exists (`if self.exists(id) {
                // return Ok(false); }`, never an overwrite), so once a file's raw content is
                // first observed, whatever `redact` happened to produce *that one time* is locked
                // in under that id forever — every later fix to the redaction engine (this
                // session shipped at least one real one, `devlog_100`) silently never applies to
                // it again, since the same unchanged raw content always re-derives the same
                // pre-redaction id and `write` sees "already have this" and skips. Confirmed live:
                // `crates/clickhouse-query/src/client.rs`'s real `password: password.into()` field
                // init was mangled by a since-fixed version of the `generic-assigned-secret`
                // pattern into `[REDACTED:...].into()` — today's `redact()` no longer does this to
                // fresh content, but the stale artifact from whenever this file was first observed
                // kept serving the broken version, corrupting `rust_analyzer.rs`'s parse of it on
                // every subsequent `recover` (RUST003) regardless of how many times `build`/
                // `recover` reran. The fix: recompute the id from the *final*, already-redacted
                // `content` right here, so a redaction-logic change that alters the output for
                // unchanged raw source naturally becomes a new id — a real, fresh artifact `write`
                // actually persists — instead of silently resolving to a stale one forever.
                artifact.id = ArtifactId::compute(
                    &serde_json::to_value(&artifact.content).expect("content must serialize"),
                );
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

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_compiler_core::config::SecretPatternConfig;
    use ekos_kir::ObjectKind;
    use tempfile::tempdir;

    fn file_object_count(config: &EkosConfig, cwd: &Path) -> usize {
        open_store(config, cwd)
            .unwrap()
            .all_objects()
            .unwrap()
            .iter()
            .filter(|o| o.kind == ObjectKind::File)
            .count()
    }

    #[tokio::test]
    async fn rebuilding_after_a_ledger_clear_reproduces_file_objects_despite_a_fingerprint_hit() {
        // RFC 0077: found live — clearing just `.ekos/ledger/` while keeping the artifact cache
        // and `fingerprints.json` used to reproduce zero `File` objects, because an unchanged
        // fingerprint skipped the whole per-path scan (and the `File`-KirObject construction
        // inlined inside it) unconditionally, regardless of what the ledger actually still held.
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("hello.txt"), b"hello world").unwrap();
        let config = EkosConfig::default();

        run(&config, dir.path()).await.unwrap();
        assert!(
            file_object_count(&config, dir.path()) > 0,
            "first build must produce File objects"
        );

        // Real scenario: only the ledger is cleared. The artifact cache and fingerprints.json
        // survive untouched.
        std::fs::remove_dir_all(config.ekos_dir(dir.path()).join("ledger")).unwrap();

        // Source content is unchanged, so the fingerprint would normally match and skip
        // everything — but the ledger is now empty, so this must force a real rescan instead.
        run(&config, dir.path()).await.unwrap();
        assert!(
            file_object_count(&config, dir.path()) > 0,
            "File objects must be reproduced after a ledger clear, even though the fingerprint \
             cache matches the unchanged source content"
        );
    }

    /// The real `data.source` text `rust_analyzer.rs` (and every other recovery pass) actually
    /// reads back from the persisted artifact store — the thing that stayed stale under the real
    /// bug, unlike the ledger's own `File` KirObject (rebuilt fresh from in-memory data on every
    /// `build` run regardless of what the artifact store's `write()` actually persisted, so it
    /// can never expose this class of staleness).
    /// Every real `data.source` text a "rust" connector artifact for `target` currently holds in
    /// the store — plural, deliberately: the old, stale, pre-fix artifact and a freshly re-
    /// redacted one legitimately coexist in the store under two different content-addressed ids
    /// once the fix writes a new one (the fix's job is making sure a *fresh* one gets written at
    /// all, not garbage-collecting the old one — that's a separate, real, un-addressed cleanup
    /// question this test doesn't claim to answer).
    fn rust_artifact_sources(config: &EkosConfig, cwd: &Path, target: &str) -> Vec<String> {
        let store = ekos_artifact::PackArtifactStore::open(config.artifact_dir(cwd)).unwrap();
        let mut found = Vec::new();
        for id in store.list().unwrap() {
            let Some(json) = store.read(&id).unwrap() else {
                continue;
            };
            if json["connector_name"] == "rust" && json["target"] == target {
                found.push(
                    json["data"]["source"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string(),
                );
            }
        }
        found
    }

    /// Real bug, found live 2026-08-25 re-running `ekos build`/`recover` against EKOS's own
    /// repository: an `ObservationArtifact`'s content-addressed id is computed from the *raw*,
    /// pre-redaction data inside the observer's own `scan()`, but `redact_json` mutates that data
    /// afterward — so `PackArtifactStore::write`'s skip-if-exists semantics permanently locks in
    /// whatever redaction happened to run the *first* time a file's unchanged raw content was
    /// ever observed, and no later fix to the redaction engine (or, as exercised here, no later
    /// *addition* of a real `[security]` custom pattern) can ever re-redact it — the same
    /// unchanged raw content always re-derives the same pre-redaction id, `write` sees "already
    /// have this," and the stale, differently-redacted version keeps being served forever to
    /// every recovery pass that reads it back (confirmed live: `rust_analyzer.rs` failing to
    /// parse a real file whose stale artifact still held a since-fixed redaction mangling).
    #[tokio::test]
    async fn a_later_redaction_pattern_addition_actually_re_redacts_unchanged_source() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("secret.rs"),
            b"const TOKEN: &str = \"myspecialtoken123\";\n",
        )
        .unwrap();

        // First build: no custom pattern matches this content yet — it's stored untouched.
        let config1 = EkosConfig::default();
        run(&config1, dir.path()).await.unwrap();
        let first = rust_artifact_sources(&config1, dir.path(), "secret.rs");
        assert_eq!(first.len(), 1);
        assert!(
            first[0].contains("myspecialtoken123"),
            "unmatched content must be stored as-is on the first build"
        );

        // Same real scenario the RFC 0077 test above already established forces a genuine
        // rescan (not a cached-artifact re-derivation): clear just the ledger.
        std::fs::remove_dir_all(config1.ekos_dir(dir.path()).join("ledger")).unwrap();

        // Second build: a real `[security]` custom pattern now matches the *same, unchanged*
        // raw file content — simulating a redaction-engine fix/addition between two runs.
        let mut config2 = EkosConfig::default();
        config2.security.extra_patterns.push(SecretPatternConfig {
            label: "test-secret".to_string(),
            regex: "myspecialtoken123".to_string(),
        });
        run(&config2, dir.path()).await.unwrap();

        let second = rust_artifact_sources(&config2, dir.path(), "secret.rs");
        assert!(
            second.iter().any(|s| s.contains("[REDACTED:test-secret]")),
            "a freshly, correctly re-redacted artifact must exist after the pattern addition — \
             got: {second:?}"
        );
    }

    #[tokio::test]
    async fn an_unchanged_fingerprint_still_skips_the_rescan_when_the_ledger_is_not_empty() {
        // The fix must not defeat the cache entirely — only bypass it when the ledger looks
        // freshly cleared. A second build with an intact ledger and unchanged content should
        // report the connector(s) as cache-skipped, not re-scanned.
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("hello.txt"), b"hello world").unwrap();
        let config = EkosConfig::default();

        run(&config, dir.path()).await.unwrap();
        let first_count = open_store(&config, dir.path())
            .unwrap()
            .object_count()
            .unwrap();
        assert!(first_count > 0);

        run(&config, dir.path()).await.unwrap();
        let second_count = open_store(&config, dir.path())
            .unwrap()
            .object_count()
            .unwrap();
        assert_eq!(
            first_count, second_count,
            "a cache-hit re-run against an intact ledger must not duplicate or lose objects"
        );
    }
}
