//! `ElixirAnalyzerPass` — structural extraction of Elixir source (RFC 0081) into plain KIR
//! objects and relationships, real AST-adjacent decomposition instead of `plugins/file`'s crude
//! declaration-prefix symbol scan (RFC 0019/0076) — the only thing every non-Rust/Python language
//! got until now: bare name strings, no relationships, no module hierarchy at all.
//!
//! No mature Elixir-grammar Rust crate exists (unlike `syn`/`rustpython-parser`), so this is a
//! real, bounded, hand-written structural scanner — same "read what's declared, don't build a
//! full resolver" spirit `crate_topology_analyzer.rs`'s own doc comment states for Cargo, and the
//! same scope decision `python_analyzer.rs` made: module/symbol/dependency structure, not a call
//! graph. Elixir's OTP/Phoenix architecture is legible from module boundaries and `alias`/
//! `import`/`use`/`require` dependency edges, which is what an architecture diagram needs — not
//! interprocedural call tracing.
//!
//! Scope, deliberately narrow and honest rather than a claim of full parsing:
//! - `defmodule Name do ... end` becomes `KirObject(Custom("ElixirModule"))` + a `Contains` edge
//!   from the owning `File`.
//! - `def`/`defp` become `KirObject(Custom("ElixirSymbol"))` (tagged `visibility: public/private`)
//!   and a `Contains` edge from the owning module. Multiple clauses of the same `name/arity`
//!   (real Elixir multi-clause dispatch) collapse into **one** symbol, matching how this whole
//!   codebase already treats one Rust `impl` method or one Python `def` as one symbol, not one
//!   per clause.
//! - `alias`/`import`/`use`/`require` become real `DependsOn` edges from the owning module to the
//!   named target module — using the *same* deterministic id scheme for both a module's own
//!   `defmodule` declaration and any reference to it, so a real internal dependency (module A
//!   depends on module B, both defined in this codebase) resolves onto the *same* real object,
//!   not two disconnected ones. Multi-alias forms (`alias Plausible.{Auth, Teams}`) expand to one
//!   real `DependsOn` edge per named leaf (`Plausible.Auth`, `Plausible.Teams`) — pre-scanned
//!   separately (`prescan_multi_alias_targets`), the same lookahead the doc-comment pre-scan
//!   already needs, since Elixir's own formatter commonly wraps each leaf onto its own line and
//!   the main per-line loop has no lookahead. Fixed 2026-08-23: this used to create one honest but
//!   misleading `DependsOn` edge to the bare shared prefix instead (`Plausible` itself, never
//!   `defmodule`'d anywhere) — a real phantom object with no file, no properties, and no real
//!   relationships of its own, indistinguishable on its generated entity page from an actually
//!   undocumented real module.
//! - Block nesting (`do`/`end`/`fn`, including `if`/`case`/`cond`/`with`/try`/`receive`/`for`) is
//!   tracked generically via a simple depth stack so a real guard clause spanning to a `do` on a
//!   *later* line, or an inline `fn ... end`, doesn't desynchronize which module a later
//!   `def`/`alias` line is attributed to. Not a full parser: comments are stripped by a simple
//!   quote-aware scan (not sigil/heredoc-aware), and a stray unmatched `end`/`do` inside a string
//!   literal this misses could desynchronize depth tracking for the rest of that one file — an
//!   accepted, documented limitation, the same tradeoff `plugins/file`'s own fallback scan makes.

use async_trait::async_trait;
use ekos_artifact::ArtifactId;
use ekos_compiler_core::pass::{CompilerPass, PassContext, PassError};
use ekos_kir::{KirGraph, KirId, KirObject, KirRelationship, ObjectKind, RelationshipKind};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

#[derive(Debug, Deserialize)]
struct ElixirArtifactData {
    path: String,
    source: String,
    /// RFC 0079: present only in a multi-`[observe] paths` workspace (`build.rs`'s own choke
    /// point). Qualifies id hashing only — `path` stays bare everywhere it's displayed.
    #[serde(default)]
    project: Option<String>,
}

/// Coverage counters from one run, mirroring `RustStats`/`PythonStats`.
#[derive(Debug, Clone, Copy, Default)]
pub struct ElixirStats {
    pub files_processed: usize,
    pub modules_total: usize,
    pub symbols_total: usize,
}

pub struct ElixirAnalyzerPass {
    pass_id: String,
    artifact_ids: Vec<ArtifactId>,
    stats: Arc<Mutex<ElixirStats>>,
}

impl ElixirAnalyzerPass {
    pub fn new(workspace_name: impl Into<String>, artifact_ids: Vec<ArtifactId>) -> Self {
        Self {
            pass_id: format!("elixir-analyzer:{}", workspace_name.into()),
            artifact_ids,
            stats: Arc::new(Mutex::new(ElixirStats::default())),
        }
    }

    pub fn stats_handle(&self) -> Arc<Mutex<ElixirStats>> {
        Arc::clone(&self.stats)
    }
}

#[async_trait]
impl CompilerPass for ElixirAnalyzerPass {
    fn name(&self) -> &str {
        &self.pass_id
    }

    fn cache_inputs(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.artifact_ids.iter().map(|id| id.to_string()).collect();
        ids.sort();
        ids
    }

