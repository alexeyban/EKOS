use super::store::{open_store, store_display};
use anyhow::Result;
use ekos_compiler_core::EkosConfig;
use ekos_kir::{KirEvidence, KirGraph, KirObject, KirRelationship, SourceLocation};
use ekos_ledger::KnowledgeStore;
use ekos_semantic::{CkModel, CkmRelationship, EvidenceRecord, data_lineage, rollup};
use std::io::{BufRead, Write};
use std::path::Path;

pub async fn run(config: &EkosConfig, cwd: &Path, yes: bool) -> Result<()> {
    let model_path = config.ekos_dir(cwd).join("ckm").join("model.json");

    if ekos_common::compress::resolve_auto(&model_path).is_none() {
        anyhow::bail!(
            "CKM not found at {}[.zst]. Run `ekos compile` first.",
            model_path.display()
        );
    }

    let model: CkModel = ekos_common::compress::read_json_auto(&model_path)?;

    let ledger = open_ledger(config, cwd)?;

    // RFC 0135 Part B — every entry this `commit` writes carries `(run_id, stage, ckm hash)`.
    // The CKM content hash stands in for per-`KnowledgeArtifact` provenance until `compile`
    // propagates that (RFC 0135 §B scope line).
    let run_id = ekos_ledger::provenance::new_run_id();
    let ckm_hash = ekos_common::compress::resolve_auto(&model_path)
        .and_then(|p| std::fs::read(p).ok())
        .map(|b| format!("ckm:{}", ekos_common::ContentHash::of(&b).as_str()));
    let write_ctx = |stage: &'static str| ekos_ledger::provenance::WriteContext {
        run_id: run_id.clone(),
        stage: stage.to_string(),
        source_artifact_id: ckm_hash.clone(),
    };
    ledger.set_write_context(Some(write_ctx("commit")));

    let mut objects_written = 0usize;
    let mut objects_skipped = 0usize;
    let mut rels_written = 0usize;
    let mut evidence_written = 0usize;

    // Write evidence first (objects may reference evidence IDs).
    for ev_record in model.evidence_index.values() {
        let kir_ev = evidence_record_to_kir(ev_record);
        ledger.append_evidence(&kir_ev)?;
        evidence_written += 1;
    }

    // Write canonical objects.
    for ckm_obj in &model.objects {
        let mut kir_obj = ckm_object_to_kir(ckm_obj);
        preserve_claim_review_status(&*ledger, &mut kir_obj)?;
        if ledger.append_object(&kir_obj)? {
            objects_written += 1;
        } else {
            objects_skipped += 1;
        }
    }

    // Write canonical relationships.
    for ckm_rel in &model.relationships {
        let kir_rel = ckm_rel_to_kir(ckm_rel);
        if ledger.append_relationship(&kir_rel)? {
            rels_written += 1;
        }
    }

    ledger.set_write_context(Some(write_ctx("commit:rollup")));

    // RFC 0044: hierarchical rollups run here, not in `SemanticCompilerPass` (`ekos compile`) —
    // `File` objects, the only kind rollups group by directly, are written straight to the ledger
    // by `ekos build`, never through a `KnowledgeArtifact` the compiler reads. This is the first
    // point in the pipeline where `File` objects (just-committed-or-earlier) and CKM-derived
    // objects (just committed above) coexist in one place.
    let rollups_added = commit_rollups(&*ledger)?;

    // RFC 0075: links `TransformNode` Source/Sink nodes to the real `Table`/`Dataset` object they
    // read/write, closing the Data Architecture cross-reference gap RFC 0074 found. Run after
    // rollups for the same reason rollups run here at all — this is the first point in the
    // pipeline where every kind of object involved (CKM-derived `TransformNode`s and `Table`s,
    // just committed above) coexists in one ledger read.
    ledger.set_write_context(Some(write_ctx("commit:lineage")));
    let lineage_links_added = commit_data_lineage(&*ledger)?;
    ledger.set_write_context(Some(write_ctx("commit:llm-description")));

    // RFC 0088: real, evidence-grounded `ai_overview`/`ai_usage`/`ai_comment_check` for every
    // in-scope `Module`/`Rollup`/`Symbol` — opt-in (`[llm-description].enabled`), same reasoning
    // as `[architecture-reasoning]`. Runs last: needs the fully-committed graph above (real
    // cross-file `DependsOn` neighbors, real `Rollup`s), not the in-memory CKM `compile` builds.
    let description_stats = if config.llm_description.enabled {
        Some(run_llm_description(config, cwd, &*ledger, yes).await?)
    } else {
        None
    };

    // RFC 0125: the opt-in vector-search index. Runs last (like `[llm-description]`) so it can
    // embed the `ai_overview` prose that step just wrote. `[embeddings].enabled` gates it;
    // `arms_run.vector` in `retrieve` reports the downgrade when it's absent.
    let embed_stats = if config.embeddings.enabled {
        run_embed(config, cwd, &*ledger).await?
    } else {
        None
    };

    println!("Commit complete.");
    println!("  Objects written:       {objects_written}");
    println!("  Objects skipped:       {objects_skipped} (already in ledger)");
    println!("  Relationships written: {rels_written}");
    println!("  Evidence records:      {evidence_written}");
    if rollups_added > 0 {
        println!("  Subsystem rollups:     {rollups_added}");
    }
    if let Some(stats) = &description_stats {
        println!(
            "  AI descriptions:       {} module(s), {} symbol(s) described ({} cached, {} skipped without a source span, {} errors)",
            stats.modules_described,
            stats.symbols_described,
            stats.skipped_cached,
            stats.symbols_without_span,
            stats.llm_errors
        );
    }
    if lineage_links_added > 0 {
        println!("  Data lineage links:    {lineage_links_added}");
    }
    if let Some(stats) = &embed_stats {
        println!(
            "  Vector embeddings:     {} embedded, {} already indexed, {} error(s) ({}-dim {})",
            stats.embedded, stats.already_indexed, stats.errors, stats.dim, stats.model
        );
    }
    println!("  Ledger:                {}", store_display(config, cwd));

    Ok(())
}

