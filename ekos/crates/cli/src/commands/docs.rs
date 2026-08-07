//! `ekos docs generate` — deterministic documentation generation (RFC 0035).
//!
//! Reads every "significant" already-committed object from the ledger
//! ([`ekos_docs_gen::is_significant`] — every kind except `Column`, which stays embedded in its
//! parent's page) and renders one page per object — each with a 1-hop Mermaid dependency diagram
//! — plus a whole-workspace ER diagram (`er-diagram.*`, when there's at least one `ForeignKey`
//! relationship between two `Table` objects) and an `index.*` linking everything, grouped by kind.
//! `--format md` (default) and `--format html` render the exact same
//! [`ekos_docs_gen::ObjectPageModel`] through different renderers (RFC 0035 Phase 4) — pure
//! rendering over compiled data either way, zero LLM calls, zero cost.
//!
//! `--prose` (opt-in, RFC 0035 Phase 5) adds an LLM-written "## Overview" to each page by
//! reusing `AiRuntime::ask` — the exact same grounding+citation-validation pipeline `ekos ask`
//! uses, not a reimplementation — so a prose section can never cite evidence that doesn't exist.
//! A rough token-cost estimate is shown and must be confirmed (or `--yes`) before any LLM call.

use super::ask::ai_config;
use super::store::open_store;
use anyhow::{Context, Result};
use ekos_compiler_core::EkosConfig;
use ekos_docs_gen::{ObjectPageModel, ProseSection, RenderedPage};
use ekos_kir::{KirObject, KirRelationship, ObjectKind, RelationshipKind};
use ekos_runtime::{AiRuntime, Runtime};
use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

/// Output format for `ekos docs generate`. `--format md`/`--format html` on the CLI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Markdown,
    Html,
}