    async fn run(&mut self, ctx: &mut PassContext) -> Result<(), PassError> {
        let mut combined = KirGraph::new();
        let mut stats = ElixirStats::default();
        // Dedup module target objects across files within this one run — many files can
        // `alias`/`import` the same module; mirrors `rust_analyzer.rs`/`python_analyzer.rs`'s own
        // module dedup discipline. Safe here specifically because every `ElixirModule` object has
        // the identical shape regardless of which occurrence (self-declaration or a reference)
        // produced it first — no risk of a "richer" version losing to a "thinner" one. Also covers
        // `Custom("Technology")` (RFC 0086 Phase 6): the real analytics project alone declares 5
        // separate `use Ecto.Repo, adapter: Ecto.Adapters.ClickHouse` modules, each of which would
        // otherwise re-push an identical "ClickHouse" object into this one artifact — the exact
        // avoidable-duplication class RFC 0076 Finding 6 already tracks elsewhere, not one to
        // introduce fresh here.
        let mut seen_modules: HashSet<KirId> = HashSet::new();

        for artifact_id in &self.artifact_ids {
            let json = match ctx.artifact_store.read(artifact_id) {
                Ok(Some(j)) => j,
                Ok(None) => continue,
                Err(e) => {
                    ctx.diagnostics.lock().unwrap().warning(
                        "ELIXIR001",
                        format!("cannot read artifact {artifact_id}: {e}"),
                    );
                    continue;
                }
            };
            let data: ElixirArtifactData = match serde_json::from_value(json["data"].clone()) {
                Ok(d) => d,
                Err(e) => {
                    ctx.diagnostics.lock().unwrap().warning(
                        "ELIXIR002",
                        format!("malformed elixir payload in {artifact_id}: {e}"),
                    );
                    continue;
                }
            };

            let id_path =
                ekos_common::project::project_qualify(&data.path, data.project.as_deref());
            let file_id = KirId(Uuid::new_v5(&Uuid::NAMESPACE_URL, id_path.as_bytes()));
            let result = parse_elixir_file(&data.source, file_id, data.project.as_deref());

            stats.files_processed += 1;
            stats.modules_total += result.module_count;
            stats.symbols_total += result.symbol_count;

            for obj in result.objects {
                if seen_modules.insert(obj.id)
                    || !matches!(obj.kind, ObjectKind::Custom(ref k) if k == "ElixirModule" || k == "Technology")
                {
                    combined.add_object(obj);
                }
            }
            for rel in result.relationships {
                combined.add_relationship(rel);
            }
        }

        *self.stats.lock().unwrap() = stats;

        if combined.objects.is_empty() {
            return Ok(());
        }

        let knowledge = ekos_artifact::KnowledgeArtifact::new(&self.pass_id, vec![], combined);
        let json = serde_json::to_value(&knowledge)
            .map_err(|e| PassError::failed(format!("serialize KnowledgeArtifact: {e}")))?;
        ctx.artifact_store
            .write(&knowledge.id, &json)
            .map_err(|e| PassError::failed(format!("write artifact: {e}")))?;

        tracing::info!(
            pass = %self.pass_id,
            files = stats.files_processed,
            modules = stats.modules_total,
            symbols = stats.symbols_total,
            "elixir-analyzer complete"
        );
        Ok(())
    }
}

// ── Deterministic ids ────────────────────────────────────────────────────────

fn elixir_module_kir_id(qualified_name: &str) -> KirId {
    KirId(Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("elixir-module:{qualified_name}").as_bytes(),
    ))
}

fn elixir_symbol_kir_id(owner: KirId, qualified_name: &str, arity: usize) -> KirId {
    KirId(Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("elixir-symbol:{owner}:{qualified_name}/{arity}").as_bytes(),
    ))
}

/// RFC 0086 (Phase 6): same id scheme `dependency_analyzer.rs`'s/`crate_topology_analyzer.rs`'s/
/// `package_json_analyzer.rs`'s own `technology_kir_id` use, kept in sync deliberately (each
/// module has its own local copy — this codebase's established convention, not a shared util) so
/// a database adapter detected here resolves to the same real object a substring-pattern hit in
/// `dependency_analyzer.rs` (e.g. a real `postgres://` connection string elsewhere) would produce.
/// Deliberately **not** project-qualified, unlike `package_json_analyzer.rs`'s own npm packages:
/// "PostgreSQL"/"ClickHouse" name a real external technology, not a project-scoped artifact —
/// matches `dependency_analyzer.rs`'s own unqualified convention for the same reason.
fn technology_kir_id(name: &str) -> KirId {
    KirId(Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("technology:{name}").as_bytes(),
    ))
}

// ── Parsing ──────────────────────────────────────────────────────────────────

#[derive(Default)]
struct ElixirFileResult {
    objects: Vec<KirObject>,
    relationships: Vec<KirRelationship>,
    module_count: usize,
    symbol_count: usize,
}

/// One open block on the depth stack. `Module` carries the id so `def`/`alias` lines can find
/// "the innermost module currently open" regardless of how much unrelated nesting (`if`/`case`/
/// `fn`) sits in between. `Symbol` (RFC 0088's `source_span`) marks the block opened by a real
/// `def`/`defp`'s own body — carrying the id back out on `end` so the matching close line can be
/// recorded, the same "carry what a later line needs to find" shape `Module` already established.
enum Block {
    Module(KirId),
    Symbol(KirId),
    Other,
}

fn current_module(stack: &[Block]) -> Option<KirId> {
    stack.iter().rev().find_map(|b| match b {
        Block::Module(id) => Some(*id),
        Block::Symbol(_) | Block::Other => None,
    })
}

/// Real doc-comment extraction (Phase 1 of the "Real Descriptions, Purpose, and Links" plan) —
/// single-line or `"""`-heredoc `@moduledoc`/`@doc` text, pre-scanned separately from the main
/// loop because a real heredoc spans multiple lines and the main loop has no lookahead.
///
/// `@moduledoc` and `@doc` sit in real, *different* structural positions relative to what they
/// document: `@moduledoc` is the first real statement *inside* the module it describes (after
/// `defmodule X do`, documenting the already-open enclosing module), while `@doc` *precedes* the
/// specific `def`/`defp` it documents. So this returns two separate maps rather than one:
/// `moduledoc` keyed by the `@moduledoc` line's own index (the main loop attaches it to whichever
/// module is currently open at that line), `doc` keyed by the line index right after the
/// attribute closes (the declaration line it precedes).
///
/// Deliberately not sigil-aware (`~S"""`, used to suppress interpolation) — the same accepted
/// "not sigil/heredoc-aware" limitation this module's own top-level doc comment already states
/// for block-depth tracking; a `~S"""`-prefixed doc is silently not captured rather than
/// misparsed. `@moduledoc false`/`@doc false` (explicit "no docs for this") is recognized and
/// intentionally produces no entry — distinct from "no `@moduledoc` at all," but both render the
/// same honest "not documented" downstream, so the distinction isn't preserved further.
struct DocComments {
    moduledoc: HashMap<usize, String>,
    doc: HashMap<usize, String>,
}

fn extract_doc_comments(source: &str) -> DocComments {
    let lines: Vec<&str> = source.lines().collect();
    let mut result = DocComments {
        moduledoc: HashMap::new(),
        doc: HashMap::new(),
    };
    let mut i = 0;
    while i < lines.len() {
        let trimmed = lines[i].trim();
        let is_moduledoc = trimmed.starts_with("@moduledoc ");
        let Some(rest) = trimmed
            .strip_prefix("@moduledoc ")
            .or_else(|| trimmed.strip_prefix("@doc "))
        else {
            i += 1;
            continue;
        };
        let attr_line = i;
        let rest = rest.trim();
        if rest == "false" {
            i += 1;
            continue;
        }
        let text = if let Some(after_open) = rest.strip_prefix("\"\"\"") {
            let mut text_lines: Vec<&str> = Vec::new();
            if !after_open.trim().is_empty() {
                text_lines.push(after_open.trim());
            }
            let mut j = i + 1;
            while j < lines.len() && lines[j].trim() != "\"\"\"" {
                text_lines.push(lines[j].trim());
                j += 1;
            }
            i = j + 1;
            text_lines.join(" ").trim().to_string()
        } else {
            i += 1;
            extract_quoted(rest).unwrap_or_default()
        };
        if text.is_empty() {
            continue;
        }
        if is_moduledoc {
            result.moduledoc.insert(attr_line, text);
        } else {
            // Real Elixir convention (and the shape this was found broken against, live: `lib/ip/
            // tools.ex`'s own `allowed?`/`ranges`) puts a `@spec` — and often a blank line either
            // side of it — between `@doc` and the `def`/`defp` it documents, not immediately next
            // to it. Skip blank lines and single-line `@spec ...` lines so `doc` is keyed at the
            // real declaration line, not the `@spec` line sitting in between. A multi-line `@spec`
            // (a wrapped type signature) is not unwound here — same accepted, documented limitation
            // as this function's other real-syntax gaps (no sigil/heredoc awareness for `~S"""`).
            let mut key = i;
            while key < lines.len() {
                let t = lines[key].trim();
                if t.is_empty() || t.starts_with("@spec ") || t.starts_with("@spec(") {
                    key += 1;
                    continue;
                }
                break;
            }
            result.doc.insert(key, text);
        }
    }
    result
}

