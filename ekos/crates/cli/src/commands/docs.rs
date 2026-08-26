//! `ekos docs generate` — deterministic documentation generation (RFC 0035).
//!
//! Reads every "significant" already-committed object from the ledger
//! ([`ekos_docs_gen::is_significant`] — every kind except `Column`, which stays embedded in its
//! parent's page) and renders one page per object — each with a 1-hop Mermaid dependency diagram,
//! plus a standalone `<kind>-<name>.svg` counterpart when the object has at least one relationship
//! (RFC 0068 §61 follow-on, `ekos_docs_gen::render_object_neighborhood_svg`) — plus a
//! whole-workspace ER diagram (`er-diagram.*`, when there's at least one `ForeignKey` relationship
//! between two `Table` objects) and an `index.*` linking everything, grouped by kind.
//! `--format md` (default) and `--format html` render the exact same
//! [`ekos_docs_gen::ObjectPageModel`] through different renderers (RFC 0035 Phase 4) — pure
//! rendering over compiled data either way, zero LLM calls, zero cost.
//!
//! `--prose` (opt-in, RFC 0035 Phase 5) adds an LLM-written "## Overview" to each page by
//! reusing `AiRuntime::ask` — the exact same grounding+citation-validation pipeline `ekos ask`
//! uses, not a reimplementation — so a prose section can never cite evidence that doesn't exist.
//! A rough token-cost estimate is shown and must be confirmed (or `--yes`) before any LLM call.
//!
//! `--layout curated` (RFC 0037, additive — `--layout objects` above stays the unchanged default)
//! renders a fixed four-file set instead — `README.md`, `Architecture.md`, `API.md`,
//! `SequenceDiagrams.md` — the shape a developer actually expects from a project's docs, built
//! from the same compiled objects/relationships via [`ekos_docs_gen::render_readme`] and friends.
//! Markdown only in this phase; `--format html --layout curated` errors clearly rather than
//! silently ignoring `--format`.
//!
//! `--layout solution-architect` (RFC 0090) renders a different fixed three-file set —
//! `DependencyRiskReport.md`, `OnboardingGuide.md`, `FindingsMemo.md` — framed for a team
//! handoff rather than architecture reference. The first two are always deterministic/zero-LLM;
//! the findings memo's candidate list is too, but reuses the existing `--prose`/`--yes` flow (not
//! a new flag) to optionally layer an LLM-written executive summary on top via
//! [`enrich_findings_memo`], never replacing the deterministic list underneath it.

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

/// Output layout for `ekos docs generate`. `--layout objects` (default, RFC 0035) is one page per
/// significant object; `--layout curated` (RFC 0037) is the fixed four-file
/// README/Architecture/API/SequenceDiagrams set; `--layout solution-architect` (RFC 0090) is a
/// three-file team-facing bundle — DependencyRiskReport/OnboardingGuide/FindingsMemo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    Objects,
    Curated,
    SolutionArchitect,
}

