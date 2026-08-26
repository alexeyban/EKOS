//! `describe_objects` — RFC 0088's LLM-backed compile-time description step: real, evidence-
//! grounded `ai_overview`/`ai_usage`/`ai_comment_check` properties for every `Module`/`Rollup`/
//! `Crate` object and (when `source_span` was captured — Elixir and Rust at RFC 0088's own launch;
//! Python added as a fast-follow, `python_analyzer.rs`'s own `item_span`, once a real project found
//! every `PythonSymbol` honestly skipped with `scope = "symbols"`/`"all"`) every `Symbol` object.
//!
//! Deliberately **not** a `CompilerPass`: found before writing any code, by reading
//! `semantic/src/lib.rs`/`ledger/src/fact_ledger.rs` directly, that `merge_graphs`/`build_ckm`
//! never dedupe `KirObject`s sharing an id across two passes' artifacts, and each ledger version
//! is a complete-object snapshot, not a patch — so a `CompilerPass` emitting a bare object with
//! only the new `ai_*` properties could become the new "current" version and silently regress the
//! real structural properties another pass already wrote. This step instead runs post-`commit`
//! (the same architectural slot `commit_rollups`/`commit_data_lineage` already occupy in
//! `cli/src/commands/commit.rs`): it reads the real, fully-committed object from `&dyn
//! KnowledgeStore`, clones it, adds the new properties to that clone, and re-appends the clone —
//! never a bare partial object.
//!
//! Evidence model, deliberately simpler than `ai.rs::AiRuntime::ask`'s citation validation: every
//! prompt this module builds is assembled entirely from real, already-compiled data (real
//! neighbor names, real `source_span`-sliced source text) — nothing is spectulatively retrieved
//! from a larger corpus the way `ask`'s search step is, so there is no "did the LLM cite something
//! it wasn't shown" risk to guard against. One real [`KirEvidence`] is created per described
//! object, appended to that object's own `evidence` field (not a separate property), so it renders
//! through the same `## Evidence` section every other object already uses.

use crate::llm::{LlmProvider, LlmRequest};
use crate::llm_json::strip_json_fences;
use ekos_common::redaction::{RedactionConfig, redact};
use ekos_kir::{
    KirEvidence, KirId, KirObject, KirRelationship, ObjectKind, RelationshipKind, SourceLocation,
};
use ekos_ledger::KnowledgeStore;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::Path;

const PROMPT_VERSION: &str = "llm-description-v1";
const MAX_SOURCE_LINES: usize = 400;

const MODULE_KINDS: &[&str] = &[
    "ElixirModule",
    "RustModule",
    "PythonModule",
    "JsModule",
    "Crate",
    "Rollup",
];
const SYMBOL_KINDS: &[&str] = &["ElixirSymbol", "RustSymbol", "PythonSymbol", "JsSymbol"];

/// RFC 0088's `[llm-description] scope` — how far this run goes. `Modules` is the config default
/// specifically so enabling this once never silently commits a workspace to `All`'s ~5x larger
/// real spend without a second, explicit choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DescriptionScope {
    Modules,
    Symbols,
    All,
}

impl DescriptionScope {
    fn wants_modules(self) -> bool {
        matches!(self, Self::Modules | Self::All)
    }
    fn wants_symbols(self) -> bool {
        matches!(self, Self::Symbols | Self::All)
    }
}

/// Counts from one `describe_objects` run.
#[derive(Debug, Clone, Copy, Default)]
pub struct DescriptionStats {
    pub modules_considered: usize,
    pub modules_described: usize,
    pub symbols_considered: usize,
    pub symbols_described: usize,
    /// Already had a real `ai_overview` whose evidence hash is unchanged — no LLM call spent.
    pub skipped_cached: usize,
    /// A symbol in scope but with no `source_span` (a language this RFC's fast-follow hasn't
    /// reached yet, or a real span-capture miss) — honestly skipped, not guessed at.
    pub symbols_without_span: usize,
    /// The LLM call failed (network, rate limit, parse error) — that one object simply keeps
    /// whatever it had before, same "one bad call doesn't cost every other object" discipline
    /// `docs.rs::enrich_with_prose` already established.
    pub llm_errors: usize,
}

#[derive(Debug, Deserialize)]
struct LlmOutput {
    overview: String,
    #[serde(default)]
    usage: Option<String>,
    #[serde(default)]
    comment_check: Option<String>,
}

const SYSTEM_PROMPT: &str = r#"You are analyzing real, already-compiled source code structure for a software architecture knowledge base. You will be shown one real code object (a module, subsystem, or function/method) along with real structural facts already known about it (what it contains, what it depends on, what depends on it) and, where available, its real source text and any existing human-written comment.

Respond with a single JSON object, no markdown fences, matching exactly:
{"overview": "2-4 sentences describing what this real object actually does, grounded only in what you were shown", "usage": "1-3 sentences on how it is actually used, grounded only in the real dependency edges shown, or null if none were shown", "comment_check": "consistent" | "stale" | "incomplete" | null}

Rules:
- Never invent facts not present in what you were shown. If you cannot determine something, omit it rather than guess.
- "comment_check" must be null unless an existing comment was shown to you. "stale" means the comment contradicts what the real code/structure shows. "incomplete" means the comment is accurate as far as it goes but omits real behavior you can see. "consistent" means it matches.
- Do not repeat the existing comment verbatim as the overview — describe the real code/structure yourself."#;

struct Neighbors {
    contains_children: HashMap<KirId, Vec<KirId>>,
    contains_parent: HashMap<KirId, KirId>,
    depends_on: HashMap<KirId, Vec<KirId>>,
    depended_on_by: HashMap<KirId, Vec<KirId>>,
}

