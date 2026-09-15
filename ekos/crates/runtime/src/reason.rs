//! RFC 0123 (Phase 4 of RFC 0118) — REASON: the Query Plan IR, the rules planner, and the typed
//! Evidence Set.
//!
//! A question is compiled — [`plan`] turns a [`QueryUnderstanding`](crate::retrieval) into a
//! typed [`QueryPlan`], [`execute`] runs it against the QUERY surface (RFC 0122) + the retrieval
//! seam (RFC 0119), and the result is an [`EvidenceSet`]: a flat list of atomic claims, each
//! traceable to a source fragment. Fully offline — the LLM only enters later, in
//! [`AiRuntime::reason`](crate::ai::AiRuntime), to *explain* the assembled evidence.

use crate::retrieval::{QueryType, QueryUnderstanding, StructuralOp, understand};
use crate::{Runtime, RuntimeError};
use ekos_compiler_core::Diagnostic;
use ekos_kir::KirId;
use serde::Serialize;
use std::collections::HashMap;

/// Default cap on [`EvidenceSet`] items — keeps a hub entity from flooding the set.
pub const DEFAULT_EVIDENCE_CAP: usize = 60;
/// Most neighbours a *supporting* (planner-added) neighbourhood may contribute before it is
/// skipped entirely as hub noise (RFC 0139 §3.0). Never applies to a traversal the question
/// actually asked for.
///
/// Two thirds of [`DEFAULT_EVIDENCE_CAP`]: background context that would fill most of the evidence
/// budget on its own is describing the corpus, not the question. The measured distribution on this
/// repo is sharply bimodal and agrees — of 36 gated neighbourhoods, 31 were size 46-47 (the `ekos`
/// hub, i.e. the whole crate graph) and only 5 were smaller. An earlier value of 12 also gated
/// those 5 mid-size neighbourhoods, costing legitimate questions their context for no benefit.
const MAX_SUPPORTING_NEIGHBORS: usize = DEFAULT_EVIDENCE_CAP * 2 / 3;
/// Hop depth a `Structural` plan traverses.
const STRUCTURAL_HOPS: u32 = 2;
/// The `attr` sentinel meaning "every fact about this entity" (→ [`Runtime::facts_of`]).
const ALL_FACTS: &str = "*";

// ── the IR ─────────────────────────────────────────────────────────────────

/// A reference to an entity in a [`PlanNode`] — either already bound by the planner (RFC 0121
/// resolution) or a mention bound at execution time by an earlier `Resolve` step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum EntityRef {
    Resolved(KirId),
    Mention(String),
}

/// One node of a compiled query plan.
#[derive(Debug, Clone, Serialize)]
pub enum PlanNode {
    /// Bind `mention` → a `KirId` (best retrieval hit) into the execution environment.
    Resolve { mention: String },
    /// Lexical retrieval; each hit becomes an evidence item.
    Search { query: String, limit: usize },
    /// Read one attribute (or `"*"` for all) of an entity.
    Fact { entity: EntityRef, attr: String },
    /// A named graph traversal from `seed`.
    Graph {
        op: StructuralOp,
        seed: EntityRef,
        hops: u32,
        /// RFC 0139 §3.6 — this traversal is *supporting context* the planner added on its own,
        /// not the thing the reader asked for. True for the neighbourhood auto-expansion the
        /// `Conceptual`/`Lexical` branch attaches around a search; false when the question itself
        /// was structural ("what depends on X", "what calls Y"), where the traversal *is* the
        /// answer.
        ///
        /// The distinction decides whether these claims can support an answer on their own. An
        /// auto-expanded neighbourhood of an incidentally-matched entity is decoration: asked
        /// "what port does the EKOS message broker listen on?", it supplies a list of real EKOS
        /// crates that answer nothing, and a model handed that as evidence will invent a port.
        #[serde(default)]
        supporting: bool,
    },
    /// Sequential steps; a later step sees bindings earlier steps made.
    Compose { steps: Vec<PlanNode> },
}

/// A compiled question.
#[derive(Debug, Clone, Serialize)]
pub struct QueryPlan {
    pub raw: String,
    pub query_type: QueryType,
    pub root: PlanNode,
    /// The planner's own confidence in the routing (`0.0..=1.0`).
    pub confidence: f32,
}

// ── the rules planner ──────────────────────────────────────────────────────

/// Which planner produced a [`QueryPlan`]. The [`PlannerTier::Llm`] tier (RFC 0118 §4.2,
/// `[query-planner] planner = "llm"`) is **not implemented in this phase** — the seam exists so
/// RFC 0124+ can add it without a signature change; today it falls back to the rules planner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlannerTier {
    /// The deterministic, offline rules planner ([`plan`]).
    #[default]
    Rules,
    /// An LLM emitting the same [`QueryPlan`] shape. Stub — falls back to [`PlannerTier::Rules`].
    Llm,
}

/// Compile `u` with the requested planner `tier`. Only [`PlannerTier::Rules`] is implemented;
/// [`PlannerTier::Llm`] falls back to it (RFC 0124).
pub fn plan_with(u: &QueryUnderstanding, tier: PlannerTier) -> QueryPlan {
    match tier {
        PlannerTier::Rules | PlannerTier::Llm => plan(u),
    }
}

/// Compile a [`QueryUnderstanding`] into a [`QueryPlan`]. Deterministic, offline; rules are tried
/// in a fixed order and the first match wins.
pub fn plan(u: &QueryUnderstanding) -> QueryPlan {
    let primary = u.primary_entity();
    let query = search_query(u);

    // A fact-attribute question ("what does X return", "X's columns") is routed on the keyword,
    // ahead of the RFC 0121 intent class — "what does …" otherwise classifies `Structural`
    // (`Dependencies`), which is not what the reader asked for.
    if let (Some(e), Some(attr)) = (primary, fact_attr(&u.keywords)) {
        return QueryPlan {
            raw: u.raw.clone(),
            query_type: u.query_type,
            root: PlanNode::Compose {
                steps: vec![
                    PlanNode::Fact {
                        entity: EntityRef::Resolved(e.id),
                        attr,
                    },
                    PlanNode::Fact {
                        entity: EntityRef::Resolved(e.id),
                        attr: ALL_FACTS.to_string(),
                    },
                ],
            },
            confidence: e.confidence,
        };
    }

    let (root, confidence) = match u.query_type {
        QueryType::Lookup => match primary {
            Some(e) => (
                PlanNode::Fact {
                    entity: EntityRef::Resolved(e.id),
                    attr: ALL_FACTS.to_string(),
                },
                e.confidence,
            ),
            None => (PlanNode::Search { query, limit: 20 }, 0.4),
        },

        QueryType::Structural => match (primary, u.structural_op) {
            (Some(e), Some(op)) => (
                PlanNode::Compose {
                    steps: vec![
                        PlanNode::Graph {
                            op,
                            seed: EntityRef::Resolved(e.id),
                            hops: STRUCTURAL_HOPS,
                            // The question was structural — this traversal is the answer.
                            supporting: false,
                        },
                        PlanNode::Fact {
                            entity: EntityRef::Resolved(e.id),
                            attr: ALL_FACTS.to_string(),
                        },
                    ],
                },
                e.confidence,
            ),
            _ => (PlanNode::Search { query, limit: 20 }, 0.4),
        },

        QueryType::Aggregate => (PlanNode::Search { query, limit: 50 }, 0.3),

        QueryType::Conceptual | QueryType::Lexical => {
            let search = PlanNode::Search {
                query: query.clone(),
                limit: 20,
            };
            match primary {
                Some(e) => (
                    PlanNode::Compose {
                        steps: vec![
                            search,
                            PlanNode::Graph {
                                op: StructuralOp::Neighborhood,
                                seed: EntityRef::Resolved(e.id),
                                hops: 1,
                                // Context the planner added around a search, not the answer.
                                supporting: true,
                            },
                        ],
                    },
                    0.7,
                ),
                None => (
                    search,
                    if u.query_type == QueryType::Conceptual {
                        0.5
                    } else {
                        0.6
                    },
                ),
            }
        }
    };

    QueryPlan {
        raw: u.raw.clone(),
        query_type: u.query_type,
        root,
        confidence,
    }
}