impl Layout {
    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "objects" => Ok(Layout::Objects),
            "curated" => Ok(Layout::Curated),
            "solution-architect" => Ok(Layout::SolutionArchitect),
            other => Err(anyhow::anyhow!(
                "unknown --layout '{other}' — expected 'objects', 'curated', or \
                 'solution-architect'"
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
    layout: Layout,
    prose: bool,
    yes: bool,
) -> Result<()> {
    if layout == Layout::Curated {
        if format != Format::Markdown {
            anyhow::bail!(
                "--layout curated only supports --format md today (HTML curated output is an \
                 open question in RFC 0037, not yet implemented)"
            );
        }
        return generate_curated(config, cwd, output);
    }
    if layout == Layout::SolutionArchitect {
        if format != Format::Markdown {
            anyhow::bail!(
                "--layout solution-architect only supports --format md today (same open \
                 question RFC 0037 left open for --layout curated's HTML output)"
            );
        }
        return generate_solution_architect(config, cwd, output, prose, yes).await;
    }

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
    // RFC 0089: real file two-plus hops up the compiled `Contains` chain (e.g. symbol -> module
    // -> file) — built once over every real relationship in the ledger, not per object.
    let objects_by_id: HashMap<_, _> = objects.iter().map(|o| (o.id, o)).collect();
    let all_relationships = ledger.all_relationships()?;
    let parent_of = ekos_docs_gen::build_contains_parent_map(&all_relationships);

    std::fs::create_dir_all(output)
        .with_context(|| format!("cannot create output dir {}", output.display()))?;

    // Pass 1: build every model (kind, name, and file name all live on the model already, so the
    // original `KirObject`s aren't needed past this point except to drive the loop and re-fetch
    // relationships/evidence).
    let mut models: Vec<ObjectPageModel> = Vec::with_capacity(documented.len());
    let mut svg_written = 0usize;
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

        let mut model = ekos_docs_gen::build_object_page_model(
            object,
            &relationships,
            &evidence,
            &object_names,
        );
        model.defined_in_file =
            ekos_docs_gen::resolve_defining_file(object.id, &parent_of, &objects_by_id)
                .and_then(|file_id| object_names.get(&file_id).cloned());
        models.push(model);

        // RFC 0068 §61 follow-on: a standalone SVG counterpart to the Mermaid diagram already
        // embedded in the page above — same node/edge data, `render_graph_svg`'s generic renderer.
        if let Some(svg) =
            ekos_docs_gen::render_object_neighborhood_svg(object, &relationships, &object_names)
        {
            write_page(output, &svg)?;
            svg_written += 1;
        }
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

            // RFC 0068 §61 follow-on: a standalone SVG counterpart, same reasoning as the
            // per-object/per-relationship-kind SVGs above.
            if let Some(svg_page) =
                ekos_docs_gen::render_er_diagram_svg(&table_objects, &all_relationships)
            {
                write_page(output, &svg_page)?;
                diagrams.push((
                    "Entity-Relationship Diagram (SVG)".to_string(),
                    svg_page.file_name,
                ));
            }
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
    println!("  Neighborhood SVGs rendered: {svg_written}");
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
    // Entity pages nest under `entities/<kind>/<shard>/` (RFC 0042, `unique_page_file_names`) to
    // stay under GitHub's per-directory listing cap — the flat `--layout objects` pages never
    // have a directory component in their name, so this is a no-op for that path.
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("cannot create directory {}", parent.display()))?;
    }
    std::fs::write(&path, &page.content).with_context(|| format!("cannot write {}", path.display()))
}