/// A `"..."` single-line quoted string's content — no escape processing beyond the literal
/// characters between the first and last `"` on the line (real doc strings rarely need it; a
/// doc string containing an escaped `\"` is an accepted, documented miss, not a misparse).
fn extract_quoted(s: &str) -> Option<String> {
    let s = s.strip_prefix('"')?;
    let end = s.rfind('"')?;
    Some(s[..end].to_string())
}

fn parse_elixir_file(source: &str, file_id: KirId, project: Option<&str>) -> ElixirFileResult {
    let mut result = ElixirFileResult::default();
    let mut stack: Vec<Block> = Vec::new();
    let mut seen: HashSet<KirId> = HashSet::new();
    // RFC 0086 (Phase 6): modules that have declared `use Ecto.Repo` somewhere in their own body
    // — real Backend→Database evidence, tracked per-module so a later `adapter:` line (often 1-2
    // lines below the `use Ecto.Repo,` line itself, a real multi-line macro-call arg list) is
    // correctly attributed. Never cleared: a module doesn't stop being an Ecto Repo declarer for
    // the rest of its own body.
    let mut ecto_repo_modules: HashSet<KirId> = HashSet::new();
    // Phase 1 ("Real Descriptions, Purpose, and Links"): real `@moduledoc`/`@doc` text.
    let doc_comments = extract_doc_comments(source);
    // Real per-leaf expansion of multi-target `alias/import/require/use X.{A, B}` forms — see
    // `prescan_multi_alias_targets`'s own doc comment for why this needs a separate pre-scan.
    let multi_alias_targets = prescan_multi_alias_targets(source);
    // Index into `result.objects` for a module created *in this file* — `@moduledoc` mutates the
    // already-created object in place once it's encountered a few lines later (real Elixir puts
    // `@moduledoc` *inside* the module, after `defmodule X do`, not on the same line). Only
    // covers modules whose `defmodule` this file itself just processed (not ones merely
    // referenced) — mutating a module discovered only via `alias` elsewhere would need a
    // cross-file index this per-file pass doesn't have, the same accepted scope this file's
    // module-level doc comment already states for cross-file dedup ordering.
    let mut module_obj_index: HashMap<KirId, usize> = HashMap::new();
    // RFC 0088: real `source_span` (1-indexed start/end line) per symbol, keyed by the symbol's
    // own id, filled in as `adjust_depth` finds the matching `do`/`end` pair for a real `def`/
    // `defp`'s own body. `pending_symbol` is armed right after a *first-occurrence* def/defp line
    // is parsed and consumed by the very next `do`/`fn` token `adjust_depth` sees (which may be
    // on a later real line — a guard clause can span several, already handled generically below)
    // — arming unconditionally overwrites any earlier unconsumed value, so a one-line `, do:
    // expr` form (no block ever opens, `source_span` simply isn't recorded for it) can never
    // wrongly attach to a later, unrelated block.
    let mut pending_symbol: Option<(KirId, usize)> = None;
    let mut symbol_spans: HashMap<KirId, (usize, usize)> = HashMap::new();

    for (line_idx, raw_line) in source.lines().enumerate() {
        let line = strip_comment(raw_line);
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("defmodule ") {
            if let Some(name) = extract_module_name(rest) {
                let qualified = ekos_common::project::project_qualify(&name, project);
                let mod_id = elixir_module_kir_id(&qualified);
                if seen.insert(mod_id) {
                    let mut obj =
                        KirObject::new(name, ObjectKind::Custom("ElixirModule".to_string()));
                    obj.id = mod_id;
                    module_obj_index.insert(mod_id, result.objects.len());
                    result.objects.push(obj);
                    result.module_count += 1;
                }
                result.relationships.push(KirRelationship::deterministic(
                    RelationshipKind::Contains,
                    file_id,
                    mod_id,
                    "",
                ));
                stack.push(Block::Module(mod_id));
            }
            // A `defmodule ... do` line's only structurally relevant token is the `do` already
            // accounted for above — skip the generic scan below so it isn't double-counted.
            continue;
        }

        if let Some(doc) = doc_comments.moduledoc.get(&line_idx)
            && let Some(owner) = current_module(&stack)
            && let Some(&idx) = module_obj_index.get(&owner)
        {
            result.objects[idx]
                .properties
                .insert("description".into(), serde_json::json!(doc));
        }

        if trimmed.starts_with("use Ecto.Repo")
            && let Some(owner) = current_module(&stack)
        {
            ecto_repo_modules.insert(owner);
        }

        if let Some(adapter_raw) = extract_ecto_adapter(trimmed)
            && let Some(owner) = current_module(&stack)
            && ecto_repo_modules.contains(&owner)
        {
            let tech_name = normalize_ecto_adapter(adapter_raw);
            let tech_id = technology_kir_id(&tech_name);
            if seen.insert(tech_id) {
                let mut obj =
                    KirObject::new(tech_name, ObjectKind::Custom("Technology".to_string()));
                obj.id = tech_id;
                obj.properties
                    .insert("ecosystem".into(), serde_json::json!("database"));
                result.objects.push(obj);
            }
            result.relationships.push(KirRelationship::deterministic(
                RelationshipKind::DependsOn,
                owner,
                tech_id,
                "",
            ));
        }

        if let Some(kind) = def_kind(trimmed) {
            // RFC 0088: any earlier-armed `pending_symbol` this def/defp line hasn't reached yet
            // (e.g. a prior one-line `, do:` clause of the *same* multi-clause function, which
            // never opens a real block to consume it) must never survive into *this* line's own
            // block — otherwise a later real `do` could wrongly stitch together a start line from
            // one clause and an end line from another. Cleared unconditionally, re-armed below
            // only on a genuine first occurrence.
            pending_symbol = None;
            if let Some((name, arity)) = parse_def_line(trimmed, kind) {
                let owner = current_module(&stack).unwrap_or(file_id);
                let qualified = ekos_common::project::project_qualify(&name, project);
                let sym_id = elixir_symbol_kir_id(owner, &qualified, arity);
                if seen.insert(sym_id) {
                    let mut obj =
                        KirObject::new(name, ObjectKind::Custom("ElixirSymbol".to_string()))
                            // "kind" matches `rust_analyzer.rs`/`python_analyzer.rs`'s own
                            // property convention (`docs-gen`'s `render_api` reads it for the
                            // displayed entity kind) — every symbol this analyzer recognizes is a
                            // function (`def`/`defp`), no other Elixir declaration shape yet.
                            .with_property("kind", serde_json::json!("function"))
                            .with_property("arity", serde_json::json!(arity))
                            .with_property(
                                "visibility",
                                serde_json::json!(if kind == "defp" {
                                    "private"
                                } else {
                                    "public"
                                }),
                            );
                    obj.id = sym_id;
                    // Real, only when this exact def/defp had a real `@doc` immediately above it
                    // — never fabricated. Multi-clause functions (RFC 0081's own collapse-to-one-
                    // symbol rule) only get a description from whichever clause happens to carry
                    // the real `@doc` — real Elixir convention already puts `@doc` once, above the
                    // first clause, so this matches how the language itself expects it to be read.
                    if let Some(doc) = doc_comments.doc.get(&line_idx) {
                        obj.properties
                            .insert("description".into(), serde_json::json!(doc));
                    }
                    result.objects.push(obj);
                    result.symbol_count += 1;
                    // RFC 0088: arm the source-span tracker on this, the first (and only
                    // span-carrying) clause — the matching `do`/`end` pair is found generically
                    // below, possibly several real lines later (guard clause).
                    pending_symbol = Some((sym_id, line_idx + 1));
                }
                result.relationships.push(KirRelationship::deterministic(
                    RelationshipKind::Contains,
                    owner,
                    sym_id,
                    "",
                ));
            }
            // Fall through to the generic scan: this line's own trailing `do` (if the signature
            // and guard fit on one line) or lack of one (guard continues to a later line, or a
            // one-line `, do:` form) is handled uniformly below, same as any other line.
        } else if let Some(target) = extract_dependency_target(trimmed) {
            let owner = current_module(&stack).unwrap_or(file_id);
            // A multi-target `X.{A, B}` form (possibly wrapped across several following real
            // lines) resolves to its real per-leaf expansion when the pre-scan found one at this
            // exact starting line; otherwise `target` (a plain single-name `alias`/`import`/
            // `require`/`use`, or the shared-prefix fallback for a brace form the pre-scan
            // couldn't close) is the one real target, unchanged from before this fix.
            let targets = multi_alias_targets
                .get(&line_idx)
                .cloned()
                .unwrap_or_else(|| vec![target]);
            for raw_target in targets {
                let qualified = ekos_common::project::project_qualify(&raw_target, project);
                let target_id = elixir_module_kir_id(&qualified);
                if target_id != owner {
                    if seen.insert(target_id) {
                        let mut obj = KirObject::new(
                            raw_target,
                            ObjectKind::Custom("ElixirModule".to_string()),
                        );
                        obj.id = target_id;
                        result.objects.push(obj);
                    }
                    result.relationships.push(KirRelationship::deterministic(
                        RelationshipKind::DependsOn,
                        owner,
                        target_id,
                        "",
                    ));
                }
            }
        }

        adjust_depth(
            &mut stack,
            trimmed,
            line_idx,
            &mut pending_symbol,
            &mut symbol_spans,
        );
    }

    // RFC 0088: apply the real, now-fully-resolved source spans onto their matching symbol
    // objects — deferred to the end (rather than written at push time) because a span's own end
    // line isn't known until its closing `end` is reached, which can be many lines after the
    // object itself was created.
    for obj in &mut result.objects {
        if let Some(&(start, end)) = symbol_spans.get(&obj.id) {
            obj.properties.insert(
                "source_span".into(),
                serde_json::json!({"start_line": start, "end_line": end}),
            );
        }
    }

    result
}