/// The BM25 query a plan should search with: the significant keywords if any survived, else the
/// raw question.
///
/// `pub` (RFC 0139 Phase 2's last open item) so a caller that needs to know what the *pipeline
/// itself* would search with — not the raw question — can ask this directly instead of
/// re-deriving it. `ekos-evals`' `agent_runner` is the motivating caller: it used to grade
/// `recall_at_10` against `RetrievalRequest::lexical(&scenario.question)`, a plain, unprocessed
/// sentence, while `plan()` above searches with exactly this function's output — a keyword-only
/// string with stopwords and punctuation already stripped by `understand()`. The two queries can
/// rank differently, so the recorded "what did retrieval find" list didn't always match what the
/// model was actually shown.
pub fn search_query(u: &QueryUnderstanding) -> String {
    if u.keywords.is_empty() {
        u.raw.clone()
    } else {
        u.keywords.join(" ")
    }
}

/// Map a fact-attribute keyword ("returns", "columns", …) to a well-known `properties` path.
/// A miss just means the plan falls through to a `Search`.
fn fact_attr(keywords: &[String]) -> Option<String> {
    for kw in keywords {
        let mapped = match kw.as_str() {
            "returns" | "return" | "returned" => "returns",
            "raises" | "raise" | "throws" | "throw" | "exception" | "exceptions" => "raises",
            "parameters" | "parameter" | "params" | "arguments" | "argument" | "args"
            | "accepts" => "parameters",
            "signature" => "signature",
            "columns" | "column" | "fields" | "field" => "columns",
            "type" => "type",
            _ => continue,
        };
        return Some(mapped.to_string());
    }
    None
}

// ── the Evidence Set ───────────────────────────────────────────────────────

/// One atomic, traceable claim assembled by [`execute`].
#[derive(Debug, Clone, Serialize)]
pub struct EvidenceItem {
    /// A short human-readable statement — `"orders.schema = \"public\""`,
    /// `"order_items — dependents of orders"`.
    pub claim: String,
    /// The structured form of the claim's value, or `Value::Null`.
    pub value: serde_json::Value,
    /// A `KirEvidence` id backing this claim, when the entity carries one.
    pub source: Option<KirId>,
    /// `"path:line"` / `"path"` / `""` when unknown.
    pub location: String,
    pub confidence: f32,
    /// The analyzer / source kind this claim's entity was recovered by (`properties["source_kind"]`
    /// / `["analyzer"]` / `["language"]`), or `""` when the object records none.
    pub extracted_by: String,
    /// The object this claim is about, when applicable.
    pub entity: Option<KirId>,
    /// RFC 0139 §3.6 — this claim came from a partial-term-overlap ("relaxed") retrieval hit, so
    /// it is a plausible candidate rather than support for the question's premise. An evidence set
    /// where *every* item is weak means the corpus did not answer the question, however full the
    /// set looks: see [`EvidenceSet::is_all_weak`].
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub weak: bool,
}

/// The typed output of [`execute`] — the input to the LLM's "explain this" step.
#[derive(Debug, Clone, Serialize)]
pub struct EvidenceSet {
    pub items: Vec<EvidenceItem>,
    #[serde(skip)]
    pub plan: QueryPlan,
    pub diagnostics: Vec<Diagnostic>,
}

impl EvidenceSet {
    /// True when the set holds nothing but partial-term-overlap matches (RFC 0139 §3.6).
    ///
    /// This is the guard that keeps §3.1's retrieval relaxation from becoming a fabrication engine.
    /// Relaxing the query made evidence sets non-empty for questions about things that do not
    /// exist — asked *"what port does the EKOS message broker listen on?"*, the pipeline now
    /// returns real EKOS crates that merely share a word, and a model handed that will invent a
    /// port. Measured live when relaxation shipped un-guarded: fabrications rose 10 → 15 on the
    /// RFC 0138 suite. An all-weak set must therefore be treated as "found nothing", not as
    /// evidence.
    pub fn is_all_weak(&self) -> bool {
        !self.items.is_empty() && self.items.iter().all(|i| i.weak)
    }

    /// Cap the item count, emitting an `RSN001` diagnostic if anything was dropped.
    pub fn truncate_to(&mut self, cap: usize) {
        if self.items.len() > cap {
            let dropped = self.items.len() - cap;
            self.items.truncate(cap);
            self.diagnostics.push(Diagnostic::warning(
                "RSN001",
                format!("evidence set truncated to {cap} items — {dropped} dropped"),
            ));
        }
    }

    /// Every distinct `source` evidence id — the "known evidence" set a citation is checked against.
    pub fn source_ids(&self) -> Vec<KirId> {
        let mut seen = std::collections::HashSet::new();
        self.items
            .iter()
            .filter_map(|i| i.source)
            .filter(|id| seen.insert(*id))
            .collect()
    }
}

// ── execution ──────────────────────────────────────────────────────────────

#[derive(Default)]
struct ExecCtx {
    bindings: HashMap<String, KirId>,
    last_resolved: Option<KirId>,
}

impl ExecCtx {
    fn resolve(&self, r: &EntityRef) -> Option<KirId> {
        match r {
            EntityRef::Resolved(id) => Some(*id),
            EntityRef::Mention(m) => self.bindings.get(m).copied().or(self.last_resolved),
        }
    }
}

/// Execute `plan` against the ledger `runtime` wraps, assembling an [`EvidenceSet`].
pub fn execute(plan: &QueryPlan, runtime: &Runtime) -> Result<EvidenceSet, RuntimeError> {
    let mut ctx = ExecCtx::default();
    let mut items = Vec::new();
    let mut diagnostics = Vec::new();
    if plan.query_type == QueryType::Aggregate {
        diagnostics.push(Diagnostic::info(
            "RSN005",
            "aggregate questions (\"how many …\", \"list all … by …\") are best answered by an EKL \
             COUNT / GROUP BY query — this plan falls back to a keyword search"
                .to_string(),
        ));
    }
    exec_node(&plan.root, runtime, &mut ctx, &mut items, &mut diagnostics)?;
    let mut set = EvidenceSet {
        items,
        plan: plan.clone(),
        diagnostics,
    };
    set.truncate_to(DEFAULT_EVIDENCE_CAP);
    Ok(set)
}