/// `--layout curated` (RFC 0037): reads the committed ledger once (`all_objects()` +
/// `all_relationships()`, no per-object evidence assembly needed — these are aggregate-level
/// documents, not per-object citation pages) and writes exactly four fixed-name files.
fn generate_curated(config: &EkosConfig, cwd: &Path, output: &Path) -> Result<()> {
    let ledger = open_store(config, cwd).map_err(|e| {
        anyhow::anyhow!(
            "{e}\nRun `ekos build && ekos recover && ekos resolve && ekos compile && ekos commit` first."
        )
    })?;

    let objects = ledger.all_objects()?;
    let relationships = ledger.all_relationships()?;

    std::fs::create_dir_all(output)
        .with_context(|| format!("cannot create output dir {}", output.display()))?;

    // RFC 0083: real per-workspace escape hatch for `## System Decomposition`'s convention-based
    // Backend/Frontend/Database classification — same `ekos.toml` → analyzer-input translation
    // shape `redaction_config`/`[recover.sql.dialect-rules]` already establish elsewhere in this
    // file's sibling commands.
    let layer_overrides: Vec<ekos_docs_gen::LayerOverride> = config
        .architecture
        .system_decomposition
        .overrides
        .iter()
        .map(|o| ekos_docs_gen::LayerOverride {
            path_glob: o.path_glob.clone(),
            layer: o.layer.clone(),
        })
        .collect();

    // RFC 0095: the same `evaluate_architecture` computation `ekos architecture investigate`
    // already used (RFC 0065 Phase 3), now also run from the plain `docs generate` path — a real,
    // deterministic completeness/evidence-coverage score, not a fabricated one.
    let evaluation = ekos_recovery::evaluate_architecture(&objects);
    let confidence = Some(ekos_docs_gen::ArchitectureConfidence {
        score: evaluation.score,
        completeness: evaluation.completeness,
        evidence_coverage: evaluation.evidence_coverage,
        crates_total: evaluation.crates_total,
        evidenced_total: evaluation.evidenced_total,
    });

    let readme = ekos_docs_gen::render_readme(&objects);
    let architecture =
        ekos_docs_gen::render_architecture(&objects, &relationships, &layer_overrides, confidence);
    let api = ekos_docs_gen::render_api(&objects, &relationships);
    let sequence_diagrams = ekos_docs_gen::render_sequence_diagrams(&objects, &relationships);

    for page in [&readme, &architecture, &api, &sequence_diagrams] {
        write_page(output, page)?;
    }

    // RFC 0073: the one remaining RFC 0068 §61 MVP item — a standalone SVG artifact alongside the
    // Mermaid-in-Markdown diagram, not just a fenced code block. Only written when there's real
    // data behind it (`None` on an empty/no-dependency workspace), matching every other
    // conditional file this function already writes (entity pages below, ER diagram elsewhere).
    if let Some(svg_page) = ekos_docs_gen::render_system_context_svg(&objects, &relationships) {
        write_page(output, &svg_page)?;
    }
    // RFC 0083: same reasoning, for the new `## System Decomposition` section.
    if let Some(svg_page) =
        ekos_docs_gen::render_system_decomposition_svg(&objects, &relationships, &layer_overrides)
    {
        write_page(output, &svg_page)?;
    }
    // RFC 0083 Phase 4: same reasoning, for `## Crate & Workspace Topology` — one of the sections
    // RFC 0073 itself deferred ("current output is Mermaid-in-Markdown only").
    if let Some(svg_page) = ekos_docs_gen::render_crate_topology_svg(&objects, &relationships) {
        write_page(output, &svg_page)?;
    }
    // RFC 0068 §61 follow-on: same reasoning, for each `## Dependency Graph` per-relationship-kind
    // subsection. `dependency_graph_groups` is the exact same eligible-kind computation
    // `render_architecture`'s own Markdown loop used to decide "real diagram" vs. "omitted, too
    // large" — reusing it here (rather than re-deriving the same filter) is what guarantees every
    // `dependency-graph-<kind>.svg` link the Markdown just emitted resolves to a file actually
    // written below.
    let name_by_id: HashMap<_, _> = objects.iter().map(|o| (o.id, o.name.as_str())).collect();
    for (kind, rels) in ekos_docs_gen::dependency_graph_groups(&relationships) {
        if let Some(svg_page) =
            ekos_docs_gen::render_relationship_kind_graph_svg(&kind, &rels, &name_by_id)
        {
            write_page(output, &svg_page)?;
        }
    }

    // RFC 0042: per-entity detail pages for the crate/program-entity/technology/pipeline kinds
    // `Architecture.md`/`API.md` now link to — reusing the exact same object-page renderer
    // `--layout objects` uses (`build_object_page_model`/`render_markdown_object_page`), not a
    // new one, so every link the curated files emit resolves to a file written in this same run.
    // File names are taken from `unique_page_file_names` (the same collision-free naming the
    // linking renderers used to build those links) rather than the plain `page_file_name` a
    // standalone `render_markdown_object_page` call would otherwise use, so two entities sharing
    // a bare name (e.g. two `fn new` in different modules) never overwrite each other on disk.
    // `is_entity_page_kind` is the single source of truth shared with `render_architecture`'s/
    // `render_api`'s own link-generation, so a link is never emitted to a page that isn't written
    // here (or vice versa).
    let object_names: HashMap<_, _> = objects.iter().map(|o| (o.id, o.name.clone())).collect();
    let unique_file_names = ekos_docs_gen::unique_page_file_names(&objects, "md");
    // RFC 0089: same real multi-hop file resolution as the `--layout objects` entity pages, built
    // once over the whole graph already loaded into `objects`/`relationships` above.
    let objects_by_id: HashMap<_, _> = objects.iter().map(|o| (o.id, o)).collect();
    let parent_of = ekos_docs_gen::build_contains_parent_map(&relationships);
    let mut entity_pages_written = 0usize;
    for object in objects
        .iter()
        .filter(|o| ekos_docs_gen::is_entity_page_kind(&o.kind))
    {
        let obj_relationships: Vec<KirRelationship> = relationships
            .iter()
            .filter(|r| r.from == object.id || r.to == object.id)
            .cloned()
            .collect();
        let mut evidence = Vec::new();
        for id in object
            .evidence
            .iter()
            .chain(obj_relationships.iter().flat_map(|r| r.evidence.iter()))
        {
            if let Some(ev) = ledger.get_evidence(id)? {
                evidence.push(ev);
            }
        }
        let mut model = ekos_docs_gen::build_object_page_model(
            object,
            &obj_relationships,
            &evidence,
            &object_names,
        );
        model.defined_in_file =
            ekos_docs_gen::resolve_defining_file(object.id, &parent_of, &objects_by_id)
                .and_then(|file_id| object_names.get(&file_id).cloned());
        let mut page = ekos_docs_gen::render_markdown_object_page(&model);
        if let Some(name) = unique_file_names.get(&object.id) {
            page.file_name = name.clone();
        }
        write_page(output, &page)?;
        entity_pages_written += 1;
    }

    println!("Curated documentation generated.");
    println!("  Objects considered: {}", objects.len());
    println!(
        "  Files: {}, {}, {}, {}",
        readme.file_name, architecture.file_name, api.file_name, sequence_diagrams.file_name
    );
    println!("  Entity detail pages written: {entity_pages_written}");
    println!("  Output: {}", output.display());

    Ok(())
}