/// Reads the ledger's full current object/relationship set, synthesizes subsystem rollups over
/// it (RFC 0044), and appends only the newly-produced `Rollup` objects/relationships/evidence —
/// everything else was already committed above or in a prior run. Returns the number of rollup
/// objects newly written (0 on a re-run against unchanged input, since `append_object` on an
/// already-known deterministic id is a no-op).
fn commit_rollups(ledger: &dyn KnowledgeStore) -> Result<usize> {
    let objects = ledger.all_objects()?;
    let relationships = ledger.all_relationships()?;

    let original_object_count = objects.len();
    let original_relationship_count = relationships.len();

    let mut graph = KirGraph {
        objects,
        relationships,
        events: Vec::new(),
        evidence: Vec::new(),
    };
    rollup::synthesize_rollups(&mut graph, rollup::DEFAULT_DIRECTORY_DEPTH);

    let new_objects = &graph.objects[original_object_count..];
    let new_relationships = &graph.relationships[original_relationship_count..];

    for ev in &graph.evidence {
        ledger.append_evidence(ev)?;
    }
    let mut written = 0usize;
    for obj in new_objects {
        if ledger.append_object(obj)? {
            written += 1;
        }
    }
    for rel in new_relationships {
        ledger.append_relationship(rel)?;
    }

    Ok(written)
}

/// Reads the ledger's full current object/relationship set, links `TransformNode` Source/Sink
/// nodes to the real `Table`/`Dataset` object they name (RFC 0075), and appends only the newly
/// produced relationships. Deterministic ids (`data_lineage::link_transform_nodes_to_tables`) mean
/// this is a no-op on a re-run against unchanged input, the same as `commit_rollups` above.
fn commit_data_lineage(ledger: &dyn KnowledgeStore) -> Result<usize> {
    let objects = ledger.all_objects()?;
    let relationships = ledger.all_relationships()?;
    let original_relationship_count = relationships.len();

    let mut graph = KirGraph {
        objects,
        relationships,
        events: Vec::new(),
        evidence: Vec::new(),
    };
    data_lineage::link_transform_nodes_to_tables(&mut graph);

    let mut written = 0usize;
    for rel in &graph.relationships[original_relationship_count..] {
        if ledger.append_relationship(rel)? {
            written += 1;
        }
    }

    Ok(written)
}