/// Strips a `#`-comment, but only outside a `"`/`'`-quoted string — a real `#` inside `"a # b"`
/// must not truncate the line. Not sigil/heredoc-aware (`~s(...)`, `"""..."""`) — a documented,
/// accepted limitation matching this codebase's existing fallback-scanner honesty.
fn strip_comment(line: &str) -> &str {
    let mut in_string: Option<char> = None;
    let mut escaped = false;
    for (i, c) in line.char_indices() {
        if let Some(q) = in_string {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == q {
                in_string = None;
            }
        } else if c == '"' || c == '\'' {
            in_string = Some(c);
        } else if c == '#' {
            return &line[..i];
        }
    }
    line
}

fn extract_module_name(rest: &str) -> Option<String> {
    let name = rest.split_whitespace().next()?;
    if name.chars().next().is_some_and(|c| c.is_uppercase()) {
        Some(name.to_string())
    } else {
        None
    }
}

fn def_kind(line: &str) -> Option<&'static str> {
    if line.starts_with("def ") {
        Some("def")
    } else if line.starts_with("defp ") {
        Some("defp")
    } else {
        None
    }
}

fn parse_def_line(line: &str, kind: &str) -> Option<(String, usize)> {
    let prefix_len = kind.len() + 1;
    let rest = line.get(prefix_len..)?;
    let name_end = rest
        .find(|c: char| c == '(' || c.is_whitespace() || c == ',')
        .unwrap_or(rest.len());
    let name = &rest[..name_end];
    if name.is_empty()
        || !name
            .chars()
            .next()
            .is_some_and(|c| c.is_lowercase() || c == '_')
    {
        return None;
    }
    let arity = if rest[name_end..].trim_start().starts_with('(') {
        count_arity(&rest[name_end..])
    } else {
        0
    };
    Some((name.to_string(), arity))
}

/// Counts top-level commas inside the first `(...)` span, bracket-depth-aware (`()`/`[]`/`{}`, so
/// a default value, pattern match, or pinned var containing its own commas/brackets doesn't
/// inflate the count) — `0` for an empty or absent argument list.
fn count_arity(s: &str) -> usize {
    let chars: Vec<char> = s.chars().collect();
    let Some(start) = chars.iter().position(|&c| c == '(') else {
        return 0;
    };
    let mut depth = 0i32;
    let mut commas = 0usize;
    let mut has_content = false;
    for &c in &chars[start..] {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            ',' if depth == 1 => commas += 1,
            c if depth >= 1 && !c.is_whitespace() => has_content = true,
            _ => {}
        }
    }
    if has_content { commas + 1 } else { 0 }
}