/// `--layout solution-architect` (RFC 0090): reads the committed ledger once, same entry point
/// `generate_curated` uses, and writes three fixed-name files. `DependencyRiskReport.md`/
/// `OnboardingGuide.md` are always deterministic; the findings memo's candidate list is too, but
/// gains an LLM-written executive summary layered on top (never replacing the list) when `prose`
/// is set, reusing the exact `--prose`/`--yes` spend-confirmation flow `--layout objects` already
/// has (RFC 0090's resolved open question #2 — no new flags).
async fn generate_solution_architect(
    config: &EkosConfig,
    cwd: &Path,
    output: &Path,
    prose: bool,
    yes: bool,
) -> Result<()> {
    let ledger = open_store(config, cwd).map_err(|e| {
        anyhow::anyhow!(
            "{e}\nRun `ekos build && ekos recover && ekos resolve && ekos compile && ekos commit` first."
        )
    })?;

    let objects = ledger.all_objects()?;
    let relationships = ledger.all_relationships()?;

    std::fs::create_dir_all(output)
        .with_context(|| format!("cannot create output dir {}", output.display()))?;

    let risk_report = ekos_docs_gen::render_dependency_risk_report(&objects, &relationships);
    let onboarding_guide = ekos_docs_gen::render_onboarding_guide(&objects);
    let candidates = ekos_docs_gen::build_findings_evidence(&objects);

    let mut findings_prose = None;
    if prose && !candidates.is_empty() {
        println!(
            "Findings memo executive summary requested. {} finding(s) will be sent to the LLM.",
            candidates.len()
        );
        if !confirm_prose_spend(yes)? {
            println!("Skipped — writing deterministic findings list only.");
        } else {
            let llm = select_llm_provider_for_prose(config, &config.artifact_dir(cwd))?;
            findings_prose = enrich_findings_memo(&candidates, &*llm).await;
        }
    }
    let findings_memo = ekos_docs_gen::render_findings_memo(&candidates, findings_prose.as_ref());

    for page in [&risk_report, &onboarding_guide, &findings_memo] {
        write_page(output, page)?;
    }

    println!("Solution architect report generated.");
    println!(
        "  Files: {}, {}, {}",
        risk_report.file_name, onboarding_guide.file_name, findings_memo.file_name
    );
    println!("  Findings: {}", candidates.len());
    println!("  Output: {}", output.display());

    Ok(())
}