impl Format {
    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "md" | "markdown" => Ok(Format::Markdown),
            "html" => Ok(Format::Html),
            other => Err(anyhow::anyhow!(
                "unknown --format '{other}' — expected 'md' or 'html'"
            )),
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn generate(
    config: &EkosConfig,
    cwd: &Path,
    output: &Path,
    format: Format,
    prose: bool,
    yes: bool,
) -> Result<()> {
    let ledger = open_store(config, cwd).map_err(|e| {
        anyhow::anyhow!(
            "{e}\nRun `ekos build && ekos recover && ekos resolve && ekos compile && ekos commit` first."
        )
    })?;

    let objects = ledger.all_objects()?;
    let documented: Vec<_> = objects
        .iter()
        .filter(|o| ekos_docs_gen::is_significant(&o.kind))
        .collect();

    // Every object's name, so a relationship's other endpoint renders as
    // e.g. `orders` instead of a raw id — cheap, since names are already
    // in the `all_objects()` result, no extra ledger reads needed.
    let object_names: HashMap<_, _> = objects.iter().map(|o| (o.id, o.name.clone())).collect();

    std::fs::create_dir_all(output)
        .with_context(|| format!("cannot create output dir {}", output.display()))?;

    // Pass 1: build every model (kind, name, and file name all live on the model already, so the
    // original `KirObject`s aren't needed past this point except to drive the loop and re-fetch
    // relationships/evidence).
    let mut models: Vec<ObjectPageModel> = Vec::with_capacity(documented.len());
    for object in &documented {
        let relationships = ledger.relationships_for(&object.id)?;

        let mut evidence = Vec::new();
        for id in object
            .evidence
            .iter()
            .chain(relationships.iter().flat_map(|r| r.evidence.iter()))
        {
            if let Some(ev) = ledger.get_evidence(id)? {
                evidence.push(ev);
            }
        }

        models.push(ekos_docs_gen::build_object_page_model(
            object,
            &relationships,
            &evidence,
            &object_names,
        ));
    }

    // Pass 2 (opt-in): layer an LLM-written "## Overview" onto each model, reusing
    // `AiRuntime::ask` — never a new prompt pipeline (RFC 0035 Phase 5's explicit design).
    if prose && !models.is_empty() {
        let estimated_tokens: usize = models.iter().map(estimate_prompt_tokens).sum();
        println!(
            "Prose generation requested for {} page(s). Estimated input tokens: ~{estimated_tokens} \
             (rough estimate from each page's compiled content, not a provider-verified count).",
            models.len()
        );
        if !confirm_prose_spend(yes)? {
            println!("Skipped — writing deterministic pages only.");
        } else {
            let llm = select_llm_provider_for_prose(config, &config.artifact_dir(cwd))?;
            let runtime = Runtime::over(&*ledger);
            let ai = AiRuntime::new(&runtime, llm, ai_config(config));
            enrich_with_prose(&mut models, &ai).await;
        }
    }

    // Pass 3: render + write.
    let mut index_entries = Vec::new();
    let mut written = 0usize;
    for model in &models {
        let page = match format {
            Format::Markdown => ekos_docs_gen::render_markdown_object_page(model),
            Format::Html => ekos_docs_gen::render_html_object_page(model),
        };
        write_page(output, &page)?;
        index_entries.push((model.kind.clone(), model.name.clone(), page.file_name));
        written += 1;
    }

    // Whole-workspace ER diagram: needs every relationship (not just per-object), so it's built
    // once here rather than inside the per-object loop above.
    let table_objects: Vec<KirObject> = objects
        .iter()
        .filter(|o| o.kind == ObjectKind::Table)
        .cloned()
        .collect();
    let mut diagrams = Vec::new();
    if !table_objects.is_empty() {
        let all_relationships: Vec<KirRelationship> = ledger.all_relationships()?;
        let has_foreign_key = all_relationships
            .iter()
            .any(|r| matches!(r.kind, RelationshipKind::ForeignKey));
        if has_foreign_key {
            let er_page = render_er_diagram_page(&table_objects, &all_relationships, format);
            write_page(output, &er_page)?;
            diagrams.push(("Entity-Relationship Diagram".to_string(), er_page.file_name));
        }
    }

    let index = match format {
        Format::Markdown => ekos_docs_gen::render_index_page(&index_entries, &diagrams),
        Format::Html => ekos_docs_gen::render_html_index_page(&index_entries, &diagrams),
    };
    write_page(output, &index)?;

    println!("Documentation generated.");
    println!(
        "  Format: {}",
        if format == Format::Html { "html" } else { "md" }
    );
    println!("  Objects rendered: {written}");
    println!("  Diagrams rendered: {}", diagrams.len());
    println!("  Output: {}", output.display());
    if written == 0 {
        println!("  (No documented objects found in the ledger — nothing to render yet.)");
    }

    Ok(())
}

/// Calls `AiRuntime::ask` once per model, asking for a short overview, and sets `model.prose` on
/// success. A per-model failure (network error, rate limit, etc.) logs a warning and leaves that
/// one model's deterministic content untouched rather than aborting the whole run — one bad LLM
/// call shouldn't cost every other page its content. Separated from `generate` so it's directly
/// testable against a `MockLlmProvider`-backed `AiRuntime` without needing real credentials.
///
/// The question passed to `ask` is just `model.name`, not a full instruction sentence — found by
/// testing, not assumed: `ask`'s own retrieval step (`Runtime::find_objects`) is keyword/name
/// search, the same "2-3 keywords, not natural-language questions" constraint the `ekos_search`
/// MCP tool is documented to have elsewhere in this project. A full sentence like "Write an
/// overview of X: what it is and how..." buries the name deep enough that retrieval can return
/// zero matches, which empties `ask`'s `known_evidence` set and silently strips *every* citation
/// from the response, including genuinely valid ones — caught by a real integration test with a
/// real evidence id, not assumed from reading `ai.rs`'s source.
async fn enrich_with_prose(models: &mut [ObjectPageModel], ai: &AiRuntime<'_>) {
    for model in models {
        match ai.ask(&model.name).await {
            Ok(answer) => {
                model.prose = Some(ProseSection {
                    text: answer.answer,
                    cited_evidence: answer.evidence_refs,
                })
            }
            Err(e) => eprintln!(
                "warning: prose generation failed for {} — keeping deterministic content only: {e}",
                model.name
            ),
        }
    }
}

/// Rough proxy for how many tokens one `--prose` call for `model` will cost: the same compiled
/// content (`AiRuntime::ask`'s own grounding pulls this same object's properties, relationships,
/// and evidence) at ~4 chars/token, plus a flat overhead for the system prompt and JSON framing.
/// Explicitly a rough estimate, not a provider-verified count — shown as one before any spend,
/// per RFC 0035 Phase 5.
fn estimate_prompt_tokens(model: &ObjectPageModel) -> usize {
    let mut chars = model.name.len();
    for (key, value) in &model.properties {
        chars += key.len() + value.len();
    }
    for (kind, rows) in &model.relationship_groups {
        for row in rows {
            chars += kind.len() + row.other_name.as_deref().unwrap_or("").len() + 40;
        }
    }
    for row in &model.evidence {
        if let Some((fragment, _)) = &row.resolved {
            chars += fragment.len();
        }
    }
    chars / 4 + 250
}

/// Interactive confirmation before spending on LLM calls, mirroring `ekos marketing publish`'s
/// `approve` (RFC 0030): `--yes` skips straight to "proceed", otherwise reads a line from stdin;
/// EOF (non-interactive invocation with no answer) is treated as "no" rather than hanging.
fn confirm_prose_spend(auto: bool) -> Result<bool> {
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

/// Selects an LLM provider the same way `ekos marketing publish` does (RFC 0030's
/// `select_llm_provider`) — errors out clearly when no provider is usable rather than silently
/// falling back to `recover`'s mock ("structural analysis only") degraded mode. There is no valid
/// degraded mode for `--prose`: a mock response would just produce nonsense "Overview" text
/// instead of a clear "no API key" error, and `--prose` is explicitly opt-in — a user who asked
/// for it wants real output or an honest failure, not silent placeholder prose.
fn select_llm_provider_for_prose(
    config: &EkosConfig,
    artifact_dir: &Path,
) -> Result<std::sync::Arc<dyn ekos_recovery::LlmProvider>> {
    use ekos_recovery::{AnthropicProvider, CachedLlmProvider, OllamaProvider};

    let cache_dir = artifact_dir
        .parent()
        .unwrap_or(artifact_dir)
        .join("llm-cache");
    std::fs::create_dir_all(&cache_dir).ok();

    if config.llm.provider.as_deref() == Some("ollama") {
        return Ok(std::sync::Arc::new(CachedLlmProvider::new(
            OllamaProvider::from_env(),
            cache_dir,
        )));
    }

    let key_env = config
        .llm
        .api_key_env
        .as_deref()
        .unwrap_or("ANTHROPIC_API_KEY");
    let provider = AnthropicProvider::from_env_var(key_env).map_err(|_| {
        anyhow::anyhow!(
            "{key_env} not set and no [llm] provider = \"ollama\" configured in ekos.toml — \
             an LLM is required for --prose"
        )
    })?;
    Ok(std::sync::Arc::new(CachedLlmProvider::new(
        provider, cache_dir,
    )))
}

fn render_er_diagram_page(
    tables: &[KirObject],
    relationships: &[KirRelationship],
    format: Format,
) -> RenderedPage {
    match format {
        Format::Markdown => {
            let er = ekos_docs_gen::render_er_diagram(tables, relationships);
            RenderedPage {
                file_name: "er-diagram.md".to_string(),
                content: format!("# Entity-Relationship Diagram\n\n{er}"),
            }
        }
        Format::Html => ekos_docs_gen::render_html_er_diagram_page(tables, relationships),
    }
}

fn write_page(output: &Path, page: &RenderedPage) -> Result<()> {
    let path = output.join(&page.file_name);
    std::fs::write(&path, &page.content).with_context(|| format!("cannot write {}", path.display()))
}

/// Resolve the output directory: the `--output` flag if given, else `<cwd>/docs-generated`.
pub fn resolve_output_dir(cwd: &Path, output: Option<PathBuf>) -> PathBuf {
    output.unwrap_or_else(|| cwd.join("docs-generated"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_kir::{
        KirEvidence, KirObject, KirRelationship, ObjectKind, RelationshipKind, SourceLocation,
    };
    use ekos_ledger::Ledger;
    use tempfile::tempdir;

    #[tokio::test]
    async fn generate_renders_one_page_per_table() {
        let dir = tempdir().unwrap();
        let config = EkosConfig::default();
        let ledger = Ledger::open(&config.ledger_path(dir.path())).unwrap();

        let customers = KirObject::new("customers", ObjectKind::Table);
        let orders = KirObject::new("orders", ObjectKind::Table);
        ledger.append_object(&customers).unwrap();
        ledger.append_object(&orders).unwrap();

        let ev = KirEvidence::new(
            SourceLocation::file("schema.sql"),
            "FOREIGN KEY (customer_id)",
        );
        ledger.append_evidence(&ev).unwrap();
        let mut rel = KirRelationship::new(RelationshipKind::ForeignKey, orders.id, customers.id);
        rel.evidence.push(ev.id);
        ledger.append_relationship(&rel).unwrap();

        let output = dir.path().join("out");
        generate(&config, dir.path(), &output, Format::Markdown, false, false)
            .await
            .unwrap();

        assert!(output.join("table-customers.md").exists());
        assert!(output.join("table-orders.md").exists());
        let orders_content = std::fs::read_to_string(output.join("table-orders.md")).unwrap();
        assert!(orders_content.contains("### ForeignKey"));
        assert!(orders_content.contains("FOREIGN KEY (customer_id)"));
        assert!(
            orders_content.contains("```mermaid"),
            "object page embeds a diagram"
        );
    }

    #[tokio::test]
    async fn generate_writes_er_diagram_and_links_it_from_the_index_when_a_foreign_key_exists() {
        let dir = tempdir().unwrap();
        let config = EkosConfig::default();
        let ledger = Ledger::open(&config.ledger_path(dir.path())).unwrap();

        let customers = KirObject::new("customers", ObjectKind::Table);
        let orders = KirObject::new("orders", ObjectKind::Table);
        ledger.append_object(&customers).unwrap();
        ledger.append_object(&orders).unwrap();
        let rel = KirRelationship::new(RelationshipKind::ForeignKey, orders.id, customers.id);
        ledger.append_relationship(&rel).unwrap();

        let output = dir.path().join("out");
        generate(&config, dir.path(), &output, Format::Markdown, false, false)
            .await
            .unwrap();

        let er = std::fs::read_to_string(output.join("er-diagram.md")).unwrap();
        assert!(er.contains("erDiagram"));
        assert!(er.contains("\"orders\" }o--|| \"customers\""));

        let index = std::fs::read_to_string(output.join("index.md")).unwrap();
        assert!(index.contains("[Entity-Relationship Diagram](er-diagram.md)"));
    }

    #[tokio::test]
    async fn generate_skips_er_diagram_when_no_foreign_keys_exist() {
        let dir = tempdir().unwrap();
        let config = EkosConfig::default();
        let ledger = Ledger::open(&config.ledger_path(dir.path())).unwrap();
        ledger
            .append_object(&KirObject::new("customers", ObjectKind::Table))
            .unwrap();

        let output = dir.path().join("out");
        generate(&config, dir.path(), &output, Format::Markdown, false, false)
            .await
            .unwrap();

        assert!(!output.join("er-diagram.md").exists());
        let index = std::fs::read_to_string(output.join("index.md")).unwrap();
        assert!(!index.contains("## Diagrams"));
    }

    #[tokio::test]
    async fn generate_renders_every_significant_kind_and_excludes_column() {
        let dir = tempdir().unwrap();
        let config = EkosConfig::default();
        let ledger = Ledger::open(&config.ledger_path(dir.path())).unwrap();
        ledger
            .append_object(&KirObject::new("main.rs", ObjectKind::File))
            .unwrap();
        ledger
            .append_object(&KirObject::new(
                "fact_sales",
                ObjectKind::Custom("TransformNode".into()),
            ))
            .unwrap();
        ledger
            .append_object(&KirObject::new("id", ObjectKind::Column))
            .unwrap();

        let output = dir.path().join("out");
        generate(&config, dir.path(), &output, Format::Markdown, false, false)
            .await
            .unwrap();

        assert!(output.join("file-main-rs.md").exists());
        assert!(output.join("transformnode-fact-sales.md").exists());
        assert!(
            !output.join("column-id.md").exists(),
            "Column objects stay embedded in their parent's page, not a page of their own"
        );
    }

    #[tokio::test]
    async fn generate_always_writes_an_index_page() {
        let dir = tempdir().unwrap();
        let config = EkosConfig::default();
        let ledger = Ledger::open(&config.ledger_path(dir.path())).unwrap();
        ledger
            .append_object(&KirObject::new("customers", ObjectKind::Table))
            .unwrap();

        let output = dir.path().join("out");
        generate(&config, dir.path(), &output, Format::Markdown, false, false)
            .await
            .unwrap();

        let index = std::fs::read_to_string(output.join("index.md")).unwrap();
        assert!(index.contains("## Table (1)"));
        assert!(index.contains("[customers](table-customers.md)"));
    }

    #[tokio::test]
    async fn generate_on_empty_ledger_still_writes_an_honest_index() {
        let dir = tempdir().unwrap();
        let config = EkosConfig::default();
        let _ledger = Ledger::open(&config.ledger_path(dir.path())).unwrap();

        let output = dir.path().join("out");
        generate(&config, dir.path(), &output, Format::Markdown, false, false)
            .await
            .unwrap();

        let index = std::fs::read_to_string(output.join("index.md")).unwrap();
        assert!(index.contains("No documented objects yet"));
    }

    #[test]
    fn resolve_output_dir_defaults_to_docs_generated() {
        let cwd = Path::new("/workspace");
        assert_eq!(
            resolve_output_dir(cwd, None),
            Path::new("/workspace/docs-generated")
        );
        assert_eq!(
            resolve_output_dir(cwd, Some(PathBuf::from("/custom"))),
            Path::new("/custom")
        );
    }

    #[test]
    fn format_parse_accepts_md_markdown_and_html_rejects_unknown() {
        assert_eq!(Format::parse("md").unwrap(), Format::Markdown);
        assert_eq!(Format::parse("markdown").unwrap(), Format::Markdown);
        assert_eq!(Format::parse("html").unwrap(), Format::Html);
        assert!(Format::parse("pdf").is_err());
    }

    #[tokio::test]
    async fn generate_html_format_writes_html_pages_index_and_er_diagram() {
        let dir = tempdir().unwrap();
        let config = EkosConfig::default();
        let ledger = Ledger::open(&config.ledger_path(dir.path())).unwrap();

        let customers = KirObject::new("customers", ObjectKind::Table);
        let orders = KirObject::new("orders", ObjectKind::Table);
        ledger.append_object(&customers).unwrap();
        ledger.append_object(&orders).unwrap();
        let rel = KirRelationship::new(RelationshipKind::ForeignKey, orders.id, customers.id);
        ledger.append_relationship(&rel).unwrap();

        let output = dir.path().join("out");
        generate(&config, dir.path(), &output, Format::Html, false, false)
            .await
            .unwrap();

        assert!(output.join("table-customers.html").exists());
        assert!(output.join("table-orders.html").exists());
        assert!(output.join("er-diagram.html").exists());
        assert!(output.join("index.html").exists());
        assert!(!output.join("table-customers.md").exists());

        let orders_content = std::fs::read_to_string(output.join("table-orders.html")).unwrap();
        assert!(orders_content.starts_with("<!doctype html>"));
        assert!(orders_content.contains("ForeignKey"));

        let index = std::fs::read_to_string(output.join("index.html")).unwrap();
        assert!(index.contains("href=\"er-diagram.html\""));
        assert!(index.contains("href=\"table-orders.html\""));
    }

    // ── Phase 5 — prose (--prose) ────────────────────────────────────────────

    #[test]
    fn estimate_prompt_tokens_grows_with_model_content() {
        let table = KirObject::new("customers", ObjectKind::Table);
        let empty_model = ekos_docs_gen::build_object_page_model(&table, &[], &[], &HashMap::new());

        let table_with_props = KirObject::new("customers", ObjectKind::Table).with_property(
            "columns",
            serde_json::json!([{"name": "id", "data_type": "int"}]),
        );
        let fuller_model =
            ekos_docs_gen::build_object_page_model(&table_with_props, &[], &[], &HashMap::new());

        assert!(estimate_prompt_tokens(&fuller_model) > estimate_prompt_tokens(&empty_model));
        assert!(
            estimate_prompt_tokens(&empty_model) > 0,
            "flat overhead is never zero"
        );
    }

    #[test]
    fn confirm_prose_spend_auto_skips_the_prompt() {
        assert!(confirm_prose_spend(true).unwrap());
    }

    #[tokio::test]
    async fn enrich_with_prose_sets_model_prose_on_success() {
        let dir = tempdir().unwrap();
        let ledger = Ledger::open(&dir.path().join("ledger.db")).unwrap();
        let table = KirObject::new("customers", ObjectKind::Table);
        ledger.append_object(&table).unwrap();

        let runtime = ekos_runtime::Runtime::new(&ledger);
        let llm = std::sync::Arc::new(ekos_recovery::MockLlmProvider::new(
            r#"Customers holds one row per account. {"cited_evidence": []}"#,
        ));
        let ai = AiRuntime::new(&runtime, llm, ekos_runtime::AiRuntimeConfig::default());

        let mut models = vec![ekos_docs_gen::build_object_page_model(
            &table,
            &[],
            &[],
            &HashMap::new(),
        )];
        enrich_with_prose(&mut models, &ai).await;

        let prose = models[0].prose.as_ref().expect("prose should be set");
        assert!(prose.text.contains("Customers holds one row per account."));
    }

    #[tokio::test]
    async fn enrich_with_prose_only_cites_evidence_that_actually_resolved() {
        let dir = tempdir().unwrap();
        let ledger = Ledger::open(&dir.path().join("ledger.db")).unwrap();
        let ev = KirEvidence::new(SourceLocation::file("schema.sql"), "CREATE TABLE customers");
        let ev_id = ev.id;
        ledger.append_evidence(&ev).unwrap();
        let mut table = KirObject::new("customers", ObjectKind::Table);
        table.evidence.push(ev_id);
        ledger.append_object(&table).unwrap();

        let runtime = ekos_runtime::Runtime::new(&ledger);
        let bogus_id = ekos_kir::KirId::new();
        let llm = std::sync::Arc::new(ekos_recovery::MockLlmProvider::new(format!(
            r#"Customers table. {{"cited_evidence": ["{ev_id}", "{bogus_id}"]}}"#
        )));
        let ai = AiRuntime::new(&runtime, llm, ekos_runtime::AiRuntimeConfig::default());

        let mut models = vec![ekos_docs_gen::build_object_page_model(
            &table,
            &[],
            &[ev],
            &HashMap::new(),
        )];
        enrich_with_prose(&mut models, &ai).await;

        let prose = models[0].prose.as_ref().unwrap();
        assert_eq!(
            prose.cited_evidence,
            vec![ev_id],
            "a fabricated evidence id must never survive into the prose section \
             — AiRuntime::ask's own citation validation already filters it out"
        );
    }

    #[tokio::test]
    async fn generate_with_prose_errors_clearly_instead_of_silently_degrading() {
        let dir = tempdir().unwrap();
        let mut config = EkosConfig::default();
        // Point at a variable name guaranteed not to exist, regardless of what's set in the
        // real environment this test happens to run in — deterministic, and never risks an
        // accidental real network call to a real provider from the test suite.
        config.llm.api_key_env = Some("EKOS_DOCS_TEST_DEFINITELY_UNSET_KEY".to_string());
        let ledger = Ledger::open(&config.ledger_path(dir.path())).unwrap();
        ledger
            .append_object(&KirObject::new("customers", ObjectKind::Table))
            .unwrap();
        drop(ledger);

        // generate() builds its own strict (Anthropic/Ollama) provider selector for --prose,
        // which has no credentials here, so it should fail clearly rather than silently degrade
        // to mock prose — the exact behavior `select_llm_provider_for_prose`'s doc comment
        // promises.
        let output = dir.path().join("out");
        let result = generate(&config, dir.path(), &output, Format::Markdown, true, true).await;
        assert!(
            result.is_err(),
            "no API key configured — --prose must fail clearly, not silently degrade"
        );
    }
}