/// Pre-scans real multi-target `alias/import/require/use X.{A, B, C}` forms into their full
/// per-leaf expansion (`X.A`, `X.B`, `X.C`) — separate from the main per-line loop for the same
/// reason `extract_doc_comments` is: a real brace list commonly wraps one leaf per line (`mix
/// format`'s own convention once a line gets long), so resolving it needs lookahead the main loop
/// doesn't have. A single-line form (`alias X.{A, B}`) is handled by the same code path (the
/// closing `}` is just found on the starting line instead of a later one) so the main loop has
/// exactly one lookup regardless of how the source wraps.
///
/// Returns a map from the *starting* line index (the line containing `X.{`) to the real qualified
/// leaf names. A brace list this scan never finds a closing `}` for (truncated file, or a form
/// this scanner's own documented limitations miss) simply has no entry — the caller falls back to
/// `extract_dependency_target`'s single shared-prefix target for that line, the same honest
/// degraded behavior this whole file already accepts elsewhere.
fn prescan_multi_alias_targets(source: &str) -> HashMap<usize, Vec<String>> {
    let lines: Vec<&str> = source.lines().collect();
    let mut result = HashMap::new();
    let mut i = 0;
    while i < lines.len() {
        let trimmed = strip_comment(lines[i]).trim();
        let Some(rest) = ["alias ", "import ", "require ", "use "]
            .iter()
            .find_map(|p| trimmed.strip_prefix(p))
        else {
            i += 1;
            continue;
        };
        let Some(brace_pos) = rest.find(".{") else {
            i += 1;
            continue;
        };
        let target_prefix = &rest[..brace_pos];
        if !target_prefix
            .chars()
            .next()
            .is_some_and(|c| c.is_uppercase())
        {
            i += 1;
            continue;
        }
        let start_line = i;
        let after_brace = &rest[brace_pos + 2..];
        let mut body = String::new();
        if let Some(close) = after_brace.find('}') {
            body.push_str(&after_brace[..close]);
            i += 1;
        } else {
            body.push_str(after_brace);
            i += 1;
            while i < lines.len() {
                let line = strip_comment(lines[i]).trim();
                body.push(' ');
                if let Some(close) = line.find('}') {
                    body.push_str(&line[..close]);
                    i += 1;
                    break;
                }
                body.push_str(line);
                i += 1;
            }
        }
        let leaves: Vec<String> = body
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| format!("{target_prefix}.{s}"))
            .collect();
        if !leaves.is_empty() {
            result.insert(start_line, leaves);
        }
    }
    result
}

/// Real dependency directives only — `plug`/other macro invocations that happen to name a
/// module are deliberately not recognized here (this analyzer reads declared dependencies, not
/// every macro call site, the same "read what's declared" scope as `crate_topology_analyzer.rs`).
fn extract_dependency_target(line: &str) -> Option<String> {
    for prefix in ["alias ", "import ", "require ", "use "] {
        if let Some(rest) = line.strip_prefix(prefix) {
            let first = rest
                .split(|c: char| c == ',' || c.is_whitespace())
                .find(|s| !s.is_empty())?;
            let target = first
                .split('{')
                .next()
                .unwrap_or(first)
                .trim_end_matches(['.', ':']);
            if target.chars().next().is_some_and(|c| c.is_uppercase()) {
                return Some(target.to_string());
            }
            return None;
        }
    }
    None
}

/// RFC 0086 (Phase 6): a real `adapter: Ecto.Adapters.X` config line — the actual, in-source
/// evidence of which database a given `Ecto.Repo` module talks to (confirmed against the real
/// analytics project's own real `use Ecto.Repo, adapter: Ecto.Adapters.Postgres`/`ClickHouse`
/// declarations before writing this). Requires the literal `adapter:` keyword on the same line as
/// `Ecto.Adapters.` — an unrelated call like `Ecto.Adapters.SQL.query!(...)` (a real line that
/// exists elsewhere in this same real codebase) must not be misread as a config declaration.
fn extract_ecto_adapter(line: &str) -> Option<&str> {
    let after_keyword = &line[line.find("adapter:")? + "adapter:".len()..];
    let name_start =
        &after_keyword[after_keyword.find("Ecto.Adapters.")? + "Ecto.Adapters.".len()..];
    let end = name_start
        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
        .unwrap_or(name_start.len());
    let name = &name_start[..end];
    if name.is_empty() { None } else { Some(name) }
}

/// Maps a real Ecto adapter module suffix to the same real-world technology name
/// `dependency_analyzer.rs`'s own `PATTERNS` table already uses ("PostgreSQL", title-case) where
/// one exists, so the two analyzers' output resolves to one real object. An adapter this table
/// doesn't recognize keeps its own real name rather than being dropped — honest, not guessed.
fn normalize_ecto_adapter(raw: &str) -> String {
    match raw {
        "Postgres" | "Postgres3" => "PostgreSQL".to_string(),
        "MyXQL" | "MySQL" => "MySQL".to_string(),
        "SQLite3" => "SQLite".to_string(),
        other => other.to_string(),
    }
}