fn build_neighbors(relationships: &[KirRelationship]) -> Neighbors {
    let mut n = Neighbors {
        contains_children: HashMap::new(),
        contains_parent: HashMap::new(),
        depends_on: HashMap::new(),
        depended_on_by: HashMap::new(),
    };
    for rel in relationships {
        match rel.kind {
            RelationshipKind::Contains => {
                n.contains_children
                    .entry(rel.from)
                    .or_default()
                    .push(rel.to);
                n.contains_parent.insert(rel.to, rel.from);
            }
            RelationshipKind::DependsOn => {
                n.depends_on.entry(rel.from).or_default().push(rel.to);
                n.depended_on_by.entry(rel.to).or_default().push(rel.from);
            }
            _ => {}
        }
    }
    n
}

/// Walks the real `Contains`-parent chain up from `id` until it finds an `ObjectKind::File` —
/// a symbol's immediate parent is its owning module (Elixir) or the file directly (Rust, which
/// has no per-file module wrapper), so this can't assume a fixed depth. Bounded to a few hops as
/// a real cycle guard, not because a real containment chain is ever expected to be this deep.
fn find_owning_file<'a>(
    id: KirId,
    by_id: &'a HashMap<KirId, KirObject>,
    neighbors: &Neighbors,
) -> Option<&'a KirObject> {
    let mut current = id;
    for _ in 0..8 {
        let parent_id = *neighbors.contains_parent.get(&current)?;
        let parent = by_id.get(&parent_id)?;
        if parent.kind == ObjectKind::File {
            return Some(parent);
        }
        current = parent_id;
    }
    None
}

/// Real, deterministic evidence-set fingerprint — the cache key `ai_evidence_hash` compares
/// against on a re-run, so an object whose real neighbors (or, for a symbol, real source text)
/// haven't changed since the last run never re-spends an LLM call.
fn evidence_hash(parts: &[&str]) -> String {
    let mut h = Sha256::new();
    for p in parts {
        h.update(p.as_bytes());
        h.update([0u8]);
    }
    hex::encode(h.finalize())
}

fn name_of<'a>(id: &KirId, by_id: &'a HashMap<KirId, KirObject>) -> &'a str {
    by_id.get(id).map(|o| o.name.as_str()).unwrap_or("unknown")
}

fn build_module_prompt(
    obj: &KirObject,
    by_id: &HashMap<KirId, KirObject>,
    n: &Neighbors,
) -> (String, String) {
    let children: Vec<&str> = n
        .contains_children
        .get(&obj.id)
        .map(|ids| ids.iter().map(|id| name_of(id, by_id)).collect())
        .unwrap_or_default();
    let depends_on: Vec<&str> = n
        .depends_on
        .get(&obj.id)
        .map(|ids| ids.iter().map(|id| name_of(id, by_id)).collect())
        .unwrap_or_default();
    let depended_on_by: Vec<&str> = n
        .depended_on_by
        .get(&obj.id)
        .map(|ids| ids.iter().map(|id| name_of(id, by_id)).collect())
        .unwrap_or_default();
    let existing_comment = obj.properties.get("description").and_then(|v| v.as_str());

    let hash = evidence_hash(&[
        &obj.id.to_string(),
        &children.join(","),
        &depends_on.join(","),
        &depended_on_by.join(","),
    ]);

    let user = serde_json::json!({
        "object_kind": obj.kind.to_string(),
        "name": obj.name,
        "contains": children,
        "depends_on": depends_on,
        "depended_on_by": depended_on_by,
        "existing_comment": existing_comment,
    })
    .to_string();
    (user, hash)
}

/// Real source text for `obj`'s own `source_span`, redacted (RFC 0043 — sent to an external LLM
/// provider, the same secrets/PII boundary every other raw-content entry point already enforces),
/// capped at `MAX_SOURCE_LINES` real lines (a real cost/context-window guard, not a correctness
/// one — an oversized real function is truncated with an honest marker, never silently dropped).
fn read_symbol_source(
    workspace_root: &Path,
    file_path: &str,
    start_line: u64,
    end_line: u64,
    redaction: &RedactionConfig,
) -> Option<String> {
    let content = std::fs::read_to_string(workspace_root.join(file_path)).ok()?;
    let lines: Vec<&str> = content.lines().collect();
    let start = (start_line.saturating_sub(1)) as usize;
    let end = (end_line as usize).min(lines.len());
    if start >= end {
        return None;
    }
    let mut slice: Vec<&str> = lines[start..end].to_vec();
    let truncated = slice.len() > MAX_SOURCE_LINES;
    if truncated {
        slice.truncate(MAX_SOURCE_LINES);
    }
    let mut text = slice.join("\n");
    if truncated {
        text.push_str("\n... [truncated, real function longer than this excerpt]");
    }
    Some(redact(&text, redaction))
}

/// A `File` object's own `name` is relative to whichever single `[observe] paths` entry it was
/// walked from, not to the workspace root — real, found live against a real multi-path workspace
/// (this session's own analytics project: `paths = ["lib", "priv", ...]`), where a file at the
/// real path `lib/plausible/repo.ex` has `name == "plausible/repo.ex"`, `lib/` silently dropped.
/// `build.rs` already writes that dropped prefix back as a real `"project"` property (RFC 0079)
/// whenever more than one `[observe] paths` entry exists — re-joining it here is the one place
/// that matters for a real absolute path: reading the real source file from disk.
fn real_file_path(file: &KirObject) -> String {
    match file.properties.get("project").and_then(|v| v.as_str()) {
        Some(project) if !project.is_empty() => format!("{project}/{}", file.name),
        _ => file.name.clone(),
    }
}

fn build_symbol_prompt(
    obj: &KirObject,
    by_id: &HashMap<KirId, KirObject>,
    n: &Neighbors,
    workspace_root: &Path,
    redaction: &RedactionConfig,
) -> Option<(String, String, String)> {
    let span = obj.properties.get("source_span")?;
    let start_line = span.get("start_line")?.as_u64()?;
    let end_line = span.get("end_line")?.as_u64()?;
    let file = find_owning_file(obj.id, by_id, n)?;
    let real_path = real_file_path(file);
    let source = read_symbol_source(workspace_root, &real_path, start_line, end_line, redaction)?;

    let owner_name = n
        .contains_parent
        .get(&obj.id)
        .and_then(|pid| by_id.get(pid))
        .map(|o| o.name.as_str())
        .unwrap_or("unknown");
    let existing_comment = obj.properties.get("description").and_then(|v| v.as_str());

    let hash = evidence_hash(&[&obj.id.to_string(), &source]);
    let user = serde_json::json!({
        "object_kind": obj.kind.to_string(),
        "name": obj.name,
        "owning_module": owner_name,
        "source": source,
        "existing_comment": existing_comment,
    })
    .to_string();
    Some((user, hash, real_path))
}

