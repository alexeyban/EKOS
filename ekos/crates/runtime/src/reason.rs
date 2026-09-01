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
fn search_query(u: &QueryUnderstanding) -> String {
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
                items.push(entity_item(
                    runtime,
                    hit.id,
                    format!("search match: {}", hit.name),
                    serde_json::Value::String(hit.name),
                )?);
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

        PlanNode::Graph { op, seed, hops } => {
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
            for obj in runtime.graph_op(*op, &id, *hops)? {
                items.push(entity_item(
                    runtime,
                    obj.id,
                    format!("{} — {label} {seed_name}", obj.name),
                    serde_json::Value::String(obj.id.0.to_string()),
                )?);
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

/// Build an evidence item about `id`, pulling its first evidence fragment for provenance.
fn entity_item(
    runtime: &Runtime,
    id: KirId,
    claim: String,
    value: serde_json::Value,
) -> Result<EvidenceItem, RuntimeError> {
    let (source, location, confidence, extracted_by) = match runtime.reconstruct_state(&id)? {
        Some(state) => {
            let extracted_by = provenance_of(&state.object);
            match state.evidence.first() {
                Some(ev) => (
                    Some(ev.id),
                    fmt_location(&ev.location),
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
    })
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

fn fmt_location(loc: &ekos_kir::SourceLocation) -> String {
    match loc.line {
        Some(line) => format!("{}:{}", loc.path, line),
        None => loc.path.clone(),
    }
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
            PlanNode::Graph { op, seed, hops } => out.push_str(&format!(
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
}