/// Calls the LLM once with the full deterministic findings list (RFC 0090) and asks it to
/// prioritize/phrase an executive summary. Deliberately *not* `AiRuntime::ask` (unlike
/// `enrich_with_prose` above): there's no single object name to retrieve grounding against here —
/// the grounding *is* the candidate list itself, already deterministic — so this follows
/// `llm_description.rs::describe_project`'s direct `LlmProvider::complete` pattern instead.
/// Returns `None` (never fabricated) on any failure; the deterministic candidate list in
/// `FindingsMemo.md` is unaffected either way.
async fn enrich_findings_memo(
    candidates: &[ekos_docs_gen::FindingCandidate],
    llm: &dyn ekos_recovery::LlmProvider,
) -> Option<ekos_docs_gen::FindingsProse> {
    let user_message = serde_json::json!({
        "findings": candidates
            .iter()
            .map(|c| serde_json::json!({"title": c.title, "detail": c.detail}))
            .collect::<Vec<_>>(),
    })
    .to_string();
    let system_prompt = "You are writing a short executive summary for a solution architect's \
         findings memo about a real software project. You will be shown a real, already-compiled \
         list of findings (unresolved dependencies, undeclared crate versions, missing \
         documentation coverage). Prioritize and phrase them clearly in 3-6 sentences of prose. \
         Never invent a finding that isn't in the list you were shown, and never invent numbers \
         not present in it.";
    let req = ekos_recovery::LlmRequest {
        system: system_prompt,
        user: &user_message,
        prompt_version: "solution-architect-findings-v1",
        max_tokens: 512,
        history: &[],
    };
    match llm.complete(&req).await {
        Ok(resp) => Some(ekos_docs_gen::FindingsProse { text: resp.content }),
        Err(e) => {
            eprintln!(
                "warning: findings memo executive summary generation failed — keeping \
                 deterministic findings list only: {e}"
            );
            None
        }
    }
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
        generate(
            &config,
            dir.path(),
            &output,
            Format::Markdown,
            Layout::Objects,
            false,
            false,
        )
        .await
        .unwrap();

        assert!(output.join("table-customers.md").exists());
        assert!(output.join("table-orders.md").exists());
        let orders_content = std::fs::read_to_string(output.join("table-orders.md")).unwrap();
        // Real relationships group by real structural meaning now (RFC 0088 Phase 2), not raw
        // kind — a real outgoing, non-`Contains` edge like this one is real "Dependent on" data.
        assert!(orders_content.contains("### Dependent on"));
        assert!(orders_content.contains("FOREIGN KEY (customer_id)"));
        assert!(
            orders_content.contains("```mermaid"),
            "object page embeds a diagram"
        );

        // RFC 0068 §61 follow-on: a standalone SVG counterpart alongside the Mermaid diagram,
        // for every object real relationships were actually found for.
        let svg = std::fs::read_to_string(output.join("table-orders.svg")).unwrap();
        assert!(svg.starts_with("<svg "));
        assert!(svg.contains(">orders<"));
        assert!(svg.contains(">customers<"));
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
        generate(
            &config,
            dir.path(),
            &output,
            Format::Markdown,
            Layout::Objects,
            false,
            false,
        )
        .await
        .unwrap();

        let er = std::fs::read_to_string(output.join("er-diagram.md")).unwrap();
        assert!(er.contains("erDiagram"));
        assert!(er.contains("\"orders\" }o--|| \"customers\""));

        // RFC 0068 §61 follow-on: standalone SVG counterpart, written and linked alongside the
        // Mermaid-in-Markdown ER diagram above.
        let er_svg = std::fs::read_to_string(output.join("er-diagram.svg")).unwrap();
        assert!(er_svg.starts_with("<svg "));
        assert!(er_svg.contains(">orders<"));
        assert!(er_svg.contains(">customers<"));

        let index = std::fs::read_to_string(output.join("index.md")).unwrap();
        assert!(index.contains("[Entity-Relationship Diagram](er-diagram.md)"));
        assert!(index.contains("[Entity-Relationship Diagram (SVG)](er-diagram.svg)"));
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
        generate(
            &config,
            dir.path(),
            &output,
            Format::Markdown,
            Layout::Objects,
            false,
            false,
        )
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
        generate(
            &config,
            dir.path(),
            &output,
            Format::Markdown,
            Layout::Objects,
            false,
            false,
        )
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
        generate(
            &config,
            dir.path(),
            &output,
            Format::Markdown,
            Layout::Objects,
            false,
            false,
        )
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
        generate(
            &config,
            dir.path(),
            &output,
            Format::Markdown,
            Layout::Objects,
            false,
            false,
        )
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
        generate(
            &config,
            dir.path(),
            &output,
            Format::Html,
            Layout::Objects,
            false,
            false,
        )
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
        let result = generate(
            &config,
            dir.path(),
            &output,
            Format::Markdown,
            Layout::Objects,
            true,
            true,
        )
        .await;
        assert!(
            result.is_err(),
            "no API key configured — --prose must fail clearly, not silently degrade"
        );
    }

    // ── RFC 0037 — `--layout curated` ────────────────────────────────────────

    #[tokio::test]
    async fn generate_curated_writes_exactly_the_four_named_files_and_nothing_else() {
        let dir = tempdir().unwrap();
        let config = EkosConfig::default();
        let ledger = Ledger::open(&config.ledger_path(dir.path())).unwrap();
        ledger
            .append_object(&KirObject::new("customers", ObjectKind::Table))
            .unwrap();

        let output = dir.path().join("out");
        generate(
            &config,
            dir.path(),
            &output,
            Format::Markdown,
            Layout::Curated,
            false,
            false,
        )
        .await
        .unwrap();

        let mut names: Vec<String> = std::fs::read_dir(&output)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .collect();
        names.sort();
        assert_eq!(
            names,
            vec![
                "API.md".to_string(),
                "Architecture.md".to_string(),
                "README.md".to_string(),
                "SequenceDiagrams.md".to_string(),
                // RFC 0083: the one real `Table` object is real, honest `Database` layer data —
                // `## System Decomposition` writes its SVG the same conditional way `## System
                // Context` already does (see the sibling `*_writes_system_context_svg_*` test).
                "system-decomposition.svg".to_string(),
            ]
        );

        let readme = std::fs::read_to_string(output.join("README.md")).unwrap();
        assert!(readme.contains("**Table**: 1"));
    }

    #[tokio::test]
    async fn generate_curated_writes_entity_pages_and_every_api_link_resolves_on_disk() {
        let dir = tempdir().unwrap();
        let config = EkosConfig::default();
        let ledger = Ledger::open(&config.ledger_path(dir.path())).unwrap();

        let file = KirObject::new("crates/kir/src/lib.rs", ObjectKind::File);
        let function = KirObject::new(
            "build_object_page_model",
            ObjectKind::Custom("RustSymbol".to_string()),
        )
        .with_property("kind", serde_json::json!("function"));
        ledger.append_object(&file).unwrap();
        ledger.append_object(&function).unwrap();
        let contains = KirRelationship::new(RelationshipKind::Contains, file.id, function.id);
        ledger.append_relationship(&contains).unwrap();

        let output = dir.path().join("out");
        generate(
            &config,
            dir.path(),
            &output,
            Format::Markdown,
            Layout::Curated,
            false,
            false,
        )
        .await
        .unwrap();

        assert!(
            output
                .join("entities/rustsymbol/bu/build-object-page-model.md")
                .exists(),
            "entity detail page for the RustSymbol must be written alongside the four curated \
             files, nested under entities/<kind>/<shard>/ so no directory blows past GitHub's \
             1,000-entries listing cap at real-codebase scale (RFC 0042)"
        );

        let api = std::fs::read_to_string(output.join("API.md")).unwrap();
        assert!(api.contains("build_object_page_model"));
        // Every `](...)` link API.md emits must resolve to a file `generate_curated` actually wrote.
        for link in api.split("](").skip(1) {
            let file_name = link.split(')').next().unwrap();
            assert!(
                output.join(file_name).exists(),
                "API.md links to {file_name}, which was never written"
            );
        }
    }

    #[tokio::test]
    async fn generate_curated_writes_system_context_svg_and_links_it_from_architecture_md() {
        let dir = tempdir().unwrap();
        let config = EkosConfig::default();
        let ledger = Ledger::open(&config.ledger_path(dir.path())).unwrap();

        let krate = KirObject::new("ekos-cli", ObjectKind::Custom("Crate".to_string()));
        let tech = KirObject::new("clap", ObjectKind::Custom("Technology".to_string()));
        ledger.append_object(&krate).unwrap();
        ledger.append_object(&tech).unwrap();
        let dep = KirRelationship::new(RelationshipKind::DependsOn, krate.id, tech.id);
        ledger.append_relationship(&dep).unwrap();

        let output = dir.path().join("out");
        generate(
            &config,
            dir.path(),
            &output,
            Format::Markdown,
            Layout::Curated,
            false,
            false,
        )
        .await
        .unwrap();

        let svg_path = output.join("system-context.svg");
        assert!(
            svg_path.exists(),
            "system-context.svg must be written when real Crate/Technology dependency data exists"
        );
        let svg = std::fs::read_to_string(&svg_path).unwrap();
        assert!(svg.starts_with("<svg "));
        assert!(svg.contains(">clap<"));

        let architecture = std::fs::read_to_string(output.join("Architecture.md")).unwrap();
        assert!(architecture.contains("[System Context diagram (SVG)](system-context.svg)"));
    }

    #[tokio::test]
    async fn generate_curated_writes_dependency_graph_svg_and_links_it_from_architecture_md() {
        let dir = tempdir().unwrap();
        let config = EkosConfig::default();
        let ledger = Ledger::open(&config.ledger_path(dir.path())).unwrap();

        let orders = KirObject::new("orders", ObjectKind::Table);
        let customers = KirObject::new("customers", ObjectKind::Table);
        ledger.append_object(&orders).unwrap();
        ledger.append_object(&customers).unwrap();
        let fk = KirRelationship::new(RelationshipKind::ForeignKey, orders.id, customers.id);
        ledger.append_relationship(&fk).unwrap();

        let output = dir.path().join("out");
        generate(
            &config,
            dir.path(),
            &output,
            Format::Markdown,
            Layout::Curated,
            false,
            false,
        )
        .await
        .unwrap();

        let svg_path = output.join("dependency-graph-foreignkey.svg");
        assert!(
            svg_path.exists(),
            "dependency-graph-foreignkey.svg must be written for a real, within-cap relationship kind"
        );
        let svg = std::fs::read_to_string(&svg_path).unwrap();
        assert!(svg.starts_with("<svg "));
        assert!(svg.contains(">orders<"));
        assert!(svg.contains(">customers<"));

        let architecture = std::fs::read_to_string(output.join("Architecture.md")).unwrap();
        assert!(architecture.contains(
            "[ForeignKey Dependency Graph diagram (SVG)](dependency-graph-foreignkey.svg)"
        ));
    }

    #[tokio::test]
    async fn generate_curated_omits_system_context_svg_when_no_real_dependency_data_exists() {
        let dir = tempdir().unwrap();
        let config = EkosConfig::default();
        let ledger = Ledger::open(&config.ledger_path(dir.path())).unwrap();
        ledger
            .append_object(&KirObject::new("customers", ObjectKind::Table))
            .unwrap();

        let output = dir.path().join("out");
        generate(
            &config,
            dir.path(),
            &output,
            Format::Markdown,
            Layout::Curated,
            false,
            false,
        )
        .await
        .unwrap();

        assert!(!output.join("system-context.svg").exists());
    }

    /// RFC 0083: real Backend (`.ex` `File`) + real Database (`Table`) layer data must produce a
    /// real `## System Decomposition` SVG, linked from `Architecture.md`, same conditional-write
    /// discipline the System Context SVG already has.
    #[tokio::test]
    async fn generate_curated_writes_system_decomposition_svg_and_links_it_from_architecture_md() {
        let dir = tempdir().unwrap();
        let config = EkosConfig::default();
        let ledger = Ledger::open(&config.ledger_path(dir.path())).unwrap();
        ledger
            .append_object(&KirObject::new("lib/plausible/auth.ex", ObjectKind::File))
            .unwrap();
        ledger
            .append_object(&KirObject::new("events", ObjectKind::Table))
            .unwrap();

        let output = dir.path().join("out");
        generate(
            &config,
            dir.path(),
            &output,
            Format::Markdown,
            Layout::Curated,
            false,
            false,
        )
        .await
        .unwrap();

        let svg_path = output.join("system-decomposition.svg");
        assert!(svg_path.exists());
        let svg = std::fs::read_to_string(&svg_path).unwrap();
        assert!(svg.starts_with("<svg "));
        assert!(svg.contains("Backend"));
        assert!(svg.contains("SQL Database"));

        let architecture = std::fs::read_to_string(output.join("Architecture.md")).unwrap();
        assert!(
            architecture.contains("[System Decomposition diagram (SVG)](system-decomposition.svg)")
        );
    }

    /// RFC 0083: an `[[architecture.system-decomposition.overrides]]` entry must win over the
    /// built-in extension convention, all the way through the real CLI config → render pipeline
    /// (not just the unit-level `classify_path` test in `docs-gen`).
    #[tokio::test]
    async fn generate_curated_respects_a_system_decomposition_override() {
        let dir = tempdir().unwrap();
        let mut config = EkosConfig::default();
        config.architecture.system_decomposition.overrides.push(
            ekos_compiler_core::config::LayerOverrideConfig {
                path_glob: "vendor/**/*.rs".to_string(),
                layer: "frontend".to_string(),
            },
        );
        let ledger = Ledger::open(&config.ledger_path(dir.path())).unwrap();
        ledger
            .append_object(&KirObject::new("vendor/widget.rs", ObjectKind::File))
            .unwrap();

        let output = dir.path().join("out");
        generate(
            &config,
            dir.path(),
            &output,
            Format::Markdown,
            Layout::Curated,
            false,
            false,
        )
        .await
        .unwrap();

        let svg = std::fs::read_to_string(output.join("system-decomposition.svg")).unwrap();
        assert!(
            svg.contains("Frontend"),
            "override must route the .rs file to Frontend, not the default Backend"
        );
        assert!(!svg.contains(">Backend"));
    }

    #[tokio::test]
    async fn generate_curated_rejects_html_format_with_a_clear_error() {
        let dir = tempdir().unwrap();
        let config = EkosConfig::default();
        let _ledger = Ledger::open(&config.ledger_path(dir.path())).unwrap();

        let output = dir.path().join("out");
        let result = generate(
            &config,
            dir.path(),
            &output,
            Format::Html,
            Layout::Curated,
            false,
            false,
        )
        .await;
        assert!(
            result.is_err(),
            "--layout curated with --format html must error, not silently ignore --format"
        );
    }

    #[test]
    fn layout_parse_accepts_objects_and_curated_rejects_unknown() {
        assert_eq!(Layout::parse("objects").unwrap(), Layout::Objects);
        assert_eq!(Layout::parse("curated").unwrap(), Layout::Curated);
        assert_eq!(
            Layout::parse("solution-architect").unwrap(),
            Layout::SolutionArchitect
        );
        assert!(Layout::parse("weird").is_err());
    }

    // ── RFC 0090 — `--layout solution-architect` ─────────────────────────────

    #[tokio::test]
    async fn generate_solution_architect_writes_exactly_the_three_named_files() {
        let dir = tempdir().unwrap();
        let config = EkosConfig::default();
        let ledger = Ledger::open(&config.ledger_path(dir.path())).unwrap();
        let krate = KirObject::new("ekos-cli", ObjectKind::Custom("Crate".to_string()))
            .with_property("path", serde_json::json!("crates/cli"))
            .with_property("version", serde_json::json!("0.1.0"));
        ledger.append_object(&krate).unwrap();
        let gap = KirObject::new(
            "unresolved dependency",
            ObjectKind::Custom("ArchitectureGap".to_string()),
        )
        .with_property("question", serde_json::json!("What does 'foo' resolve to?"))
        .with_property("affected_crate", serde_json::json!("ekos-cli"));
        ledger.append_object(&gap).unwrap();

        let output = dir.path().join("out");
        generate(
            &config,
            dir.path(),
            &output,
            Format::Markdown,
            Layout::SolutionArchitect,
            false,
            false,
        )
        .await
        .unwrap();

        let mut names: Vec<String> = std::fs::read_dir(&output)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .collect();
        names.sort();
        assert_eq!(
            names,
            vec![
                "DependencyRiskReport.md".to_string(),
                "FindingsMemo.md".to_string(),
                "OnboardingGuide.md".to_string(),
            ]
        );

        let risk = std::fs::read_to_string(output.join("DependencyRiskReport.md")).unwrap();
        assert!(risk.contains("| `ekos-cli` | 0.1.0 |"));

        let onboarding = std::fs::read_to_string(output.join("OnboardingGuide.md")).unwrap();
        assert!(onboarding.contains("| `crates/cli` | `ekos-cli` |"));

        let findings = std::fs::read_to_string(output.join("FindingsMemo.md")).unwrap();
        assert!(
            !findings.contains("## Executive Summary"),
            "no --prose was passed"
        );
        assert!(findings.contains("Unresolved dependency affecting `ekos-cli`"));
    }

    #[tokio::test]
    async fn generate_solution_architect_rejects_html_format_with_a_clear_error() {
        let dir = tempdir().unwrap();
        let config = EkosConfig::default();
        let _ledger = Ledger::open(&config.ledger_path(dir.path())).unwrap();

        let output = dir.path().join("out");
        let result = generate(
            &config,
            dir.path(),
            &output,
            Format::Html,
            Layout::SolutionArchitect,
            false,
            false,
        )
        .await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("only supports --format md today")
        );
    }

    #[tokio::test]
    async fn enrich_findings_memo_sets_prose_on_success() {
        let candidates = vec![ekos_docs_gen::FindingCandidate {
            title: "1 crate(s) with no declared version".to_string(),
            detail: "ekos-cli".to_string(),
        }];
        let llm = ekos_recovery::MockLlmProvider::new(
            "Declare a version for ekos-cli before the next release.",
        );
        let prose = enrich_findings_memo(&candidates, &llm).await;
        assert_eq!(
            prose.unwrap().text,
            "Declare a version for ekos-cli before the next release."
        );
    }

    #[tokio::test]
    async fn generate_solution_architect_with_prose_errors_clearly_instead_of_silently_degrading() {
        let dir = tempdir().unwrap();
        let mut config = EkosConfig::default();
        config.llm.api_key_env = Some("EKOS_DOCS_TEST_DEFINITELY_UNSET_KEY".to_string());
        let ledger = Ledger::open(&config.ledger_path(dir.path())).unwrap();
        ledger
            .append_object(&KirObject::new(
                "ekos-cli",
                ObjectKind::Custom("Crate".to_string()),
            ))
            .unwrap();
        drop(ledger);

        let output = dir.path().join("out");
        let result = generate(
            &config,
            dir.path(),
            &output,
            Format::Markdown,
            Layout::SolutionArchitect,
            true,
            true,
        )
        .await;
        assert!(
            result.is_err(),
            "no API key configured — --prose must fail clearly, not silently degrade"
        );
    }
}