async fn call_and_apply(
    llm: &dyn LlmProvider,
    user_message: &str,
    evidence_hash_value: &str,
    mut obj: KirObject,
    evidence_location: SourceLocation,
    evidence_fragment: String,
    store: &dyn KnowledgeStore,
) -> Result<(), String> {
    let req = LlmRequest {
        system: SYSTEM_PROMPT,
        user: user_message,
        prompt_version: PROMPT_VERSION,
        max_tokens: 1024,
        history: &[],
    };
    let resp = llm.complete(&req).await.map_err(|e| e.to_string())?;
    let output: LlmOutput =
        serde_json::from_str(strip_json_fences(&resp.content)).map_err(|e| e.to_string())?;

    if output.overview.trim().is_empty() {
        return Err("empty overview".to_string());
    }

    let had_existing_comment = obj.properties.contains_key("description");

    obj.properties.insert(
        "ai_overview".into(),
        serde_json::json!(output.overview.trim()),
    );
    if let Some(usage) = output.usage.as_deref().filter(|s| !s.trim().is_empty()) {
        obj.properties
            .insert("ai_usage".into(), serde_json::json!(usage.trim()));
    } else {
        obj.properties.remove("ai_usage");
    }
    match output.comment_check.as_deref() {
        Some(v @ ("consistent" | "stale" | "incomplete")) if had_existing_comment => {
            obj.properties
                .insert("ai_comment_check".into(), serde_json::json!(v));
        }
        _ => {
            obj.properties.remove("ai_comment_check");
        }
    }
    obj.properties.insert(
        "ai_evidence_hash".into(),
        serde_json::json!(evidence_hash_value),
    );

    let ev = KirEvidence::new(evidence_location, evidence_fragment);
    obj.evidence.push(ev.id);

    store.append_evidence(&ev).map_err(|e| e.to_string())?;
    store.append_object(&obj).map_err(|e| e.to_string())?;
    Ok(())
}

/// Real, cheap (no LLM call) upper-bound call-count estimate for the cost-confirmation gate
/// `commit.rs` shows before spending — `(modules, symbols)`, each already filtered to `scope`.
/// An upper bound, not exact: a real run may skip some via `skipped_cached`/
/// `symbols_without_span`, so the real spend is never more than this, only possibly less.
pub fn estimate_call_counts(objects: &[KirObject], scope: DescriptionScope) -> (usize, usize) {
    let modules = if scope.wants_modules() {
        objects
            .iter()
            .filter(
                |o| matches!(&o.kind, ObjectKind::Custom(s) if MODULE_KINDS.contains(&s.as_str())),
            )
            .count()
    } else {
        0
    };
    let symbols = if scope.wants_symbols() {
        objects
            .iter()
            .filter(
                |o| matches!(&o.kind, ObjectKind::Custom(s) if SYMBOL_KINDS.contains(&s.as_str())),
            )
            .count()
    } else {
        0
    };
    (modules, symbols)
}