fn exec_node(
    node: &PlanNode,
    runtime: &Runtime,
    ctx: &mut ExecCtx,
    items: &mut Vec<EvidenceItem>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<(), RuntimeError> {
    match node {
        PlanNode::Resolve { mention } => {
            if let Some(hit) = runtime
                .retrieve(&crate::RetrievalRequest::lexical(mention.as_str()))?
                .hits
                .into_iter()
                .next()
            {
                ctx.bindings.insert(mention.clone(), hit.id);
                ctx.last_resolved = Some(hit.id);
            } else {
                diagnostics.push(Diagnostic::warning(
                    "RSN002",
                    format!("could not resolve mention {mention:?} to any object"),
                ));
            }
        }

        PlanNode::Search { query, limit } => {
            let mut req = crate::RetrievalRequest::lexical(query.as_str());
            req.limit = *limit;
            req.per_arm_limit = (*limit).max(req.per_arm_limit);
            let hits = runtime.retrieve(&req)?;
            for hit in hits.hits.into_iter().take(*limit) {
                // RFC 0139 §3.6/§3.7: a hit that matched too little of the query shares vocabulary
                // with the question without being evidence the thing asked about exists. The
                // ledger applies the coverage threshold and marks such hits `Bm25Relaxed`;
                // labelling them in the claim keeps the distinction visible to the model, and
                // `weak` lets the caller tell an all-weak evidence set from a real one.
                let weak = hit
                    .signals
                    .iter()
                    .any(|s| s.source == ekos_ledger::SignalSource::Bm25Relaxed);
                let claim = if weak {
                    format!("possible search match (partial term overlap): {}", hit.name)
                } else {
                    format!("search match: {}", hit.name)
                };
                let mut item = entity_item_with_excerpt(
                    runtime,
                    hit.id,
                    claim,
                    serde_json::Value::String(hit.name),
                )?;
                item.weak = weak;
                items.push(item);
            }
        }

        PlanNode::Fact { entity, attr } => {
            let Some(id) = ctx.resolve(entity) else {
                diagnostics.push(Diagnostic::warning(
                    "RSN003",
                    "fact step had no entity to read".to_string(),
                ));
                return Ok(());
            };
            let name = runtime
                .load_object(&id)?
                .map(|o| o.name)
                .unwrap_or_else(|| "?".to_string());
            if attr == ALL_FACTS {
                for (k, v) in runtime.facts_of(&id)? {
                    items.push(entity_item(
                        runtime,
                        id,
                        format!("{name}.{k} = {}", render_value(&v)),
                        v,
                    )?);
                }
            } else if let Some(v) = runtime.fact(&id, attr)? {
                items.push(entity_item(
                    runtime,
                    id,
                    format!("{name}.{attr} = {}", render_value(&v)),
                    v,
                )?);
            } else {
                diagnostics.push(Diagnostic::info(
                    "RSN004",
                    format!("{name} has no {attr:?} fact"),
                ));
            }
        }

        PlanNode::Graph {
            op,
            seed,
            hops,
            supporting,
        } => {
            let Some(id) = ctx.resolve(seed) else {
                diagnostics.push(Diagnostic::warning(
                    "RSN003",
                    "graph step had no seed to traverse from".to_string(),
                ));
                return Ok(());
            };
            let seed_name = runtime
                .load_object(&id)?
                .map(|o| o.name)
                .unwrap_or_else(|| "?".to_string());
            let label = op_label(*op);
            let neighbors = runtime.graph_op(*op, &id, *hops)?;

            // RFC 0139 §3.0 — the entity gate.
            //
            // A *supporting* neighbourhood is context the planner attached around a search, not
            // something the reader asked for. When the entity it expands is a hub, that context
            // stops being informative: in an EKOS workspace the token "ekos" appears in nearly
            // every question, resolves to the `ekos` object, and its 1-hop neighbourhood is the
            // whole crate graph. Measured on the RFC 0138 suite, that made **26 completely
            // different questions receive byte-identical evidence**, and gave adversarial
            // questions ("what port does the message broker listen on?") a list of real crates to
            // fabricate from.
            //
            // Gating on neighbourhood size rather than on a name-blocklist keeps it
            // self-justifying: if one expansion would consume most of the evidence budget, it is
            // describing the corpus, not the question. A *requested* traversal ("what depends on
            // X") is never gated — there the size is the answer.
            if *supporting && neighbors.len() > MAX_SUPPORTING_NEIGHBORS {
                diagnostics.push(Diagnostic::warning(
                    "RSN007",
                    format!(
                        "skipped supporting neighbourhood of {seed_name} — {} neighbours exceeds \
                         the {MAX_SUPPORTING_NEIGHBORS}-item budget for background context; a hub \
                         entity's neighbourhood describes the corpus, not this question",
                        neighbors.len()
                    ),
                ));
                return Ok(());
            }

            for obj in neighbors {
                let item = entity_item_with_excerpt(
                    runtime,
                    obj.id,
                    format!("{} — {label} {seed_name}", obj.name),
                    serde_json::Value::String(obj.id.0.to_string()),
                )?;
                // `weak` stays false here: an ungated supporting neighbourhood is small enough to
                // be real context, and marking it weak was measured twice to collapse legitimate
                // answers (`code` correctness 72.7% -> 18.2%). The gate above is the lever instead.
                items.push(item);
            }
        }

        PlanNode::Compose { steps } => {
            for step in steps {
                exec_node(step, runtime, ctx, items, diagnostics)?;
            }
        }
    }
    Ok(())
}

/// Bound on the extra descriptive text [`entity_item_with_excerpt`] appends to a claim (RFC
/// 0139-followup "B1" fix). Deliberately smaller than RFC 0140 §3's [`MAX_SOURCE_TEXT_LINES`]:
/// this is the always-on cheap path applied to every `Search`/`Graph` hit, not the opt-in
/// per-question "expensive tier" full source-text enrichment.
const CLAIM_EXCERPT_MAX_CHARS: usize = 280;

/// Build an evidence item about `id`, pulling its first evidence fragment for provenance.
fn entity_item(
    runtime: &Runtime,
    id: KirId,
    claim: String,
    value: serde_json::Value,
) -> Result<EvidenceItem, RuntimeError> {
    entity_item_inner(runtime, id, claim, value, false)
}

/// Same as [`entity_item`], but for a `Search`/`Graph`-sourced claim: appends a bounded slice of
/// the object's own retrieved/indexed prose (RFC 0139-followup "B1" fix — see [`excerpt_of`]) so
/// the reasoner sees what retrieval actually surfaced, not just a bare name. `Fact`-sourced claims
/// don't go through this path since they already render the fact's real value.
fn entity_item_with_excerpt(
    runtime: &Runtime,
    id: KirId,
    claim: String,
    value: serde_json::Value,
) -> Result<EvidenceItem, RuntimeError> {
    entity_item_inner(runtime, id, claim, value, true)
}

fn entity_item_inner(
    runtime: &Runtime,
    id: KirId,
    claim: String,
    value: serde_json::Value,
    with_excerpt: bool,
) -> Result<EvidenceItem, RuntimeError> {
    let state = runtime.reconstruct_state(&id)?;
    let claim = match (
        with_excerpt,
        state.as_ref().and_then(|s| excerpt_of(&s.object)),
    ) {
        (true, Some(excerpt)) => format!("{claim} — {excerpt}"),
        _ => claim,
    };
    let (source, location, confidence, extracted_by) = match state {
        Some(state) => {
            let extracted_by = provenance_of(&state.object);
            // RFC 0140 §2 — prefer the object's own `source_span` over the evidence's file-level
            // location. Analyzers record a real start/end line for Rust, Elixir and Python symbols
            // (RFC 0088) but only `docs-gen` ever read it; meanwhile 33 of 35 evidence call sites
            // use `SourceLocation::file`, so measured on the RFC 0138 suite **zero** of 1,289
            // rendered claims carried a line number and only 26% carried any location at all. The
            // span was already compiled and simply never reached the reasoner.
            let span = span_of(&state.object);
            match state.evidence.first() {
                Some(ev) => (
                    Some(ev.id),
                    span_location(&ev.location, span),
                    ev.confidence,
                    extracted_by,
                ),
                None => (None, String::new(), 0.5, extracted_by),
            }
        }
        None => (None, String::new(), 0.5, String::new()),
    };
    Ok(EvidenceItem {
        claim,
        value,
        source,
        location,
        confidence,
        extracted_by,
        entity: Some(id),
        // Callers that know a claim came from a partial-term-overlap hit set this themselves; a
        // fact/graph claim is never weak, since it was reached structurally rather than by
        // vocabulary similarity.
        weak: false,
    })
}

/// The retrieved/indexed prose for `object`, if it carries any (RFC 0139-followup "B1" fix): the
/// `excerpt` property `local_docs_analyzer` writes on every Document/Section, or — for anything
/// with a `symbol_kind` (a `RustSymbol`/`PythonSymbol`/`ElixirSymbol`) — its `description`. This
/// is exactly the text BM25 indexed via `KirObject::indexed_content()`: previously matched,
/// ranked, and then discarded one line before the claim was rendered. Truncated to
/// [`CLAIM_EXCERPT_MAX_CHARS`], never longer.
fn excerpt_of(object: &ekos_kir::KirObject) -> Option<String> {
    let text = object
        .properties
        .get("excerpt")
        .and_then(|v| v.as_str())
        .or_else(|| {
            object
                .properties
                .get("description")
                .and_then(|v| v.as_str())
        })?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(truncate_chars(trimmed, CLAIM_EXCERPT_MAX_CHARS))
}

/// Truncates `s` to at most `max` chars (not bytes — safe on multi-byte UTF-8), appending an
/// ellipsis when it actually cut something off.
fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push('…');
    out
}