/// RFC 0109: a freshly-compiled role `Claim` never carries `review_status` — the reasoning pass
/// that produces it is deliberately ledger-free (RFC 0068 §31's own established precedent) and has
/// no way to know whether a human already reviewed this exact claim id via `ekos_architecture_review`.
/// Without this, a reviewed claim would silently revert to unconfirmed on the very next `commit`
/// re-run: the pass's freshly-derived object never has `review_status`, so its content signature
/// (RFC 0015) would always differ from the reviewed version already in the ledger, and
/// `append_object` would write a new, unreviewed-looking version over it. Fixed here — the one
/// place that already does real ledger-aware object enrichment before appending, matching
/// `commit_rollups`/`commit_data_lineage`'s own precedent — by checking the ledger's *current*
/// version of this claim id (if any) and carrying `review_status`/`reviewed_at` forward when the
/// role `value` is unchanged. A genuinely changed `value` does *not* inherit the old review status
/// — that confirmation was never about this new assertion.
fn preserve_claim_review_status(ledger: &dyn KnowledgeStore, obj: &mut KirObject) -> Result<()> {
    if !matches!(&obj.kind, ekos_kir::ObjectKind::Custom(k) if k == "Claim") {
        return Ok(());
    }
    if obj.properties.get("predicate").and_then(|v| v.as_str()) != Some("has_role") {
        return Ok(());
    }
    let Some(current) = ledger.get_object(&obj.id)? else {
        return Ok(()); // brand-new claim, nothing to preserve
    };
    if current.properties.get("value") != obj.properties.get("value") {
        return Ok(()); // the role genuinely changed — a new assertion, not a re-derivation
    }
    if let Some(status) = current.properties.get("review_status") {
        obj.properties
            .insert("review_status".to_string(), status.clone());
    }
    if let Some(reviewed_at) = current.properties.get("reviewed_at") {
        obj.properties
            .insert("reviewed_at".to_string(), reviewed_at.clone());
    }
    Ok(())
}

/// RFC 0088: shows a real cost estimate (an upper bound — a real run may skip some via caching
/// or a missing `source_span`), asks for confirmation unless `yes`, then runs
/// `ekos_recovery::describe_objects` against the real committed ledger.
async fn run_llm_description(
    config: &EkosConfig,
    cwd: &Path,
    ledger: &dyn KnowledgeStore,
    yes: bool,
) -> Result<ekos_recovery::DescriptionStats> {
    let scope = match config.llm_description.scope {
        ekos_compiler_core::config::DescriptionScope::Modules => {
            ekos_recovery::DescriptionScope::Modules
        }
        ekos_compiler_core::config::DescriptionScope::Symbols => {
            ekos_recovery::DescriptionScope::Symbols
        }
        ekos_compiler_core::config::DescriptionScope::All => ekos_recovery::DescriptionScope::All,
    };

    let objects = ledger.all_objects()?;
    let (modules, symbols) = ekos_recovery::estimate_call_counts(&objects, scope);
    // +1: the real project-level Purpose/Architecture-style call `describe_project` always
    // attempts once, regardless of scope — real, but a single flat call, not scaled by workspace
    // size the way the per-object counts are.
    let total = modules + symbols + 1;
    if modules + symbols == 0 {
        return Ok(ekos_recovery::DescriptionStats::default());
    }

    println!(
        "LLM description requested for up to {total} real call(s) ({modules} module(s)/subsystem(s), \
         {symbols} symbol(s), 1 project-level summary) — real cost, some may be skipped via caching."
    );
    if !confirm_description_spend(yes)? {
        println!("Skipped (not confirmed).");
        return Ok(ekos_recovery::DescriptionStats::default());
    }

    let llm = select_llm_provider_for_description(config, &config.artifact_dir(cwd))?;
    let redaction = config.redaction_config();
    let stats = ekos_recovery::describe_objects(ledger, &*llm, scope, cwd, &redaction)
        .await
        .map_err(|e| anyhow::anyhow!("LLM description failed: {e}"))?;

    // Best-effort: a failed project-level summary shouldn't fail the whole `commit` run when the
    // real per-object work above already succeeded. `cwd`'s own directory name is a real, concrete
    // anchor for the LLM prompt (RFC 0088 fast-follow, 2026-08-24 — see `describe_project`'s own
    // doc comment for the real self-reference failure this fixes).
    let workspace_name = cwd
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "workspace".to_string());
    if let Err(e) = ekos_recovery::describe_project(ledger, &*llm, &workspace_name).await {
        eprintln!("warning: project-level LLM summary failed (skipped): {e}");
    }

    Ok(stats)
}