/// Runs RFC 0088's compile-time description step against the real, already-committed ledger.
/// Called from `commit.rs` after `commit_data_lineage`, only when `[llm-description].enabled`.
pub async fn describe_objects(
    store: &dyn KnowledgeStore,
    llm: &dyn LlmProvider,
    scope: DescriptionScope,
    workspace_root: &Path,
    redaction: &RedactionConfig,
) -> Result<DescriptionStats, String> {
    let objects = store.all_objects().map_err(|e| e.to_string())?;
    let relationships = store.all_relationships().map_err(|e| e.to_string())?;
    let neighbors = build_neighbors(&relationships);
    let by_id: HashMap<KirId, KirObject> = objects.iter().map(|o| (o.id, o.clone())).collect();

    let mut stats = DescriptionStats::default();

    let mut module_targets: Vec<&KirObject> = objects
        .iter()
        .filter(|o| matches!(&o.kind, ObjectKind::Custom(s) if MODULE_KINDS.contains(&s.as_str())))
        .collect();
    module_targets.sort_by_key(|o| o.id.0);

    let mut symbol_targets: Vec<&KirObject> = objects
        .iter()
        .filter(|o| matches!(&o.kind, ObjectKind::Custom(s) if SYMBOL_KINDS.contains(&s.as_str())))
        .collect();
    symbol_targets.sort_by_key(|o| o.id.0);

    if scope.wants_modules() {
        stats.modules_considered = module_targets.len();
        for obj in module_targets {
            let (user_message, hash) = build_module_prompt(obj, &by_id, &neighbors);
            if obj
                .properties
                .get("ai_evidence_hash")
                .and_then(|v| v.as_str())
                == Some(&hash)
            {
                stats.skipped_cached += 1;
                continue;
            }
            let location = SourceLocation::file(format!("compiled dependency graph: {}", obj.name));
            let fragment = format!(
                "ai_overview grounded in {}'s real compiled neighbors",
                obj.name
            );
            match call_and_apply(
                llm,
                &user_message,
                &hash,
                obj.clone(),
                location,
                fragment,
                store,
            )
            .await
            {
                Ok(()) => stats.modules_described += 1,
                Err(_) => stats.llm_errors += 1,
            }
        }
    }

    if scope.wants_symbols() {
        stats.symbols_considered = symbol_targets.len();
        for obj in symbol_targets {
            let Some((user_message, hash, file_path)) =
                build_symbol_prompt(obj, &by_id, &neighbors, workspace_root, redaction)
            else {
                stats.symbols_without_span += 1;
                continue;
            };
            if obj
                .properties
                .get("ai_evidence_hash")
                .and_then(|v| v.as_str())
                == Some(&hash)
            {
                stats.skipped_cached += 1;
                continue;
            }
            let start_line = obj
                .properties
                .get("source_span")
                .and_then(|s| s.get("start_line"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32;
            let location = SourceLocation::at(file_path, start_line);
            let fragment = format!(
                "ai_overview grounded in {}'s own real source lines",
                obj.name
            );
            match call_and_apply(
                llm,
                &user_message,
                &hash,
                obj.clone(),
                location,
                fragment,
                store,
            )
            .await
            {
                Ok(()) => stats.symbols_described += 1,
                Err(_) => stats.llm_errors += 1,
            }
        }
    }

    Ok(stats)
}

/// Deterministic id for the one real synthetic `Custom("ProjectSummary")` object per workspace
/// — same "fixed id, not content-derived" shape `role_claim_kir_id`-style helpers elsewhere in
/// this crate use for a real, singular, well-known target.
fn project_summary_id() -> KirId {
    KirId(uuid::Uuid::new_v5(
        &uuid::Uuid::NAMESPACE_URL,
        b"ekos:project-summary",
    ))
}

#[derive(Debug, Deserialize)]
struct ProjectSummaryOutput {
    purpose: Option<String>,
    architecture_style: Option<String>,
}

/// True only for a real top-level README (`README.md`, `README`, `README.rst`, ... —
/// case-insensitive on the basename's own stem), never a loose substring match. Found live:
/// a real vendored file this project bundles, `ua_inspector/ua_inspector.readme.md`, matched
/// a naive `.contains("readme")` check before the real project README did, because iteration
/// order isn't alphabetical and (separately, see `plugins/file`'s own fix) the real README's
/// `name` was empty at the time this was first written.
fn is_real_readme_name(name: &str) -> bool {
    let basename = name.rsplit('/').next().unwrap_or(name);
    let stem = basename.split('.').next().unwrap_or(basename);
    stem.eq_ignore_ascii_case("readme")
}

/// RFC 0088's project-level call — one real LLM call per `describe_objects` run (not per-object),
/// writing `purpose`/`architecture_style` onto the one synthetic `ProjectSummary` object.
/// `Architecture.md`'s `## Architecture Summary` reads it when present; keeps today's honest
/// "not yet computed" text otherwise (this function does nothing when it has no real input at
/// all — never fabricates a purpose from an empty workspace).
pub async fn describe_project(
    store: &dyn KnowledgeStore,
    llm: &dyn LlmProvider,
    workspace_name: &str,
) -> Result<bool, String> {
    let objects = store.all_objects().map_err(|e| e.to_string())?;

    // Real bug found live, 2026-08-25, against a real whole-project workspace with more than one
    // legitimate `README.md` (the project's own root `README.md`, plus `frontend/README.md` — a
    // real but generic Vite/React scaffold template README, not project-specific content): a bare
    // `.find()` took whichever matched first in iteration order, which isn't guaranteed to be the
    // most relevant one, and produced a "purpose" describing Vite/HMR scaffolding instead of the
    // real project. Prefer the shallowest real match (fewest `/` in its name) — the root-level
    // README a project actually introduces itself in, not a nested package's own boilerplate one.
    let readme = objects
        .iter()
        .filter(|o| {
            matches!(&o.kind, ObjectKind::Custom(s) if s == "Document")
                && is_real_readme_name(&o.name)
        })
        .min_by_key(|o| o.name.matches('/').count());
    let readme_excerpt = readme
        .and_then(|o| o.properties.get("excerpt"))
        .and_then(|v| v.as_str());

    let mut rollups: Vec<&KirObject> = objects
        .iter()
        .filter(|o| matches!(&o.kind, ObjectKind::Custom(s) if s == "Rollup"))
        .collect();
    rollups.sort_by_key(|o| {
        std::cmp::Reverse(
            o.properties
                .get("member_count")
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
        )
    });
    let top_rollups: Vec<&str> = rollups.iter().take(10).map(|o| o.name.as_str()).collect();

    let technologies: Vec<&str> = objects
        .iter()
        .filter(|o| matches!(&o.kind, ObjectKind::Custom(s) if s == "Technology"))
        .map(|o| o.name.as_str())
        .collect();

    if readme_excerpt.is_none() && top_rollups.is_empty() && technologies.is_empty() {
        return Ok(false);
    }

    let user_message = serde_json::json!({
        "workspace_name": workspace_name,
        "readme_excerpt": readme_excerpt,
        "top_subsystems": top_rollups,
        "technologies": technologies,
    })
    .to_string();

    // Real bug found live, 2026-08-24: against a real project (a Python/TypeScript PDF reader),
    // a weak local model (`llama3:latest` via Ollama) produced a self-referential "purpose" that
    // described *this analysis tool* ("generating knowledge ledger facts... AI overview") instead
    // of the real project it was actually shown. No caching/prompt-construction bug explains
    // this — the prompt genuinely was built from that project's own real compiled data — so this
    // is a real model-quality failure, not something fixable with a guarantee. Two bounded,
    // testable mitigations: (1) `workspace_name` above gives the model a concrete named anchor
    // instead of only generic subsystem/technology name lists; (2) the explicit anti-self-
    // reference sentence below, added after this exact failure was observed.
    let system_prompt = "You are summarizing a real, already-compiled software workspace for an architecture knowledge base. You will be shown the real workspace's own name, a real README excerpt (if any), the largest real compiled subsystems, and real compiled technology dependencies — all real facts about that one specific workspace, not about the tool producing this summary.\n\nRespond with a single JSON object, no markdown fences, matching exactly:\n{\"purpose\": \"1-2 sentences on what this real project (named in workspace_name) is for, grounded only in what you were shown, or null if you cannot tell\", \"architecture_style\": \"a short real architectural style label (e.g. \\\"modular monolith\\\", \\\"microservices\\\", \\\"layered\\\", \\\"event-driven\\\"), grounded only in what you were shown, or null if you cannot tell\"}\n\nNever invent facts not present in what you were shown. Describe the real project named in workspace_name only — never describe EKOS, a knowledge ledger, an architecture knowledge base, or any analysis/compiler tool; that is not the project being analyzed.";

    let req = LlmRequest {
        system: system_prompt,
        user: &user_message,
        prompt_version: PROMPT_VERSION,
        max_tokens: 512,
        history: &[],
    };
    let resp = llm.complete(&req).await.map_err(|e| e.to_string())?;
    let output: ProjectSummaryOutput =
        serde_json::from_str(strip_json_fences(&resp.content)).map_err(|e| e.to_string())?;

    if output.purpose.is_none() && output.architecture_style.is_none() {
        return Ok(false);
    }

    let mut obj = KirObject::new(
        "Project Summary",
        ObjectKind::Custom("ProjectSummary".into()),
    );
    obj.id = project_summary_id();
    if let Some(purpose) = output.purpose.as_deref().filter(|s| !s.trim().is_empty()) {
        obj.properties
            .insert("purpose".into(), serde_json::json!(purpose.trim()));
    }
    if let Some(style) = output
        .architecture_style
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        obj.properties
            .insert("architecture_style".into(), serde_json::json!(style.trim()));
    }

    let location = readme
        .map(|r| SourceLocation::file(r.name.clone()))
        .unwrap_or_else(|| SourceLocation::file("compiled dependency graph"));
    let ev = KirEvidence::new(
        location,
        "purpose/architecture_style grounded in real README/subsystems/technologies",
    );
    obj.evidence.push(ev.id);

    store.append_evidence(&ev).map_err(|e| e.to_string())?;
    store.append_object(&obj).map_err(|e| e.to_string())?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::{LlmError, LlmResponse};
    use async_trait::async_trait;
    use ekos_kir::KirGraph;
    use ekos_ledger::Ledger;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tempfile::tempdir;

    /// Records every real request it receives (so a test can assert on real prompt content, not
    /// just the final property) and counts calls (so a cache-hit test can assert the LLM was
    /// genuinely *not* called a second time, not just that the output looks the same).
    struct RecordingLlmProvider {
        response: String,
        calls: AtomicUsize,
        requests: Mutex<Vec<String>>,
    }

    impl RecordingLlmProvider {
        fn new(response: impl Into<String>) -> Self {
            Self {
                response: response.into(),
                calls: AtomicUsize::new(0),
                requests: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl LlmProvider for RecordingLlmProvider {
        fn model_name(&self) -> &str {
            "recording-mock"
        }
        async fn complete(&self, req: &LlmRequest<'_>) -> Result<LlmResponse, LlmError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.requests.lock().unwrap().push(req.user.to_string());
            Ok(LlmResponse {
                content: self.response.clone(),
                model: self.model_name().to_string(),
                input_tokens: 0,
                output_tokens: 0,
            })
        }
    }

    fn temp_store() -> (Ledger, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let path = dir.path().join("ledger.db");
        (Ledger::open(&path).unwrap(), dir)
    }

    fn seed(store: &dyn KnowledgeStore, graph: KirGraph) {
        for ev in &graph.evidence {
            store.append_evidence(ev).unwrap();
        }
        for obj in &graph.objects {
            store.append_object(obj).unwrap();
        }
        for rel in &graph.relationships {
            store.append_relationship(rel).unwrap();
        }
    }

    fn module_with_symbol() -> (KirObject, KirObject, KirObject, KirGraph) {
        let file = KirObject::new("lib/plausible/repo.ex", ObjectKind::File);
        let module = KirObject::new("Plausible.Repo", ObjectKind::Custom("ElixirModule".into()));
        let symbol = KirObject::new("get_user", ObjectKind::Custom("ElixirSymbol".into()));
        let mut graph = KirGraph::new();
        graph.relationships.push(KirRelationship::new(
            RelationshipKind::Contains,
            file.id,
            module.id,
        ));
        graph.relationships.push(KirRelationship::new(
            RelationshipKind::Contains,
            module.id,
            symbol.id,
        ));
        graph.objects.push(file.clone());
        graph.objects.push(module.clone());
        graph.objects.push(symbol.clone());
        (file, module, symbol, graph)
    }

    #[tokio::test]
    async fn a_module_with_real_neighbors_gets_a_grounded_overview() {
        let (store, _dir) = temp_store();
        let (_file, module, _symbol, graph) = module_with_symbol();
        seed(&store, graph);
        let llm = RecordingLlmProvider::new(
            r#"{"overview": "Ecto repo for the Plausible domain.", "usage": "Used by callers needing user records.", "comment_check": null}"#,
        );
        let workspace = tempdir().unwrap();

        let stats = describe_objects(
            &store,
            &llm,
            DescriptionScope::Modules,
            workspace.path(),
            &RedactionConfig::default(),
        )
        .await
        .unwrap();

        assert_eq!(stats.modules_described, 1);
        assert_eq!(
            stats.symbols_considered, 0,
            "Modules scope must not touch symbols"
        );
        let updated = store.get_object(&module.id).unwrap().unwrap();
        assert_eq!(
            updated.properties["ai_overview"],
            "Ecto repo for the Plausible domain."
        );
        assert_eq!(
            updated.properties["ai_usage"],
            "Used by callers needing user records."
        );
        assert!(!updated.properties.contains_key("ai_comment_check"));
        // The real structural property from before this run must survive the new version.
        assert_eq!(updated.name, "Plausible.Repo");
        assert!(
            !updated.evidence.is_empty(),
            "a real evidence record must be attached"
        );

        // The real neighbor name must actually have reached the prompt.
        let sent = llm.requests.lock().unwrap();
        assert!(sent[0].contains("get_user"));
    }

    #[tokio::test]
    async fn scope_symbols_never_touches_modules() {
        let (store, _dir) = temp_store();
        let (_file, module, _symbol, graph) = module_with_symbol();
        seed(&store, graph);
        let llm = crate::llm::MockLlmProvider::new("{}");
        let workspace = tempdir().unwrap();

        let stats = describe_objects(
            &store,
            &llm,
            DescriptionScope::Symbols,
            workspace.path(),
            &RedactionConfig::default(),
        )
        .await
        .unwrap();

        assert_eq!(stats.modules_considered, 0);
        let unchanged = store.get_object(&module.id).unwrap().unwrap();
        assert!(!unchanged.properties.contains_key("ai_overview"));
    }

    #[tokio::test]
    async fn an_unchanged_evidence_hash_skips_the_llm_call_on_a_second_run() {
        let (store, _dir) = temp_store();
        let (_file, _module, _symbol, graph) = module_with_symbol();
        seed(&store, graph);
        let llm = RecordingLlmProvider::new(r#"{"overview": "A repo module."}"#);
        let workspace = tempdir().unwrap();
        let redaction = RedactionConfig::default();

        describe_objects(
            &store,
            &llm,
            DescriptionScope::Modules,
            workspace.path(),
            &redaction,
        )
        .await
        .unwrap();
        assert_eq!(llm.calls.load(Ordering::SeqCst), 1);

        describe_objects(
            &store,
            &llm,
            DescriptionScope::Modules,
            workspace.path(),
            &redaction,
        )
        .await
        .unwrap();
        assert_eq!(
            llm.calls.load(Ordering::SeqCst),
            1,
            "an unchanged real neighbor set must not re-spend a second LLM call"
        );
    }

    #[tokio::test]
    async fn a_changed_neighbor_set_invalidates_the_cache_and_re_calls() {
        let (store, _dir) = temp_store();
        let (file, module, _symbol, mut graph) = module_with_symbol();
        seed(&store, graph.clone());
        let llm = RecordingLlmProvider::new(r#"{"overview": "A repo module."}"#);
        let workspace = tempdir().unwrap();
        let redaction = RedactionConfig::default();

        describe_objects(
            &store,
            &llm,
            DescriptionScope::Modules,
            workspace.path(),
            &redaction,
        )
        .await
        .unwrap();
        assert_eq!(llm.calls.load(Ordering::SeqCst), 1);

        // A real new dependency edge changes the module's real neighbor set. `Technology` (not a
        // module/symbol kind) so this is purely a new neighbor, not itself a second description
        // target — isolates the assertion to "did the existing module's own cache invalidate."
        let other = KirObject::new("PostgreSQL", ObjectKind::Custom("Technology".into()));
        graph.objects.push(other.clone());
        store.append_object(&other).unwrap();
        store
            .append_relationship(&KirRelationship::new(
                RelationshipKind::DependsOn,
                module.id,
                other.id,
            ))
            .unwrap();
        let _ = file;

        describe_objects(
            &store,
            &llm,
            DescriptionScope::Modules,
            workspace.path(),
            &redaction,
        )
        .await
        .unwrap();
        assert_eq!(
            llm.calls.load(Ordering::SeqCst),
            2,
            "a real new dependency edge must invalidate the cached evidence hash"
        );
    }

    #[tokio::test]
    async fn a_symbol_without_source_span_is_honestly_skipped() {
        let (store, _dir) = temp_store();
        let (_file, _module, symbol, graph) = module_with_symbol();
        seed(&store, graph);
        let llm = RecordingLlmProvider::new(r#"{"overview": "x"}"#);
        let workspace = tempdir().unwrap();

        let stats = describe_objects(
            &store,
            &llm,
            DescriptionScope::Symbols,
            workspace.path(),
            &RedactionConfig::default(),
        )
        .await
        .unwrap();

        assert_eq!(stats.symbols_without_span, 1);
        assert_eq!(stats.symbols_described, 0);
        assert_eq!(llm.calls.load(Ordering::SeqCst), 0);
        let unchanged = store.get_object(&symbol.id).unwrap().unwrap();
        assert!(!unchanged.properties.contains_key("ai_overview"));
    }

    #[tokio::test]
    async fn a_multi_path_workspaces_file_prefix_is_reconstructed_from_its_real_project_property() {
        // Real bug caught live (this session's own local-Ollama verification run): in a real
        // multi-`[observe] paths` workspace, `File.name` is relative to whichever single path
        // entry it was walked from — `build.rs` writes the dropped prefix back as a real
        // `"project"` property, but this module originally read `file.name` alone, so it tried
        // (and failed) to open e.g. `<workspace_root>/repo.ex` instead of the real
        // `<workspace_root>/lib/plausible/repo.ex`.
        let workspace = tempdir().unwrap();
        std::fs::create_dir_all(workspace.path().join("lib/plausible")).unwrap();
        std::fs::write(
            workspace.path().join("lib/plausible/repo.ex"),
            "defmodule Plausible.Repo do\n  def get_user(id) do\n    Repo.get(User, id)\n  end\nend\n",
        )
        .unwrap();

        let file = KirObject::new("plausible/repo.ex", ObjectKind::File)
            .with_property("project", serde_json::json!("lib"));
        let module = KirObject::new("Plausible.Repo", ObjectKind::Custom("ElixirModule".into()));
        let symbol = KirObject::new("get_user", ObjectKind::Custom("ElixirSymbol".into()))
            .with_property(
                "source_span",
                serde_json::json!({"start_line": 2, "end_line": 4}),
            );
        let mut graph = KirGraph::new();
        graph.relationships.push(KirRelationship::new(
            RelationshipKind::Contains,
            file.id,
            module.id,
        ));
        graph.relationships.push(KirRelationship::new(
            RelationshipKind::Contains,
            module.id,
            symbol.id,
        ));
        graph.objects.push(file);
        graph.objects.push(module);
        graph.objects.push(symbol.clone());

        let (store, _dir) = temp_store();
        seed(&store, graph);
        let llm = RecordingLlmProvider::new(r#"{"overview": "Fetches a user by id."}"#);

        let stats = describe_objects(
            &store,
            &llm,
            DescriptionScope::Symbols,
            workspace.path(),
            &RedactionConfig::default(),
        )
        .await
        .unwrap();

        assert_eq!(
            stats.symbols_without_span, 0,
            "the real file must be found and read, not skipped"
        );
        assert_eq!(stats.symbols_described, 1);
        let sent = llm.requests.lock().unwrap();
        assert!(
            sent[0].contains("Repo.get(User, id)"),
            "the real source, read via the reconstructed lib/ prefix, must reach the prompt: {}",
            sent[0]
        );
    }

    #[tokio::test]
    async fn a_symbol_with_a_real_source_span_reads_the_real_file_and_gets_grounded_output() {
        let workspace = tempdir().unwrap();
        std::fs::create_dir_all(workspace.path().join("lib/plausible")).unwrap();
        std::fs::write(
            workspace.path().join("lib/plausible/repo.ex"),
            "defmodule Plausible.Repo do\n  def get_user(id) do\n    Repo.get(User, id)\n  end\nend\n",
        )
        .unwrap();

        let (store, _dir) = temp_store();
        let (_file, _module, symbol, mut graph) = module_with_symbol();
        // Real source_span matching the file just written (1-indexed, the `def get_user` line
        // through its closing `end`).
        for obj in &mut graph.objects {
            if obj.id == symbol.id {
                obj.properties.insert(
                    "source_span".into(),
                    serde_json::json!({"start_line": 2, "end_line": 4}),
                );
            }
        }
        seed(&store, graph);

        let llm = RecordingLlmProvider::new(
            r#"{"overview": "Fetches a user by id via the Ecto repo.", "usage": null, "comment_check": null}"#,
        );

        let stats = describe_objects(
            &store,
            &llm,
            DescriptionScope::Symbols,
            workspace.path(),
            &RedactionConfig::default(),
        )
        .await
        .unwrap();

        assert_eq!(stats.symbols_described, 1);
        let updated = store.get_object(&symbol.id).unwrap().unwrap();
        assert_eq!(
            updated.properties["ai_overview"],
            "Fetches a user by id via the Ecto repo."
        );
        let sent = llm.requests.lock().unwrap();
        assert!(
            sent[0].contains("Repo.get(User, id)"),
            "the real sliced source text must have reached the prompt: {}",
            sent[0]
        );
    }

    #[tokio::test]
    async fn comment_check_is_kept_only_when_a_real_existing_comment_was_present() {
        let (store, _dir) = temp_store();
        let (_file, module, _symbol, graph) = module_with_symbol();
        seed(&store, graph);
        // No `description` property on `module` — an existing comment was never shown.
        let llm = RecordingLlmProvider::new(
            r#"{"overview": "A repo module.", "comment_check": "stale"}"#,
        );
        let workspace = tempdir().unwrap();

        describe_objects(
            &store,
            &llm,
            DescriptionScope::Modules,
            workspace.path(),
            &RedactionConfig::default(),
        )
        .await
        .unwrap();

        let updated = store.get_object(&module.id).unwrap().unwrap();
        assert!(
            !updated.properties.contains_key("ai_comment_check"),
            "comment_check must never be kept when no real existing comment was shown"
        );
    }

    #[tokio::test]
    async fn comment_check_is_kept_when_a_real_existing_comment_was_present() {
        let (store, _dir) = temp_store();
        let (_file, module, _symbol, mut graph) = module_with_symbol();
        for obj in &mut graph.objects {
            if obj.id == module.id {
                obj.properties.insert(
                    "description".into(),
                    serde_json::json!("A module for handling repos."),
                );
            }
        }
        seed(&store, graph);
        let llm = RecordingLlmProvider::new(
            r#"{"overview": "A repo module.", "comment_check": "incomplete"}"#,
        );
        let workspace = tempdir().unwrap();

        describe_objects(
            &store,
            &llm,
            DescriptionScope::Modules,
            workspace.path(),
            &RedactionConfig::default(),
        )
        .await
        .unwrap();

        let updated = store.get_object(&module.id).unwrap().unwrap();
        assert_eq!(updated.properties["ai_comment_check"], "incomplete");
        // The real, original comment property must survive untouched.
        assert_eq!(
            updated.properties["description"],
            "A module for handling repos."
        );
    }

    #[tokio::test]
    async fn an_empty_overview_is_rejected_and_not_written() {
        let (store, _dir) = temp_store();
        let (_file, module, _symbol, graph) = module_with_symbol();
        seed(&store, graph);
        let llm = RecordingLlmProvider::new(r#"{"overview": "   "}"#);
        let workspace = tempdir().unwrap();

        let stats = describe_objects(
            &store,
            &llm,
            DescriptionScope::Modules,
            workspace.path(),
            &RedactionConfig::default(),
        )
        .await
        .unwrap();

        assert_eq!(stats.llm_errors, 1);
        assert_eq!(stats.modules_described, 0);
        let unchanged = store.get_object(&module.id).unwrap().unwrap();
        assert!(!unchanged.properties.contains_key("ai_overview"));
    }

    #[test]
    fn secrets_in_real_symbol_source_are_redacted_before_reaching_the_prompt() {
        let mut config = RedactionConfig::default();
        // The built-in baseline already covers common real secret shapes; this just confirms
        // `read_symbol_source` actually calls through to it rather than skipping redaction.
        config.extra_patterns.push((
            "test-token".to_string(),
            r"TESTTOKEN-[A-Za-z0-9]+".to_string(),
        ));
        let workspace = tempdir().unwrap();
        std::fs::write(
            workspace.path().join("f.ex"),
            "def f do\n  TESTTOKEN-abc123\nend\n",
        )
        .unwrap();
        let text = read_symbol_source(workspace.path(), "f.ex", 1, 3, &config).unwrap();
        assert!(!text.contains("TESTTOKEN-abc123"));
        assert!(text.contains("[REDACTED:test-token]"));
    }

    // ── RFC 0088 — describe_project ─────────────────────────────────────────

    #[tokio::test]
    async fn describe_project_writes_purpose_and_style_from_real_input() {
        let (store, _dir) = temp_store();
        let readme = KirObject::new("README.md", ObjectKind::Custom("Document".into()))
            .with_property(
                "excerpt",
                serde_json::json!("A privacy-friendly analytics tool."),
            );
        let rollup = KirObject::new("lib", ObjectKind::Custom("Rollup".into()))
            .with_property("member_count", serde_json::json!(500));
        store.append_object(&readme).unwrap();
        store.append_object(&rollup).unwrap();

        let llm = RecordingLlmProvider::new(
            r#"{"purpose": "A privacy-friendly web analytics platform.", "architecture_style": "modular monolith"}"#,
        );
        let wrote = describe_project(&store, &llm, "analytics").await.unwrap();
        assert!(wrote);

        let summary = store.get_object(&project_summary_id()).unwrap().unwrap();
        assert_eq!(
            summary.properties["purpose"],
            "A privacy-friendly web analytics platform."
        );
        assert_eq!(summary.properties["architecture_style"], "modular monolith");
        assert!(!summary.evidence.is_empty());

        let sent = llm.requests.lock().unwrap();
        assert!(sent[0].contains("privacy-friendly analytics tool"));
        assert!(sent[0].contains("\"lib\""));
        // RFC 0088 fast-follow, 2026-08-24: a real concrete workspace-name anchor must reach the
        // prompt, not just generic subsystem/technology name lists.
        assert!(sent[0].contains("\"analytics\""));
    }

    #[test]
    fn is_real_readme_name_rejects_a_vendored_readme_lookalike() {
        // The exact real shape found live: a real vendored file this project bundles under
        // `priv/ua_inspector/`, which a naive substring check on "readme" would match ahead
        // of the real top-level README depending on iteration order.
        assert!(is_real_readme_name("README.md"));
        assert!(is_real_readme_name("readme"));
        assert!(is_real_readme_name("README.rst"));
        assert!(!is_real_readme_name("ua_inspector/ua_inspector.readme.md"));
        assert!(!is_real_readme_name(
            "ref_inspector/ref_inspector.readme.md"
        ));
    }

    #[tokio::test]
    async fn describe_project_prefers_the_real_readme_over_a_vendored_lookalike() {
        let (store, _dir) = temp_store();
        // Inserted first, deliberately, so a naive substring/first-match check would win.
        let vendored = KirObject::new(
            "ua_inspector/ua_inspector.readme.md",
            ObjectKind::Custom("Document".into()),
        )
        .with_property(
            "excerpt",
            serde_json::json!("A parser database for UAInspector."),
        );
        let real_readme = KirObject::new("README.md", ObjectKind::Custom("Document".into()))
            .with_property(
                "excerpt",
                serde_json::json!("A privacy-friendly analytics tool."),
            );
        store.append_object(&vendored).unwrap();
        store.append_object(&real_readme).unwrap();

        let llm = RecordingLlmProvider::new(r#"{"purpose": "x", "architecture_style": "y"}"#);
        describe_project(&store, &llm, "analytics").await.unwrap();

        let sent = llm.requests.lock().unwrap();
        assert!(
            sent[0].contains("privacy-friendly analytics tool"),
            "the real top-level README must win, not the vendored lookalike: {}",
            sent[0]
        );
        assert!(!sent[0].contains("UAInspector"));
    }

    #[tokio::test]
    async fn describe_project_prefers_the_root_readme_over_a_nested_packages_own_real_readme() {
        // Real bug found live, 2026-08-25, against a real whole-project workspace: both a real
        // project root `README.md` *and* a real, legitimate `frontend/README.md` (a generic
        // Vite/React scaffold template, not vendored/fake — `is_real_readme_name` correctly
        // accepts it) exist. Unlike the vendored-lookalike case above, both pass the name check;
        // a bare first-match `.find()` picked whichever came first in iteration order and
        // produced a "purpose" describing Vite/HMR scaffolding instead of the real project.
        let (store, _dir) = temp_store();
        let nested = KirObject::new("frontend/README.md", ObjectKind::Custom("Document".into()))
            .with_property(
                "excerpt",
                serde_json::json!(
                    "This template provides a minimal setup to get React working in Vite with HMR."
                ),
            );
        let root = KirObject::new("README.md", ObjectKind::Custom("Document".into()))
            .with_property(
                "excerpt",
                serde_json::json!("A PDF reader with AI-assisted explain and translate."),
            );
        // Inserted in the order that reproduced the real bug — the nested one first.
        store.append_object(&nested).unwrap();
        store.append_object(&root).unwrap();

        let llm = RecordingLlmProvider::new(r#"{"purpose": "x", "architecture_style": "y"}"#);
        describe_project(&store, &llm, "pdf-reader").await.unwrap();

        let sent = llm.requests.lock().unwrap();
        assert!(
            sent[0].contains("PDF reader"),
            "the real project root README must win over a nested package's own README: {}",
            sent[0]
        );
        assert!(!sent[0].contains("Vite"));
    }

    #[tokio::test]
    async fn describe_project_writes_nothing_on_an_empty_workspace() {
        let (store, _dir) = temp_store();
        let llm = RecordingLlmProvider::new(r#"{"purpose": "x", "architecture_style": "y"}"#);
        let wrote = describe_project(&store, &llm, "empty-workspace")
            .await
            .unwrap();
        assert!(!wrote);
        assert_eq!(llm.calls.load(Ordering::SeqCst), 0);
        assert!(store.get_object(&project_summary_id()).unwrap().is_none());
    }

    #[tokio::test]
    async fn describe_project_writes_nothing_when_the_llm_cannot_tell() {
        let (store, _dir) = temp_store();
        let rollup = KirObject::new("lib", ObjectKind::Custom("Rollup".into()))
            .with_property("member_count", serde_json::json!(1));
        store.append_object(&rollup).unwrap();
        let llm = RecordingLlmProvider::new(r#"{"purpose": null, "architecture_style": null}"#);
        let wrote = describe_project(&store, &llm, "lib").await.unwrap();
        assert!(!wrote);
        assert!(store.get_object(&project_summary_id()).unwrap().is_none());
    }
}