/// The analyzer / source kind an object was recovered by, from the first provenance property it
/// carries — `""` when it records none.
fn provenance_of(obj: &ekos_kir::KirObject) -> String {
    for key in ["source_kind", "analyzer", "language"] {
        if let Some(v) = obj.properties.get(key).and_then(|v| v.as_str())
            && !v.is_empty()
        {
            return v.to_string();
        }
    }
    String::new()
}

/// The `(start_line, end_line)` an analyzer recorded for this object, when it recorded one
/// (RFC 0088's `source_span`, written by `rust_analyzer`/`elixir_analyzer`/`python_analyzer`).
fn span_of(object: &ekos_kir::KirObject) -> Option<(u64, u64)> {
    let v = object.properties.get("source_span")?;
    Some((v.get("start_line")?.as_u64()?, v.get("end_line")?.as_u64()?))
}

/// Render a claim's location, upgrading a file-level evidence location to the object's real line
/// span when one exists (RFC 0140 §2). An evidence location that already carries its own line is
/// left alone — it is the more specific statement about *that* fragment.
fn span_location(loc: &ekos_kir::SourceLocation, span: Option<(u64, u64)>) -> String {
    match (loc.line, span) {
        (None, Some((start, end))) if end > start => format!("{}:{}-{}", loc.path, start, end),
        (None, Some((start, _))) => format!("{}:{}", loc.path, start),
        _ => fmt_location(loc),
    }
}

fn fmt_location(loc: &ekos_kir::SourceLocation) -> String {
    match loc.line {
        Some(line) => format!("{}:{}", loc.path, line),
        None => loc.path.clone(),
    }
}

// ── RFC 0140 §3: on-demand source text ──────────────────────────────────────

/// Cap on how many lines [`attach_source_text`] attaches per entity — generous relative to
/// `source_evidence`'s 40-line compile-time cap on every span-carrying symbol (this is RFC 0140's
/// deliberately "expensive tier": bounded to a handful of already-retrieved entities, not applied
/// to all of them), but still bounded — nothing stops a real function from spanning thousands of
/// lines, and no single item should be able to consume the whole evidence budget on its own.
const MAX_SOURCE_TEXT_LINES: u64 = 400;

/// RFC 0140 §3 — on-demand, (near-)uncapped source text for the first `top_k` *distinct* entities
/// already named in `set.items`, read from the artifact store rather than the live filesystem
/// (RFC 0043: artifact content already passed redaction at observation time; a disk read at query
/// time would be a new raw-content entry point that never did). Each entity that yields real text
/// gets exactly one additional [`EvidenceItem`] appended — nothing already in `set` is modified or
/// removed, and `set`'s existing item order/count is untouched below `top_k` appended items.
///
/// This is a best-effort enrichment, not a required one: an entity with no `source_span` (RFC
/// 0088 — only Rust/Python/Elixir symbols have one), no recorded `source_artifact_id` (RFC 0135
/// Part B), or whose backing artifact can't be read or doesn't have the expected
/// `{"data": {"source": "..."}}` shape is silently skipped, exactly as if it were never
/// considered — never a hard error, since the evidence set it enriches must still be usable
/// without it.
pub(crate) fn attach_source_text(
    set: &mut EvidenceSet,
    runtime: &Runtime,
    store: &dyn ekos_artifact::ArtifactStore,
    top_k: usize,
) {
    let mut seen = std::collections::HashSet::new();
    let mut additions = Vec::new();
    for item in &set.items {
        if seen.len() >= top_k {
            break;
        }
        let Some(id) = item.entity else { continue };
        if !seen.insert(id) {
            continue;
        }
        if let Some(addition) = source_text_item(runtime, store, id) {
            additions.push(addition);
        }
    }
    set.items.extend(additions);
}

fn source_text_item(
    runtime: &Runtime,
    store: &dyn ekos_artifact::ArtifactStore,
    id: KirId,
) -> Option<EvidenceItem> {
    let state = runtime.reconstruct_state(&id).ok()??;
    let (start, end) = span_of(&state.object)?;
    let path = state.evidence.first()?.location.path.clone();
    let artifact_id = latest_source_artifact_id(runtime, &id)?;
    let artifact = store.read(&artifact_id).ok()??;
    let source = artifact.get("data")?.get("source")?.as_str()?;
    let end_capped = end.min(start + MAX_SOURCE_TEXT_LINES - 1);
    let text = full_span_text(source, start, end_capped);
    if text.is_empty() {
        return None;
    }
    Some(EvidenceItem {
        claim: format!(
            "full source text of `{}` ({path}:{start}-{end_capped}):\n{text}",
            state.object.name
        ),
        value: serde_json::Value::String(text),
        source: state.evidence.first().map(|e| e.id),
        location: format!("{path}:{start}-{end_capped}"),
        confidence: 1.0,
        extracted_by: provenance_of(&state.object),
        entity: Some(id),
        weak: false,
    })
}

/// The artifact a symbol's *most recent* write descends from — `audit_trail` is ordered oldest
/// first, so the last record carrying a `source_artifact_id` is the one to trust if the entity was
/// ever re-recovered from a changed file.
fn latest_source_artifact_id(runtime: &Runtime, id: &KirId) -> Option<ekos_artifact::ArtifactId> {
    runtime
        .audit_trail(id)
        .ok()?
        .into_iter()
        .rev()
        .find_map(|r| r.source_artifact_id)
        .map(ekos_artifact::ArtifactId)
}