/// RFC 0125: the opt-in post-`commit` embed pass. No spend prompt — embeddings are cheap and
/// cached, unlike the `[llm-description]` generation calls. Vector search is a `FactLedger`-only
/// feature (`Ledger`/SQLite has no vector arm), and single-node this phase, so this is a no-op on
/// a SQLite or partitioned workspace.
async fn run_embed(
    config: &EkosConfig,
    cwd: &Path,
    ledger: &dyn KnowledgeStore,
) -> Result<Option<ekos_recovery::EmbedStats>> {
    use super::store::{facts_dir, uses_fact_engine};

    if config.storage.partition.is_enabled() {
        eprintln!(
            "note: [embeddings] is single-node this phase — skipped on a partitioned workspace"
        );
        return Ok(None);
    }
    if !uses_fact_engine(config, cwd) {
        eprintln!("note: [embeddings] needs the fact engine — skipped on a SQLite workspace");
        return Ok(None);
    }

    let Some(provider) = build_embedding_provider(config, &config.artifact_dir(cwd))? else {
        return Ok(None);
    };
    let index_dir = facts_dir(config, cwd).join("vectors");
    let redaction = config.redaction_config();
    let stats = ekos_recovery::embed_objects(ledger, &*provider, &index_dir, &redaction)
        .await
        .map_err(|e| anyhow::anyhow!("embedding pass failed: {e}"))?;
    Ok(Some(stats))
}

/// Embed one query string for the retrieval vector arm (`ekos query find --mode vector`, MCP
/// `ekos_search {mode}`). Bridges the sync call site into the async provider the same way
/// `run_clickhouse_query_blocking` does. Errors clearly when `[embeddings]` is not configured.
pub(crate) fn embed_query_blocking(
    config: &EkosConfig,
    cwd: &Path,
    text: &str,
) -> Result<Vec<f32>> {
    let provider = build_embedding_provider(config, &config.artifact_dir(cwd))?.ok_or_else(|| {
        anyhow::anyhow!(
            "vector search needs `[embeddings]` enabled in ekos.toml (and `ekos commit` re-run to \
             build the index)"
        )
    })?;
    let texts = [text.to_string()];
    let fut = async move { provider.embed(&texts).await };
    let vecs = match tokio::runtime::Handle::try_current() {
        Ok(handle) => tokio::task::block_in_place(|| handle.block_on(fut)),
        Err(_) => tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?
            .block_on(fut),
    }
    .map_err(|e| anyhow::anyhow!("embedding the query failed: {e}"))?;
    vecs.into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("embedding provider returned no vector"))
}

/// Mirrors `select_llm_provider_for_description` for `[embeddings]`. `None` when the table is
/// absent/disabled; provider kind falls back to `[llm] provider`, then `"mock"` (offline).
pub(crate) fn build_embedding_provider(
    config: &EkosConfig,
    artifact_dir: &Path,
) -> Result<Option<std::sync::Arc<dyn ekos_recovery::EmbeddingProvider>>> {
    use ekos_recovery::{
        CachedEmbeddingProvider, EmbeddingProvider, MockEmbeddingProvider, OllamaEmbeddingProvider,
        OpenAiEmbeddingProvider,
    };
    use std::sync::Arc;

    let ec = &config.embeddings;
    if !ec.enabled {
        return Ok(None);
    }
    let kind = ec
        .provider
        .as_deref()
        .or(config.llm.provider.as_deref())
        .unwrap_or("mock");
    let base: Arc<dyn EmbeddingProvider> = match kind {
        "mock" => Arc::new(MockEmbeddingProvider::default()),
        "ollama" => Arc::new(OllamaEmbeddingProvider::from_env(ec.model.clone())),
        "openai" => {
            let key_env = ec.api_key_env.as_deref().unwrap_or("OPENAI_API_KEY");
            Arc::new(
                OpenAiEmbeddingProvider::from_env(ec.model.clone(), key_env).map_err(|_| {
                    anyhow::anyhow!(
                        "{key_env} not set — [embeddings] provider = \"openai\" needs it"
                    )
                })?,
            )
        }
        other => anyhow::bail!("unknown [embeddings] provider {other:?} (want mock/ollama/openai)"),
    };
    if ec.cache {
        let cache_dir = artifact_dir
            .parent()
            .unwrap_or(artifact_dir)
            .join("embed-cache");
        std::fs::create_dir_all(&cache_dir).ok();
        Ok(Some(Arc::new(CachedEmbeddingProvider::new(
            base, cache_dir,
        ))))
    } else {
        Ok(Some(base))
    }
}