/// Generic block-depth tracking: any bare `do`/`fn` token opens a block that needs a matching
/// `end`; `do:` (a distinct whitespace-split token) does not. `if`/`case`/`cond`/`with`/`try`/
/// `receive`/`for`/`defmodule`/`def`/`defp` themselves never push directly — only the `do` token
/// that (eventually, possibly on a later line for a guard clause) follows them does, which this
/// function alone is responsible for, regardless of which line matched a declaration above.
/// `line_idx` (0-indexed, the same convention every other per-line lookup in this file uses) is
/// only consulted for `source_span`'s own 1-indexed end line; `pending_symbol`/`spans` implement
/// RFC 0088's real per-symbol source-span capture — see `pending_symbol`'s own doc comment above
/// its declaration in `parse_elixir_file` for why arming is unconditional-overwrite, not queued.
fn adjust_depth(
    stack: &mut Vec<Block>,
    line: &str,
    line_idx: usize,
    pending_symbol: &mut Option<(KirId, usize)>,
    spans: &mut HashMap<KirId, (usize, usize)>,
) {
    for word in line.split_whitespace() {
        let w = word.trim_end_matches([')', ',', ';', ']', '}']);
        match w {
            "end" => {
                if let Some(Block::Symbol(sym_id)) = stack.pop()
                    && let Some(entry) = spans.get_mut(&sym_id)
                {
                    entry.1 = line_idx + 1;
                }
            }
            "do" | "fn" => {
                if let Some((sym_id, start_line)) = pending_symbol.take() {
                    stack.push(Block::Symbol(sym_id));
                    spans.insert(sym_id, (start_line, start_line));
                } else {
                    stack.push(Block::Other);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str) -> ElixirFileResult {
        parse_elixir_file(source, KirId(Uuid::new_v4()), None)
    }

    #[test]
    fn recognizes_a_module_and_its_public_function() {
        let result = parse(
            "defmodule Plausible.Auth.Password do\n  def hash(password) do\n    Bcrypt.hash_pwd_salt(password)\n  end\nend\n",
        );
        assert_eq!(result.module_count, 1);
        assert_eq!(result.symbol_count, 1);
        let module = result
            .objects
            .iter()
            .find(|o| o.name == "Plausible.Auth.Password")
            .unwrap();
        assert_eq!(module.kind, ObjectKind::Custom("ElixirModule".to_string()));
        let symbol = result.objects.iter().find(|o| o.name == "hash").unwrap();
        assert_eq!(symbol.kind, ObjectKind::Custom("ElixirSymbol".to_string()));
        assert_eq!(symbol.properties["kind"], "function");
        assert_eq!(symbol.properties["arity"], 1);
        assert_eq!(symbol.properties["visibility"], "public");
        assert!(
            result
                .relationships
                .iter()
                .any(|r| r.kind == RelationshipKind::Contains
                    && r.from == module.id
                    && r.to == symbol.id)
        );
    }

    #[test]
    fn recognizes_a_private_function_and_zero_arity() {
        let result = parse("defmodule M do\n  defp dummy do\n    :ok\n  end\nend\n");
        let symbol = result.objects.iter().find(|o| o.name == "dummy").unwrap();
        assert_eq!(symbol.properties["visibility"], "private");
        assert_eq!(symbol.properties["arity"], 0);
    }

    #[test]
    fn multiple_clauses_of_the_same_function_collapse_into_one_symbol() {
        let result = parse(
            "defmodule M do\n  def foo(1), do: :a\n  def foo(2), do: :b\n  def foo(x), do: x\nend\n",
        );
        assert_eq!(
            result.symbol_count, 1,
            "three clauses of foo/1 must collapse into one real symbol, matching Elixir's own \
             multi-clause dispatch semantics"
        );
    }

    #[test]
    fn arity_counts_top_level_commas_only_not_ones_inside_nested_patterns() {
        let result = parse("defmodule M do\n  def foo(%{a: a}, [x, y], z) do\n  end\nend\n");
        let symbol = result.objects.iter().find(|o| o.name == "foo").unwrap();
        assert_eq!(symbol.properties["arity"], 3);
    }

    #[test]
    fn alias_import_use_require_become_real_depends_on_edges() {
        let result = parse(
            "defmodule PlausibleWeb.AuthController do\n  use PlausibleWeb, :controller\n  alias Plausible.Auth\n  import Ecto.Query\n  require Logger\nend\n",
        );
        let module = result
            .objects
            .iter()
            .find(|o| o.name == "PlausibleWeb.AuthController")
            .unwrap();
        let targets: Vec<&str> = result
            .objects
            .iter()
            .filter(|o| {
                o.kind == ObjectKind::Custom("ElixirModule".to_string()) && o.id != module.id
            })
            .map(|o| o.name.as_str())
            .collect();
        assert!(targets.contains(&"PlausibleWeb"));
        assert!(targets.contains(&"Plausible.Auth"));
        assert!(targets.contains(&"Ecto.Query"));
        assert!(targets.contains(&"Logger"));
        assert_eq!(
            result
                .relationships
                .iter()
                .filter(|r| r.kind == RelationshipKind::DependsOn && r.from == module.id)
                .count(),
            4
        );
    }

    #[test]
    fn a_single_line_multi_alias_expands_to_one_edge_per_real_leaf() {
        let result =
            parse("defmodule Plausible.Auth do\n  alias Plausible.{Teams, Billing}\nend\n");
        let module = result
            .objects
            .iter()
            .find(|o| o.name == "Plausible.Auth")
            .unwrap();
        let targets: Vec<&str> = result
            .objects
            .iter()
            .filter(|o| {
                o.kind == ObjectKind::Custom("ElixirModule".to_string()) && o.id != module.id
            })
            .map(|o| o.name.as_str())
            .collect();
        assert!(targets.contains(&"Plausible.Teams"));
        assert!(targets.contains(&"Plausible.Billing"));
        assert!(
            !targets.contains(&"Plausible"),
            "the bare shared prefix must never become its own phantom object: {targets:?}"
        );
    }

    #[test]
    fn a_multi_line_wrapped_multi_alias_expands_to_one_edge_per_real_leaf() {
        // The real, common `mix format` shape for a long multi-alias list — one leaf per line,
        // the main per-line loop has no lookahead to see past the opening `{` on its own.
        let result = parse(
            "defmodule PlausibleWeb.Live.CustomerSupport.Team do\n  alias PlausibleWeb.CustomerSupport.Team.Components.{\n    Overview,\n    Billing,\n    SSO\n  }\nend\n",
        );
        let module = result
            .objects
            .iter()
            .find(|o| o.name == "PlausibleWeb.Live.CustomerSupport.Team")
            .unwrap();
        let targets: Vec<&str> = result
            .objects
            .iter()
            .filter(|o| {
                o.kind == ObjectKind::Custom("ElixirModule".to_string()) && o.id != module.id
            })
            .map(|o| o.name.as_str())
            .collect();
        assert!(targets.contains(&"PlausibleWeb.CustomerSupport.Team.Components.Overview"));
        assert!(targets.contains(&"PlausibleWeb.CustomerSupport.Team.Components.Billing"));
        assert!(targets.contains(&"PlausibleWeb.CustomerSupport.Team.Components.SSO"));
        assert!(
            !targets.contains(&"PlausibleWeb.CustomerSupport.Team.Components"),
            "the bare shared prefix must never become its own phantom object: {targets:?}"
        );
        assert_eq!(
            result
                .relationships
                .iter()
                .filter(|r| r.kind == RelationshipKind::DependsOn && r.from == module.id)
                .count(),
            3
        );
    }

    #[test]
    fn a_dependency_on_a_locally_defined_module_resolves_to_the_same_real_object() {
        // The real, valuable case: module A depends on module B, and B is *also* defined in this
        // codebase — both must resolve to one real linked object, not two disconnected ones.
        let result = parse(
            "defmodule Plausible.Teams do\n  def noop, do: :ok\nend\n\ndefmodule Plausible.Auth do\n  alias Plausible.Teams\nend\n",
        );
        let teams = result
            .objects
            .iter()
            .filter(|o| o.name == "Plausible.Teams")
            .count();
        assert_eq!(
            teams, 1,
            "Plausible.Teams must appear once, shared between its own declaration and the alias reference"
        );
    }

    #[test]
    fn a_guard_clause_spanning_to_a_later_line_does_not_desync_block_depth() {
        let result = parse(
            "defmodule M do\n  def foo(x)\n      when is_binary(x) do\n    x\n  end\n\n  def bar do\n    :ok\n  end\nend\n",
        );
        // If depth tracking desynced, `bar` would be attributed to the wrong scope or dropped.
        assert_eq!(result.module_count, 1);
        assert_eq!(result.symbol_count, 2);
        let module = result.objects.iter().find(|o| o.name == "M").unwrap();
        let bar = result.objects.iter().find(|o| o.name == "bar").unwrap();
        assert!(
            result
                .relationships
                .iter()
                .any(|r| r.kind == RelationshipKind::Contains
                    && r.from == module.id
                    && r.to == bar.id)
        );
    }

    #[test]
    fn an_inline_fn_end_on_one_line_does_not_desync_block_depth() {
        let result = parse(
            "defmodule M do\n  def run do\n    Enum.each([1, 2], fn x -> IO.puts(x) end)\n  end\n\n  def after_it do\n    :ok\n  end\nend\n",
        );
        assert_eq!(result.symbol_count, 2);
        let module = result.objects.iter().find(|o| o.name == "M").unwrap();
        let after = result
            .objects
            .iter()
            .find(|o| o.name == "after_it")
            .unwrap();
        assert!(
            result
                .relationships
                .iter()
                .any(|r| r.kind == RelationshipKind::Contains
                    && r.from == module.id
                    && r.to == after.id)
        );
    }

    #[test]
    fn a_comment_containing_keyword_like_text_is_ignored() {
        let result = parse("defmodule M do\n  # def fake_fn do end\n  def real, do: :ok\nend\n");
        assert_eq!(result.symbol_count, 1);
        assert!(result.objects.iter().find(|o| o.name == "real").is_some());
        assert!(result.objects.iter().all(|o| o.name != "fake_fn"));
    }

    #[test]
    fn a_project_field_qualifies_ids_but_not_displayed_names() {
        let a = parse_elixir_file(
            "defmodule M do\n  def foo, do: :ok\nend\n",
            KirId(Uuid::new_v4()),
            Some("service-a"),
        );
        let b = parse_elixir_file(
            "defmodule M do\n  def foo, do: :ok\nend\n",
            KirId(Uuid::new_v4()),
            Some("service-b"),
        );
        let mod_a = a.objects.iter().find(|o| o.name == "M").unwrap();
        let mod_b = b.objects.iter().find(|o| o.name == "M").unwrap();
        assert_eq!(mod_a.name, "M");
        assert_eq!(mod_b.name, "M");
        assert_ne!(
            mod_a.id, mod_b.id,
            "the same module name in two different projects must not collide"
        );
    }

    #[test]
    fn empty_source_produces_nothing() {
        let result = parse("");
        assert!(result.objects.is_empty());
        assert!(result.relationships.is_empty());
    }

    /// RFC 0086 (Phase 6): the real shape confirmed against the analytics project's own
    /// `lib/plausible/repo.ex` — `use Ecto.Repo,` then `adapter: Ecto.Adapters.Postgres` two
    /// lines later, inside the same open module.
    #[test]
    fn an_ecto_repo_adapter_becomes_a_real_backend_to_database_edge() {
        let result = parse(
            "defmodule Plausible.Repo do\n  use Ecto.Repo,\n    otp_app: :plausible,\n    adapter: Ecto.Adapters.Postgres\nend\n",
        );
        let module = result
            .objects
            .iter()
            .find(|o| o.name == "Plausible.Repo")
            .unwrap();
        let tech = result
            .objects
            .iter()
            .find(|o| o.kind == ObjectKind::Custom("Technology".to_string()))
            .unwrap();
        assert_eq!(tech.name, "PostgreSQL");
        assert_eq!(tech.properties["ecosystem"], "database");
        assert!(
            result
                .relationships
                .iter()
                .any(|r| r.kind == RelationshipKind::DependsOn
                    && r.from == module.id
                    && r.to == tech.id)
        );
    }

    #[test]
    fn a_clickhouse_ecto_repo_keeps_its_own_real_adapter_name() {
        let result = parse(
            "defmodule Plausible.ClickhouseRepo do\n  use Ecto.Repo,\n    adapter: Ecto.Adapters.ClickHouse,\n    read_only: true\nend\n",
        );
        let tech = result
            .objects
            .iter()
            .find(|o| o.kind == ObjectKind::Custom("Technology".to_string()))
            .unwrap();
        assert_eq!(tech.name, "ClickHouse");
    }

    /// A real, unrelated `Ecto.Adapters.SQL.query!(...)` call site (this exact line exists in
    /// the real analytics project's `lib/plausible/purge.ex`) must not be misread as a config
    /// declaration — no `adapter:` keyword precedes it on the line.
    #[test]
    fn an_unrelated_ecto_adapters_call_site_is_not_misread_as_a_config_line() {
        let result = parse(
            "defmodule M do\n  use Ecto.Repo,\n    adapter: Ecto.Adapters.Postgres\n  def f do\n    Ecto.Adapters.SQL.query!(M, \"SELECT 1\", [])\n  end\nend\n",
        );
        let tech_count = result
            .objects
            .iter()
            .filter(|o| o.kind == ObjectKind::Custom("Technology".to_string()))
            .count();
        assert_eq!(
            tech_count, 1,
            "only the real adapter: line should produce a Technology object"
        );
    }

    #[test]
    fn a_module_without_use_ecto_repo_never_gets_a_database_edge() {
        // A bare `adapter: Ecto.Adapters.X`-shaped line with no real `use Ecto.Repo` anywhere in
        // the module must not be treated as real database evidence — coincidental text shape is
        // not the same as a real Ecto Repo declaration.
        let result = parse(
            "defmodule M do\n  def f(adapter: Ecto.Adapters.Postgres) do\n    :ok\n  end\nend\n",
        );
        assert!(
            result
                .objects
                .iter()
                .all(|o| o.kind != ObjectKind::Custom("Technology".to_string()))
        );
    }

    #[test]
    fn a_single_line_moduledoc_becomes_a_real_description() {
        let result = parse(
            "defmodule Plausible.Auth.Password do\n  @moduledoc \"Handles password hashing.\"\n\n  def hash(pw) do\n    :ok\n  end\nend\n",
        );
        let module = result
            .objects
            .iter()
            .find(|o| o.name == "Plausible.Auth.Password")
            .unwrap();
        assert_eq!(
            module.properties["description"],
            "Handles password hashing."
        );
    }

    #[test]
    fn a_heredoc_moduledoc_joins_its_lines_into_one_real_description() {
        let result = parse(
            "defmodule M do\n  @moduledoc \"\"\"\n  Line one.\n  Line two.\n  \"\"\"\n\n  def f do\n    :ok\n  end\nend\n",
        );
        let module = result.objects.iter().find(|o| o.name == "M").unwrap();
        assert_eq!(module.properties["description"], "Line one. Line two.");
    }

    #[test]
    fn a_single_line_doc_attaches_to_the_next_real_function_only() {
        let result = parse(
            "defmodule M do\n  @doc \"Hashes a password.\"\n  def hash(pw) do\n    :ok\n  end\n\n  def other do\n    :ok\n  end\nend\n",
        );
        let hash_fn = result.objects.iter().find(|o| o.name == "hash").unwrap();
        assert_eq!(hash_fn.properties["description"], "Hashes a password.");
        let other_fn = result.objects.iter().find(|o| o.name == "other").unwrap();
        assert!(!other_fn.properties.contains_key("description"));
    }

    #[test]
    fn a_doc_still_attaches_across_a_real_spec_line_in_between() {
        // Real bug found live against `analytics/lib/ip/tools.ex`: `@doc` -> `@spec` -> `def` is
        // the standard real Elixir convention (credo's own style guide puts spec directly above
        // def, doc directly above spec) — every real public function in that file used this exact
        // shape, and every one of them silently lost its doc before this fix.
        let result = parse(
            "defmodule M do\n  @doc \"Hashes a password.\"\n  @spec hash(String.t()) :: String.t()\n  def hash(pw) do\n    :ok\n  end\nend\n",
        );
        let hash_fn = result.objects.iter().find(|o| o.name == "hash").unwrap();
        assert_eq!(hash_fn.properties["description"], "Hashes a password.");
    }

    #[test]
    fn a_doc_still_attaches_across_a_blank_line_then_a_spec_line() {
        // The other real shape found live: a blank line *and* a `@spec` both sit between `@doc`
        // and `def` (`analytics/lib/ip/tools.ex`'s own `allowed?`).
        let result = parse(
            "defmodule M do\n  @doc \"Checks validity.\"\n\n  @spec allowed?(String.t()) :: boolean()\n  def allowed?(ip) do\n    true\n  end\nend\n",
        );
        let f = result
            .objects
            .iter()
            .find(|o| o.name == "allowed?")
            .unwrap();
        assert_eq!(f.properties["description"], "Checks validity.");
    }

    #[test]
    fn moduledoc_false_and_doc_false_produce_no_description_not_the_literal_word_false() {
        let result = parse(
            "defmodule M do\n  @moduledoc false\n\n  @doc false\n  def f do\n    :ok\n  end\nend\n",
        );
        let module = result.objects.iter().find(|o| o.name == "M").unwrap();
        assert!(!module.properties.contains_key("description"));
        let f = result.objects.iter().find(|o| o.name == "f").unwrap();
        assert!(!f.properties.contains_key("description"));
    }

    #[test]
    fn a_function_with_no_real_doc_comment_has_no_description_property_at_all() {
        let result = parse("defmodule M do\n  def f do\n    :ok\n  end\nend\n");
        let f = result.objects.iter().find(|o| o.name == "f").unwrap();
        assert!(!f.properties.contains_key("description"));
    }

    // ── RFC 0088 — real source_span capture ─────────────────────────────────

    #[test]
    fn a_single_line_function_gets_a_real_source_span() {
        // Lines: 1 defmodule, 2 def, 3 body, 4 end, 5 end.
        let result = parse("defmodule M do\n  def f do\n    :ok\n  end\nend\n");
        let f = result.objects.iter().find(|o| o.name == "f").unwrap();
        assert_eq!(
            f.properties["source_span"],
            serde_json::json!({"start_line": 2, "end_line": 4})
        );
    }

    #[test]
    fn a_multi_line_function_body_gets_a_real_source_span() {
        let result =
            parse("defmodule M do\n  def f do\n    x = 1\n    y = 2\n    x + y\n  end\nend\n");
        let f = result.objects.iter().find(|o| o.name == "f").unwrap();
        assert_eq!(
            f.properties["source_span"],
            serde_json::json!({"start_line": 2, "end_line": 6})
        );
    }

    #[test]
    fn a_guard_clause_spanning_lines_still_gets_a_real_source_span_from_its_own_do() {
        // The real, live case this project's own multi-line `when` guard already covers for
        // depth tracking — the span's start is the real `def` line, not the later `do` line.
        let result =
            parse("defmodule M do\n  def foo(x)\n      when is_binary(x) do\n    x\n  end\nend\n");
        let foo = result.objects.iter().find(|o| o.name == "foo").unwrap();
        assert_eq!(
            foo.properties["source_span"],
            serde_json::json!({"start_line": 2, "end_line": 5})
        );
    }

    #[test]
    fn multiple_clauses_the_first_clauses_real_block_wins_the_source_span() {
        // RFC 0081's own multi-clause collapse-to-one-symbol rule — the span must come from the
        // first clause only, never overwritten by a later one (matching the same convention
        // already established for `@doc` attachment).
        let result = parse(
            "defmodule M do\n  def f(0) do\n    :zero\n  end\n\n  def f(x) do\n    x\n  end\nend\n",
        );
        let f = result.objects.iter().find(|o| o.name == "f").unwrap();
        assert_eq!(
            f.properties["source_span"],
            serde_json::json!({"start_line": 2, "end_line": 4})
        );
    }

    #[test]
    fn a_one_line_first_clause_leaves_no_span_even_when_a_later_clause_has_a_real_block() {
        // The honest, safer choice over guessing: when the *first* clause never opens a real
        // block (a one-line `, do:` form), the symbol gets no `source_span` at all, rather than
        // stitching a start line from clause 1 onto an end line from clause 2 — a real
        // mismatched-span bug this exact test caught live during implementation.
        let result =
            parse("defmodule M do\n  def f(0), do: :zero\n\n  def f(x) do\n    x\n  end\nend\n");
        let f = result.objects.iter().find(|o| o.name == "f").unwrap();
        assert!(!f.properties.contains_key("source_span"));
    }

    #[test]
    fn a_one_line_do_colon_function_never_desyncs_a_later_functions_real_span() {
        // The real risk this design has to guard against: `f`'s one-line `, do:` form never
        // opens a real block at all, so its own `pending_symbol` must not wrongly attach to
        // `g`'s real block below it.
        let result =
            parse("defmodule M do\n  def f(x), do: x\n\n  def g do\n    :ok\n  end\nend\n");
        let f = result.objects.iter().find(|o| o.name == "f").unwrap();
        assert!(!f.properties.contains_key("source_span"));
        let g = result.objects.iter().find(|o| o.name == "g").unwrap();
        assert_eq!(
            g.properties["source_span"],
            serde_json::json!({"start_line": 4, "end_line": 6})
        );
    }
}