/// The text of a 1-indexed, inclusive line range — same slicing as
/// `ekos_recovery::source_evidence::slice_lines`, duplicated rather than reused: that helper is
/// private to the `recovery` crate and caps at 40 lines, the wrong cap for this on-demand read.
fn full_span_text(source: &str, start: u64, end: u64) -> String {
    let take = (end.saturating_sub(start) + 1) as usize;
    source
        .lines()
        .skip(start.saturating_sub(1) as usize)
        .take(take)
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_value(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn op_label(op: StructuralOp) -> &'static str {
    match op {
        StructuralOp::Dependents | StructuralOp::Impact => "dependents of",
        StructuralOp::Dependencies => "dependency of",
        StructuralOp::Callers => "caller of",
        StructuralOp::Neighborhood => "related to",
    }
}

/// Understand + plan `question` against the ledger `runtime` wraps.
pub fn plan_question(question: &str, runtime: &Runtime) -> Result<QueryPlan, RuntimeError> {
    Ok(plan(&understand(question, runtime)?))
}

/// Render an [`EvidenceSet`] as the numbered, cite-able context block the LLM sees.
pub fn render_evidence(set: &EvidenceSet) -> String {
    if set.items.is_empty() {
        return "(no structured evidence was found for this question)".to_string();
    }
    let mut out = String::new();
    for (i, item) in set.items.iter().enumerate() {
        let loc = if item.location.is_empty() {
            String::new()
        } else {
            format!(" [{}]", item.location)
        };
        let src = match item.source {
            Some(id) => format!(" (evidence {id})"),
            None => String::new(),
        };
        out.push_str(&format!("{}. {}{loc}{src}\n", i + 1, item.claim));
    }
    out
}

/// Render a [`QueryPlan`] as an indented human-readable tree — the `--explain` output shared by
/// `ekos ask --explain` and `ekos query find --explain`.
pub fn render_plan(plan: &QueryPlan) -> String {
    fn ref_str(r: &EntityRef) -> String {
        match r {
            EntityRef::Resolved(id) => format!("#{id}"),
            EntityRef::Mention(m) => format!("?{m:?}"),
        }
    }
    fn node(out: &mut String, n: &PlanNode, indent: usize) {
        let pad = "  ".repeat(indent);
        match n {
            PlanNode::Resolve { mention } => out.push_str(&format!("{pad}Resolve {mention:?}\n")),
            PlanNode::Search { query, limit } => {
                out.push_str(&format!("{pad}Search {query:?} (limit {limit})\n"))
            }
            PlanNode::Fact { entity, attr } => {
                out.push_str(&format!("{pad}Fact {}.{attr}\n", ref_str(entity)))
            }
            PlanNode::Graph { op, seed, hops, .. } => out.push_str(&format!(
                "{pad}Graph {op:?} from {} ({hops} hops)\n",
                ref_str(seed)
            )),
            PlanNode::Compose { steps } => {
                out.push_str(&format!("{pad}Compose\n"));
                for s in steps {
                    node(out, s, indent + 1);
                }
            }
        }
    }
    let mut out = format!(
        "query type: {:?}\nrouting confidence: {:.2}\nplan:\n",
        plan.query_type, plan.confidence
    );
    node(&mut out, &plan.root, 1);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::retrieval::understand;
    use ekos_kir::{
        KirEvidence, KirObject, KirRelationship, ObjectKind, RelationshipKind, SourceLocation,
    };
    use ekos_ledger::Ledger;
    use tempfile::TempDir;

    fn temp() -> (Ledger, TempDir) {
        let dir = TempDir::new().unwrap();
        (Ledger::open(&dir.path().join("l.db")).unwrap(), dir)
    }

    /// a <-Calls- b <-DependsOn- c ; `orders` table with a schema property + evidence.
    fn seed(l: &Ledger) -> (KirId, KirId, KirId, KirId) {
        let ev = KirEvidence::new(SourceLocation::at("schema.sql", 12), "CREATE TABLE orders");
        l.append_evidence(&ev).unwrap();
        let mut orders = KirObject::new("orders", ObjectKind::Table);
        orders
            .properties
            .insert("schema".into(), serde_json::json!("public"));
        orders
            .properties
            .insert("source_kind".into(), serde_json::json!("sql"));
        orders.evidence.push(ev.id);
        let (a, b, c) = (
            KirObject::new("alpha_fn", ObjectKind::Custom("Symbol".into())),
            KirObject::new("beta_fn", ObjectKind::Custom("Symbol".into())),
            KirObject::new("gamma_fn", ObjectKind::Custom("Symbol".into())),
        );
        for o in [&orders, &a, &b, &c] {
            l.append_object(o).unwrap();
        }
        l.append_relationship(&KirRelationship::new(RelationshipKind::Calls, a.id, b.id))
            .unwrap();
        l.append_relationship(&KirRelationship::new(
            RelationshipKind::DependsOn,
            b.id,
            c.id,
        ))
        .unwrap();
        (orders.id, a.id, b.id, c.id)
    }

    /// Shape assertions against a `PlanNode` — enough to pin every planner rule.
    fn root_is_fact_star(root: &PlanNode) -> bool {
        matches!(root, PlanNode::Fact { attr, .. } if attr == ALL_FACTS)
    }
    fn root_is_search(root: &PlanNode) -> bool {
        matches!(root, PlanNode::Search { .. })
    }
    /// `Compose[Graph{op}, Fact "*"]` — the `Structural` shape.
    fn root_is_graph_then_facts(root: &PlanNode, want_op: StructuralOp) -> bool {
        matches!(root, PlanNode::Compose { steps }
            if matches!(&steps[..], [PlanNode::Graph { op, .. }, PlanNode::Fact { attr, .. }]
                if *op == want_op && attr == ALL_FACTS))
    }
    /// `Compose[Fact{attr}, Fact "*"]` — the fact-attribute shape.
    fn root_is_attr_then_facts(root: &PlanNode, want_attr: &str) -> bool {
        matches!(root, PlanNode::Compose { steps }
            if matches!(&steps[..], [PlanNode::Fact { attr: a, .. }, PlanNode::Fact { attr: b, .. }]
                if a == want_attr && b == ALL_FACTS))
    }
    /// `Compose[Search, Graph{Neighborhood}]` — conceptual/lexical-with-entity.
    fn root_is_search_then_neighborhood(root: &PlanNode) -> bool {
        matches!(root, PlanNode::Compose { steps }
            if matches!(&steps[..], [PlanNode::Search { .. },
                PlanNode::Graph { op: StructuralOp::Neighborhood, .. }]))
    }

    #[test]
    fn planner_routes_by_query_shape() {
        let (l, _d) = temp();
        seed(&l);
        let rt = Runtime::new(&l);
        let p = |q: &str| plan(&understand(q, &rt).unwrap());

        // ── Lookup: a bare exact name → Fact "*", high confidence ──
        let lk = p("orders");
        assert_eq!(lk.query_type, QueryType::Lookup);
        assert!(root_is_fact_star(&lk.root));
        assert!(lk.confidence > 0.99);

        // ── Structural: every op → Compose[Graph{op}, Fact "*"] ──
        for (q, op) in [
            ("what depends on the orders table", StructuralOp::Dependents),
            ("dependencies of alpha_fn", StructuralOp::Dependencies),
            ("callers of alpha_fn", StructuralOp::Callers),
            ("what breaks if we drop orders", StructuralOp::Impact),
            ("what is related to orders", StructuralOp::Neighborhood),
        ] {
            let plan = p(q);
            assert_eq!(plan.query_type, QueryType::Structural, "{q:?}");
            assert!(
                root_is_graph_then_facts(&plan.root, op),
                "{q:?} → {:?}",
                plan.root
            );
        }

        // ── fact-attribute: every keyword → Compose[Fact{mapped}, Fact "*"] ──
        // (fires ahead of the `Structural` class — "what does …" would otherwise route there).
        for (q, attr) in [
            ("what does alpha_fn return", "returns"),
            ("what exceptions does alpha_fn raise", "raises"),
            ("alpha_fn parameters", "parameters"),
            ("alpha_fn signature", "signature"),
            ("orders columns", "columns"),
        ] {
            let plan = p(q);
            assert!(
                root_is_attr_then_facts(&plan.root, attr),
                "{q:?} → {:?}",
                plan.root
            );
        }

        // ── Aggregate → low-confidence Search + the EKL diagnostic ──
        let ag = p("how many tables are there");
        assert_eq!(ag.query_type, QueryType::Aggregate);
        assert!(ag.confidence < 0.5);
        assert!(root_is_search(&ag.root));

        // ── Conceptual with a dominant entity → Compose[Search, Graph{Neighborhood}] ──
        let ce = p("how does alpha_fn work");
        assert!(root_is_search_then_neighborhood(&ce.root), "{:?}", ce.root);

        // ── Conceptual with no entity → a bare Search ──
        let cn = p("how does authentication work");
        assert_eq!(cn.query_type, QueryType::Conceptual);
        assert!(root_is_search(&cn.root));
    }

    #[test]
    fn plan_with_llm_tier_falls_back_to_rules() {
        let (l, _d) = temp();
        seed(&l);
        let rt = Runtime::new(&l);
        let u = understand("orders", &rt).unwrap();
        let rules = plan_with(&u, PlannerTier::Rules);
        let llm = plan_with(&u, PlannerTier::Llm);
        assert_eq!(rules.query_type, llm.query_type);
        assert!(root_is_fact_star(&llm.root));
    }

    #[test]
    fn execute_fact_star_yields_one_item_per_fact_with_provenance() {
        let (l, _d) = temp();
        let (orders, ..) = seed(&l);
        let rt = Runtime::new(&l);

        let plan = QueryPlan {
            raw: "orders".into(),
            query_type: QueryType::Lookup,
            root: PlanNode::Fact {
                entity: EntityRef::Resolved(orders),
                attr: "*".into(),
            },
            confidence: 1.0,
        };
        let set = execute(&plan, &rt).unwrap();
        // name + kind + schema + source_kind
        assert!(set.items.iter().any(|i| i.claim == "orders.name = orders"));
        assert!(
            set.items
                .iter()
                .any(|i| i.claim == "orders.schema = public")
        );
        // every item is about `orders` and carries the seeded evidence location + provenance
        assert!(set.items.iter().all(|i| i.entity == Some(orders)));
        assert!(set.items.iter().all(|i| i.location == "schema.sql:12"));
        assert!(set.items.iter().all(|i| i.extracted_by == "sql"));
        assert_eq!(set.source_ids().len(), 1);
    }

    /// RFC 0140 §2 — a symbol's compiled `source_span` reaches the reasoner as a line range.
    ///
    /// Measured before this existed: of 1,289 evidence claims rendered across a full RFC 0138 run,
    /// **zero** carried a line number and only 26.4% carried any location. The spans were already
    /// in the ledger — `rust_analyzer` and friends write them for RFC 0088 — but nothing on the
    /// query path read them, so answers could cite a file and never a place in it.
    #[test]
    fn a_symbol_span_upgrades_a_file_level_location_to_a_line_range() {
        let (l, _d) = temp();
        let ev = KirEvidence::new(SourceLocation::file("src/lib.rs"), "fn parse() {}");
        l.append_evidence(&ev).unwrap();
        let mut obj = KirObject::new("parse", ObjectKind::Custom("RustSymbol".into()));
        obj.properties.insert(
            "source_span".into(),
            serde_json::json!({"start_line": 40, "end_line": 76}),
        );
        obj.evidence.push(ev.id);
        l.append_object(&obj).unwrap();
        let rt = Runtime::new(&l);

        let item = entity_item(&rt, obj.id, "c".into(), serde_json::Value::Null).unwrap();
        assert_eq!(item.location, "src/lib.rs:40-76");
    }

    #[test]
    fn an_evidence_location_that_already_has_a_line_is_left_alone() {
        // The fragment's own line is a more specific statement than the whole symbol's span.
        let (l, _d) = temp();
        let ev = KirEvidence::new(SourceLocation::at("src/lib.rs", 12), "fn parse() {}");
        l.append_evidence(&ev).unwrap();
        let mut obj = KirObject::new("parse2", ObjectKind::Custom("RustSymbol".into()));
        obj.properties.insert(
            "source_span".into(),
            serde_json::json!({"start_line": 40, "end_line": 76}),
        );
        obj.evidence.push(ev.id);
        l.append_object(&obj).unwrap();
        let rt = Runtime::new(&l);

        let item = entity_item(&rt, obj.id, "c".into(), serde_json::Value::Null).unwrap();
        assert_eq!(item.location, "src/lib.rs:12");
    }

    // ── RFC 0139-followup "B1" fix: Search/Graph claims carry retrieved prose ───────────────

    #[test]
    fn excerpt_of_prefers_excerpt_over_description() {
        let mut obj = KirObject::new("doc", ObjectKind::Custom("Section".into()));
        obj.properties
            .insert("description".into(), serde_json::json!("fallback text"));
        obj.properties
            .insert("excerpt".into(), serde_json::json!("real excerpt text"));
        assert_eq!(excerpt_of(&obj).as_deref(), Some("real excerpt text"));
    }

    #[test]
    fn excerpt_of_falls_back_to_description_for_a_symbol() {
        let mut obj = KirObject::new("parse", ObjectKind::Custom("RustSymbol".into()));
        obj.properties
            .insert("description".into(), serde_json::json!("parses input"));
        assert_eq!(excerpt_of(&obj).as_deref(), Some("parses input"));
    }

    #[test]
    fn excerpt_of_is_none_for_an_object_with_neither_property() {
        let obj = KirObject::new("bare", ObjectKind::Table);
        assert_eq!(excerpt_of(&obj), None);
    }

    #[test]
    fn excerpt_of_is_none_for_blank_text() {
        let mut obj = KirObject::new("blank", ObjectKind::Custom("Section".into()));
        obj.properties
            .insert("excerpt".into(), serde_json::json!("   "));
        assert_eq!(excerpt_of(&obj), None);
    }

    #[test]
    fn excerpt_of_truncates_long_text_and_marks_the_cut() {
        let mut obj = KirObject::new("long", ObjectKind::Custom("Section".into()));
        let long_text = "x".repeat(CLAIM_EXCERPT_MAX_CHARS + 50);
        obj.properties
            .insert("excerpt".into(), serde_json::json!(long_text));
        let excerpt = excerpt_of(&obj).unwrap();
        assert_eq!(excerpt.chars().count(), CLAIM_EXCERPT_MAX_CHARS + 1); // +1 for the ellipsis
        assert!(excerpt.ends_with('…'));
    }

    /// The single largest yield in a full 59-scenario failure classification: 21 real scenarios
    /// failed purely because the compiled/indexed/retrieved excerpt was discarded one line before
    /// the prompt was built, leaving the model nothing but a bare name to reason from.
    #[test]
    fn a_search_hit_carries_its_retrieved_excerpt_not_just_its_name() {
        let (l, _d) = temp();
        let mut obj = KirObject::new("widget_parser", ObjectKind::Custom("Section".into()));
        obj.properties.insert(
            "excerpt".into(),
            serde_json::json!(
                "widget_parser converts raw widget feeds into normalized gadget records."
            ),
        );
        l.append_object(&obj).unwrap();
        let rt = Runtime::new(&l);

        let plan = QueryPlan {
            raw: "widget_parser".into(),
            query_type: QueryType::Lexical,
            root: PlanNode::Search {
                query: "widget_parser".into(),
                limit: 5,
            },
            confidence: 0.5,
        };
        let set = execute(&plan, &rt).unwrap();
        let item = set
            .items
            .iter()
            .find(|i| i.entity == Some(obj.id))
            .expect("widget_parser should be retrieved");
        assert!(
            item.claim.contains("normalized gadget records"),
            "claim should carry the retrieved excerpt, not just the name: {:?}",
            item.claim
        );
    }

    #[test]
    fn a_graph_traversal_hit_carries_its_retrieved_excerpt_too() {
        let (l, _d) = temp();
        let (_orders, _a, b, c) = seed(&l);
        let mut beta = KirObject::new("beta_fn", ObjectKind::Custom("Symbol".into()));
        beta.id = b;
        beta.properties.insert(
            "description".into(),
            serde_json::json!("beta_fn aggregates results for the reporting job."),
        );
        l.append_object(&beta).unwrap();
        let rt = Runtime::new(&l);

        let plan = QueryPlan {
            raw: "what depends on gamma_fn".into(),
            query_type: QueryType::Structural,
            root: PlanNode::Graph {
                op: StructuralOp::Dependents,
                seed: EntityRef::Resolved(c),
                hops: 3,
                supporting: false,
            },
            confidence: 1.0,
        };
        let set = execute(&plan, &rt).unwrap();
        let item = set
            .items
            .iter()
            .find(|i| i.entity == Some(b))
            .expect("beta_fn should be in the traversal");
        assert!(
            item.claim
                .contains("aggregates results for the reporting job"),
            "graph claim should carry the retrieved excerpt too: {:?}",
            item.claim
        );
    }

    /// RFC 0139 §3.0 — the entity gate. A hub's neighbourhood describes the corpus, not the
    /// question; on the real suite an ungated one made 26 different questions receive
    /// byte-identical evidence and handed adversarial questions a list of real objects to
    /// fabricate from.
    #[test]
    fn a_supporting_neighbourhood_of_a_hub_entity_is_skipped() {
        let (l, _d) = temp();
        let hub = KirObject::new("hub", ObjectKind::Custom("Crate".into()));
        l.append_object(&hub).unwrap();
        for i in 0..(MAX_SUPPORTING_NEIGHBORS + 5) {
            let leaf = KirObject::new(format!("leaf_{i}"), ObjectKind::Custom("Symbol".into()));
            l.append_object(&leaf).unwrap();
            l.append_relationship(&KirRelationship::new(
                RelationshipKind::DependsOn,
                leaf.id,
                hub.id,
            ))
            .unwrap();
        }
        let rt = Runtime::new(&l);
        let graph = |supporting| QueryPlan {
            raw: "hub".into(),
            query_type: QueryType::Lexical,
            root: PlanNode::Graph {
                op: StructuralOp::Neighborhood,
                seed: EntityRef::Resolved(hub.id),
                hops: 1,
                supporting,
            },
            confidence: 1.0,
        };

        let gated = execute(&graph(true), &rt).unwrap();
        assert!(
            gated.items.is_empty(),
            "a hub's *supporting* neighbourhood must contribute nothing: {:?}",
            gated.items.iter().map(|i| &i.claim).collect::<Vec<_>>()
        );
        assert!(
            gated.diagnostics.iter().any(|d| d.code == "RSN007"),
            "the skip must be visible as a diagnostic, not silent"
        );

        // The same traversal, when the question actually asked for it, is the answer — never gated.
        let requested = execute(&graph(false), &rt).unwrap();
        assert!(
            requested.items.len() > MAX_SUPPORTING_NEIGHBORS,
            "a requested traversal must not be gated by size — there the size is the answer"
        );
    }

    #[test]
    fn a_small_supporting_neighbourhood_is_still_kept_as_context() {
        let (l, _d) = temp();
        let (_orders, a, b, _c) = seed(&l);
        let rt = Runtime::new(&l);
        let plan = QueryPlan {
            raw: "beta_fn".into(),
            query_type: QueryType::Lexical,
            root: PlanNode::Graph {
                op: StructuralOp::Neighborhood,
                seed: EntityRef::Resolved(b),
                hops: 1,
                supporting: true,
            },
            confidence: 1.0,
        };
        let set = execute(&plan, &rt).unwrap();
        assert!(
            set.items.iter().any(|i| i.entity == Some(a)),
            "a specific entity's small neighbourhood is real context and must survive the gate"
        );
    }

    #[test]
    fn execute_graph_dependents_walks_inward() {
        let (l, _d) = temp();
        let (_orders, _a, _b, c) = seed(&l);
        let rt = Runtime::new(&l);

        let plan = QueryPlan {
            raw: "what depends on gamma_fn".into(),
            query_type: QueryType::Structural,
            root: PlanNode::Graph {
                op: StructuralOp::Dependents,
                seed: EntityRef::Resolved(c),
                hops: 3,
                supporting: false,
            },
            confidence: 1.0,
        };
        let set = execute(&plan, &rt).unwrap();
        let claims: Vec<&str> = set.items.iter().map(|i| i.claim.as_str()).collect();
        assert!(
            claims
                .iter()
                .any(|c| c.starts_with("beta_fn — dependents of gamma_fn"))
        );
        assert!(
            claims
                .iter()
                .any(|c| c.starts_with("alpha_fn — dependents of gamma_fn"))
        );
    }

    #[test]
    fn execute_compose_binds_a_resolve_into_a_later_mention() {
        let (l, _d) = temp();
        seed(&l);
        let rt = Runtime::new(&l);

        let plan = QueryPlan {
            raw: "gamma_fn callers".into(),
            query_type: QueryType::Structural,
            root: PlanNode::Compose {
                steps: vec![
                    PlanNode::Resolve {
                        mention: "gamma_fn".into(),
                    },
                    PlanNode::Graph {
                        op: StructuralOp::Dependents,
                        seed: EntityRef::Mention("gamma_fn".into()),
                        hops: 3,
                        supporting: false,
                    },
                ],
            },
            confidence: 1.0,
        };
        let set = execute(&plan, &rt).unwrap();
        assert!(
            set.items
                .iter()
                .any(|i| i.claim.starts_with("beta_fn — dependents of gamma_fn"))
        );
    }

    #[test]
    fn evidence_set_truncates_and_diagnoses() {
        let (l, _d) = temp();
        // one hub entity with far more facts than the cap — `Fact "*"` yields name + kind + one
        // item per property.
        let mut hub = KirObject::new("hub", ObjectKind::Table);
        for i in 0..(DEFAULT_EVIDENCE_CAP + 20) {
            hub.properties
                .insert(format!("p{i:03}"), serde_json::json!(i));
        }
        l.append_object(&hub).unwrap();
        let rt = Runtime::new(&l);
        let plan = QueryPlan {
            raw: "hub".into(),
            query_type: QueryType::Lookup,
            root: PlanNode::Fact {
                entity: EntityRef::Resolved(hub.id),
                attr: "*".into(),
            },
            confidence: 1.0,
        };
        let set = execute(&plan, &rt).unwrap();
        assert_eq!(set.items.len(), DEFAULT_EVIDENCE_CAP);
        assert!(set.diagnostics.iter().any(|d| d.code == "RSN001"));
    }

    #[test]
    fn execute_aggregate_plan_points_at_ekl() {
        let (l, _d) = temp();
        seed(&l);
        let rt = Runtime::new(&l);
        let plan = plan(&understand("how many tables are there", &rt).unwrap());
        assert_eq!(plan.query_type, QueryType::Aggregate);
        let set = execute(&plan, &rt).unwrap();
        assert!(set.diagnostics.iter().any(|d| d.code == "RSN005"));
    }

    // ── RFC 0140 §3: on-demand source text ──────────────────────────────────

    mod source_text {
        use super::*;
        use ekos_artifact::{ArtifactId, ArtifactStore, FileSystemArtifactStore};
        use ekos_ledger::provenance::WriteContext;

        fn empty_plan() -> QueryPlan {
            QueryPlan {
                raw: String::new(),
                query_type: QueryType::Lexical,
                root: PlanNode::Search {
                    query: String::new(),
                    limit: 1,
                },
                confidence: 0.0,
            }
        }

        fn evidence_set_for(entities: &[KirId]) -> EvidenceSet {
            EvidenceSet {
                items: entities
                    .iter()
                    .map(|&id| EvidenceItem {
                        claim: "search match".into(),
                        value: serde_json::Value::Null,
                        source: None,
                        location: String::new(),
                        confidence: 0.5,
                        extracted_by: String::new(),
                        entity: Some(id),
                        weak: false,
                    })
                    .collect(),
                plan: empty_plan(),
                diagnostics: Vec::new(),
            }
        }

        /// Writes a real Rust symbol (a `source_span`-carrying object, matching what
        /// `rust_analyzer.rs` produces) whose write is attributed to a real artifact via
        /// `WriteContext` — the same mechanism `commit.rs` uses in production — and stashes that
        /// artifact's real source text under a matching id in a real, on-disk `ArtifactStore`.
        fn seed_span_carrying_symbol(
            l: &Ledger,
            store_dir: &std::path::Path,
            artifact_id: &str,
            source: &str,
        ) -> KirId {
            let ev = KirEvidence::new(SourceLocation::at("src/lib.rs", 2), "fn parse_thing() {");
            l.append_evidence(&ev).unwrap();
            let mut obj = KirObject::new("parse_thing", ObjectKind::Custom("RustSymbol".into()));
            obj.properties.insert(
                "source_span".into(),
                serde_json::json!({"start_line": 2, "end_line": 4}),
            );
            obj.evidence.push(ev.id);
            let id = obj.id;
            l.set_write_context(Some(WriteContext {
                run_id: "run-1".into(),
                stage: "recover".into(),
                source_artifact_id: Some(artifact_id.to_string()),
            }));
            l.append_object(&obj).unwrap();
            l.set_write_context(None);

            let store = FileSystemArtifactStore::new(store_dir);
            store
                .write(
                    &ArtifactId(artifact_id.to_string()),
                    &serde_json::json!({"data": {"path": "src/lib.rs", "source": source}}),
                )
                .unwrap();
            id
        }

        #[test]
        fn attaches_real_source_text_for_a_span_carrying_entity() {
            let (l, _d) = temp();
            let store_dir = TempDir::new().unwrap();
            let id = seed_span_carrying_symbol(
                &l,
                store_dir.path(),
                "art-1",
                "fn unrelated() {}\nfn parse_thing() {\n    1\n}\n",
            );
            let rt = Runtime::new(&l);
            let store = FileSystemArtifactStore::new(store_dir.path());

            let mut set = evidence_set_for(&[id]);
            let before = set.items.len();
            attach_source_text(&mut set, &rt, &store, 3);

            assert_eq!(
                set.items.len(),
                before + 1,
                "one item appended, none removed"
            );
            let added = set.items.last().unwrap();
            assert_eq!(added.entity, Some(id));
            assert!(
                added.claim.contains("fn parse_thing() {\n    1\n}"),
                "must contain the real sliced source text: {}",
                added.claim
            );
            assert!(
                !added.claim.contains("fn unrelated"),
                "must not include lines outside the recorded span: {}",
                added.claim
            );
            assert_eq!(added.location, "src/lib.rs:2-4");
        }

        #[test]
        fn an_entity_with_no_source_span_is_silently_skipped() {
            let (l, _d) = temp();
            let store_dir = TempDir::new().unwrap();
            // A Table object never carries `source_span` (RFC 0088 is Rust/Python/Elixir-only).
            let obj = KirObject::new("orders", ObjectKind::Table);
            let id = obj.id;
            l.append_object(&obj).unwrap();
            let rt = Runtime::new(&l);
            let store = FileSystemArtifactStore::new(store_dir.path());

            let mut set = evidence_set_for(&[id]);
            let before = set.items.len();
            attach_source_text(&mut set, &rt, &store, 3);
            assert_eq!(
                set.items.len(),
                before,
                "nothing to attach — must not fabricate an item"
            );
        }

        #[test]
        fn an_entity_with_no_recorded_source_artifact_is_silently_skipped() {
            let (l, _d) = temp();
            let store_dir = TempDir::new().unwrap();
            // Span-carrying, but written with no WriteContext at all (pre-RFC-0135 shape).
            let mut obj = KirObject::new("parse_thing", ObjectKind::Custom("RustSymbol".into()));
            obj.properties.insert(
                "source_span".into(),
                serde_json::json!({"start_line": 1, "end_line": 1}),
            );
            let id = obj.id;
            l.append_object(&obj).unwrap();
            let rt = Runtime::new(&l);
            let store = FileSystemArtifactStore::new(store_dir.path());

            let mut set = evidence_set_for(&[id]);
            let before = set.items.len();
            attach_source_text(&mut set, &rt, &store, 3);
            assert_eq!(set.items.len(), before);
        }

        #[test]
        fn top_k_bounds_how_many_distinct_entities_are_attempted() {
            let (l, _d) = temp();
            let store_dir = TempDir::new().unwrap();
            let ids: Vec<KirId> = (0..5)
                .map(|i| {
                    seed_span_carrying_symbol(
                        &l,
                        store_dir.path(),
                        &format!("art-{i}"),
                        "fn parse_thing() {\n    1\n}\n",
                    )
                })
                .collect();
            let rt = Runtime::new(&l);
            let store = FileSystemArtifactStore::new(store_dir.path());

            let mut set = evidence_set_for(&ids);
            let before = set.items.len();
            attach_source_text(&mut set, &rt, &store, 2);
            assert_eq!(
                set.items.len(),
                before + 2,
                "top_k=2 must attempt exactly 2 distinct entities, not all 5"
            );
        }

        #[test]
        fn the_same_entity_referenced_by_multiple_items_is_only_attempted_once() {
            let (l, _d) = temp();
            let store_dir = TempDir::new().unwrap();
            let id = seed_span_carrying_symbol(
                &l,
                store_dir.path(),
                "art-1",
                "fn parse_thing() {\n    1\n}\n",
            );
            let rt = Runtime::new(&l);
            let store = FileSystemArtifactStore::new(store_dir.path());

            // The same entity named by two separate evidence items (e.g. a search hit and a
            // dependents claim) must still only cost one artifact read / one appended item.
            let mut set = evidence_set_for(&[id, id]);
            let before = set.items.len();
            attach_source_text(&mut set, &rt, &store, 3);
            assert_eq!(set.items.len(), before + 1);
        }
    }
}