/// Same shape as `docs.rs::confirm_prose_spend`.
fn confirm_description_spend(auto: bool) -> Result<bool> {
    if auto {
        return Ok(true);
    }
    print!("Proceed with these LLM call(s)? [y/N]: ");
    std::io::stdout().flush().ok();
    let mut line = String::new();
    if std::io::stdin().lock().read_line(&mut line)? == 0 {
        return Ok(false);
    }
    Ok(matches!(line.trim().to_lowercase().as_str(), "y" | "yes"))
}

/// Same shape as `docs.rs::select_llm_provider_for_prose` — no degraded mode: an opt-in LLM step
/// with no real API access should fail clearly, not silently produce nonsense output.
fn select_llm_provider_for_description(
    config: &EkosConfig,
    artifact_dir: &Path,
) -> Result<std::sync::Arc<dyn ekos_recovery::LlmProvider>> {
    use ekos_recovery::{AnthropicProvider, CachedLlmProvider, OllamaProvider, OpenAiProvider};

    let cache_dir = artifact_dir
        .parent()
        .unwrap_or(artifact_dir)
        .join("llm-cache");
    std::fs::create_dir_all(&cache_dir).ok();

    if config.llm.provider.as_deref() == Some("ollama") {
        // `from_env_with_model`, not `from_env` — found live (this session's own local-Ollama
        // verification run): `from_env` silently ignores `[llm].model` and always uses the
        // built-in `llama3.1:8b` default, the same real, pre-existing gap `recover.rs` already
        // fixed for its own Ollama call site but `docs.rs`/`marketing.rs` still have.
        return Ok(std::sync::Arc::new(CachedLlmProvider::new(
            OllamaProvider::from_env_with_model(config.llm.model.as_deref()),
            cache_dir,
        )));
    }

    let key_env = config
        .llm
        .api_key_env
        .as_deref()
        .unwrap_or("ANTHROPIC_API_KEY");

    // Mirror `recover.rs::build_llm_provider` — `[llm] provider = "openai"` must route here too,
    // not silently fall through to Anthropic (which then 401s on an OpenAI key).
    if config.llm.provider.as_deref() == Some("openai") {
        let provider = OpenAiProvider::from_env_var(key_env).map_err(|_| {
            anyhow::anyhow!(
                "{key_env} not set — [llm] provider = \"openai\" needs it for [llm-description]"
            )
        })?;
        return Ok(std::sync::Arc::new(CachedLlmProvider::new(
            provider, cache_dir,
        )));
    }

    let provider = AnthropicProvider::from_env_var(key_env).map_err(|_| {
        anyhow::anyhow!(
            "{key_env} not set and no [llm] provider = \"ollama\"/\"openai\" configured in \
             ekos.toml — an LLM is required for [llm-description]"
        )
    })?;
    Ok(std::sync::Arc::new(CachedLlmProvider::new(
        provider, cache_dir,
    )))
}

fn open_ledger(config: &EkosConfig, cwd: &Path) -> Result<Box<dyn KnowledgeStore>> {
    open_store(config, cwd)
}

fn ckm_rel_to_kir(rel: &CkmRelationship) -> KirRelationship {
    use chrono::Utc;
    KirRelationship {
        id: rel.id,
        kind: rel.kind.clone(),
        from: rel.from,
        to: rel.to,
        properties: rel.properties.clone(),
        evidence: rel.evidence.iter().map(|e| e.id).collect(),
        created_at: Utc::now(),
        // CKM relationships don't carry temporal validity yet (RFC 0047 scoped this to
        // KIR/ledger/runtime, not the semantic compiler) — always unbounded for now.
        valid_from: None,
        valid_until: None,
    }
}

fn ckm_object_to_kir(obj: &ekos_semantic::CkmObject) -> KirObject {
    use chrono::Utc;
    KirObject {
        id: obj.id,
        name: obj.name.clone(),
        kind: obj.kind.clone(),
        properties: obj.properties.clone(),
        evidence: obj.evidence.iter().map(|e| e.id).collect(),
        created_at: Utc::now(),
    }
}

fn evidence_record_to_kir(ev: &EvidenceRecord) -> KirEvidence {
    use chrono::Utc;
    KirEvidence {
        id: ev.id,
        location: SourceLocation::file(ev.source.clone()),
        fragment: ev.fragment.clone(),
        confidence: ev.confidence,
        created_at: Utc::now(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_kir::{KirId, ObjectKind};
    use ekos_ledger::FactLedger;

    fn role_claim(id: KirId, crate_name: &str, value: &str) -> KirObject {
        let mut o = KirObject::new(crate_name, ObjectKind::Custom("Claim".to_string()))
            .with_property("predicate", serde_json::json!("has_role"))
            .with_property("value", serde_json::json!(value));
        o.id = id;
        o
    }

    #[test]
    fn a_reviewed_claim_keeps_its_status_and_writes_no_new_version_when_value_is_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let ledger = FactLedger::open(dir.path()).unwrap();
        let id = KirId::new();

        let mut reviewed = role_claim(id, "ekos-cli", "CLI entrypoint");
        reviewed
            .properties
            .insert("review_status".into(), serde_json::json!("confirmed"));
        reviewed.properties.insert(
            "reviewed_at".into(),
            serde_json::json!("2026-08-26T00:00:00Z"),
        );
        ledger.append_object(&reviewed).unwrap();

        // A fresh re-derivation from the reasoning pass — same role value, no review_status at all.
        let mut fresh = role_claim(id, "ekos-cli", "CLI entrypoint");
        preserve_claim_review_status(&ledger, &mut fresh).unwrap();

        assert_eq!(
            fresh.properties.get("review_status"),
            Some(&serde_json::json!("confirmed")),
            "review status must be carried forward onto the fresh object"
        );
        let wrote_new_version = ledger.append_object(&fresh).unwrap();
        assert!(
            !wrote_new_version,
            "an unchanged, already-reviewed claim must not gain a new version on re-commit"
        );
        let current = ledger.get_object(&id).unwrap().unwrap();
        assert_eq!(
            current.properties.get("review_status"),
            Some(&serde_json::json!("confirmed"))
        );
    }

    #[test]
    fn a_changed_role_value_does_not_inherit_the_old_review_status() {
        let dir = tempfile::tempdir().unwrap();
        let ledger = FactLedger::open(dir.path()).unwrap();
        let id = KirId::new();

        let mut reviewed = role_claim(id, "ekos-cli", "CLI entrypoint");
        reviewed
            .properties
            .insert("review_status".into(), serde_json::json!("confirmed"));
        ledger.append_object(&reviewed).unwrap();

        // The reasoning pass reclassified the crate — a genuinely different assertion.
        let mut reclassified = role_claim(id, "ekos-cli", "core library");
        preserve_claim_review_status(&ledger, &mut reclassified).unwrap();

        assert_eq!(
            reclassified.properties.get("review_status"),
            None,
            "a changed role value must not inherit the previous claim's review status"
        );
        let wrote_new_version = ledger.append_object(&reclassified).unwrap();
        assert!(
            wrote_new_version,
            "a genuinely changed claim must write a new version"
        );
    }

    #[test]
    fn a_brand_new_claim_with_no_prior_version_is_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let ledger = FactLedger::open(dir.path()).unwrap();
        let mut fresh = role_claim(KirId::new(), "ekos-cli", "CLI entrypoint");

        preserve_claim_review_status(&ledger, &mut fresh).unwrap();

        assert_eq!(fresh.properties.get("review_status"), None);
    }

    #[test]
    fn a_non_claim_object_is_left_completely_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let ledger = FactLedger::open(dir.path()).unwrap();
        let mut table = KirObject::new("orders", ObjectKind::Table);
        let original = table.clone();

        preserve_claim_review_status(&ledger, &mut table).unwrap();

        assert_eq!(table.properties, original.properties);
    }
}
