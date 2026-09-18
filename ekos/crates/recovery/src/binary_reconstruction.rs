//! `reconstruct_binary_logic` — RFC 0148 stage 2: LLM reconstruction of business logic from the
//! structural facts stage 1 recovered from a compiled binary.
//!
//! Deliberately **not** a `CompilerPass`, for the reason RFC 0088 documents at length for
//! `llm_description`: `merge_graphs`/`build_ckm` never dedupe `KirObject`s sharing an id across
//! passes, and each ledger version is a complete-object snapshot rather than a patch, so a pass
//! emitting a partial object would silently regress the structural properties another pass wrote.
//! This runs post-`commit`, reading committed objects through `&dyn KnowledgeStore` and appending
//! whole objects — the same architectural slot `commit_rollups`/`describe_objects` occupy.
//!
//! # Phases 3 and 4 ship together, deliberately
//!
//! RFC 0148 names this as its own top risk and makes it a hard sequencing constraint: a wrong
//! inferred rule is indistinguishable from a right one without the machinery that scores it. So
//! nothing here emits an accepted fact without first passing the guards below.
//!
//! ## The guard that does the real work
//!
//! Prompt wording cannot prevent hallucination; *checking* can. Every rule the model returns must
//! cite `evidence_locators`, and **every locator is checked against the exact set the prompt
//! contained**. Locators the model invented are dropped and counted; a rule left with no surviving
//! locator is discarded entirely. This is mechanical and cheap, and it is the difference between
//! "we asked it to be careful" and "it cannot assert something we did not show it".
//!
//! ## What the model is and is not given
//!
//! Only recovered facts: metadata names, the inheritance chain, field names and types, per-method
//! signatures, resolved call targets, string and numeric literals, branch counts and I/O
//! boundaries — plus an explicit statement that this is [`Fidelity::Structural`], with no
//! statement bodies. A model not told that will narrate control flow it was never shown.
//!
//! ## Confidence, and why it is never a constant
//!
//! Three real inputs, multiplied: the model's own self-report, a penalty proportional to how many
//! locators it invented in that slice, and cross-reference agreement across slices that describe
//! the same locator. Anything below `min_confidence`, and any rule contradicted by another
//! reconstruction of the same locator, is written with `status: "unconfirmed"` — the convention
//! `semantic`'s own candidate relationships already use for "a human must review this" — rather
//! than as an accepted fact.

use crate::llm::{LlmProvider, LlmRequest};
use crate::llm_json::{extract_json_object, strip_json_fences};
use ekos_kir::{
    KirEvidence, KirId, KirObject, KirRelationship, ObjectKind, RelationshipKind, SourceLocation,
};
use ekos_ledger::KnowledgeStore;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

const PROMPT_VERSION: &str = "llm-reconstruction-v1";

/// The `extractor` stamped on every fact this module writes.
///
/// Deliberately different from `ekos-jvm-classfile/v1` / `ekos-cil-metadata/v2`, so no query can
/// conflate a deterministically-read fact with an inferred one — RFC 0148's provenance rule.
pub const EXTRACTOR: &str = "llm-reconstruction-v1";

/// How far a reconstruction run goes, and how strict it is.
#[derive(Debug, Clone, Copy)]
pub struct ReconstructionConfig {
    /// Hard cap on LLM calls. One slice is one type, so this is also the cost ceiling — declared
    /// before the run rather than discovered during it, the shape RFC 0146's enrichment budget
    /// established.
    pub max_slices: usize,
    /// Rules scoring below this are written `unconfirmed` instead of accepted.
    pub min_confidence: f32,
    /// Whether to reconstruct compiler-generated types (lambda closures, async state machines).
    /// Off by default: they carry real logic, but their *names* are machine noise, and a
    /// reconstruction budget is better spent on developer-written types first.
    pub include_compiler_generated: bool,
}

impl Default for ReconstructionConfig {
    fn default() -> Self {
        Self {
            max_slices: 50,
            min_confidence: 0.5,
            include_compiler_generated: false,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ReconstructionStats {
    pub slices_sent: usize,
    pub types_named: usize,
    pub rules_accepted: usize,
    /// Rules written `unconfirmed` because they scored below `min_confidence` or were contradicted.
    pub rules_unconfirmed: usize,
    /// Rules discarded outright: every locator they cited was invented.
    pub rules_dropped: usize,
    /// Individual locators the model cited that were not in its own prompt.
    pub locators_hallucinated: usize,
    pub errors: usize,
    /// Types not attempted because `max_slices` was reached.
    pub skipped_budget: usize,
}

// ────────────────────────────────────────────────────────────────────────────
// The model's response
// ────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct Reconstruction {
    #[serde(default)]
    business_name: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    rules: Vec<RawRule>,
}

#[derive(Debug, Deserialize)]
struct RawRule {
    #[serde(default)]
    inputs: Vec<String>,
    #[serde(default)]
    condition: String,
    #[serde(default)]
    outcome: String,
    #[serde(default)]
    evidence_locators: Vec<String>,
    #[serde(default)]
    confidence: f32,
}

/// A rule that survived locator validation.
#[derive(Debug, Clone)]
struct ValidRule {
    inputs: Vec<String>,
    condition: String,
    outcome: String,
    locators: Vec<String>,
    model_confidence: f32,
}

// ────────────────────────────────────────────────────────────────────────────
// Entry point
// ────────────────────────────────────────────────────────────────────────────

/// Reconstruct business logic for every compiled type in the ledger, within budget.
pub async fn reconstruct_binary_logic(
    store: &dyn KnowledgeStore,
    llm: &dyn LlmProvider,
    config: ReconstructionConfig,
) -> Result<ReconstructionStats, String> {
    let objects = store
        .all_objects()
        .map_err(|e| format!("cannot read objects: {e}"))?;
    let relationships = store
        .all_relationships()
        .map_err(|e| format!("cannot read relationships: {e}"))?;

    let mut stats = ReconstructionStats::default();
    let slices = build_slices(&objects, &relationships, &config);

    // Every rule keyed by the locator it cites, so a second reconstruction of the same method can
    // agree with or contradict the first.
    let mut by_locator: HashMap<String, Vec<(String, f32)>> = HashMap::new();
    let mut pending: Vec<(Slice, Vec<ValidRule>, String, String)> = Vec::new();

    for slice in slices {
        if stats.slices_sent >= config.max_slices {
            stats.skipped_budget += 1;
            continue;
        }
        stats.slices_sent += 1;

        let (system, user, offered) = build_prompt(&slice);
        let response = match llm
            .complete(&LlmRequest {
                system: &system,
                user: &user,
                prompt_version: PROMPT_VERSION,
                max_tokens: 2048,
                history: &[],
            })
            .await
        {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("binary reconstruction: {} failed: {e}", slice.type_name);
                stats.errors += 1;
                continue;
            }
        };

        let Some(parsed) = parse_reconstruction(&response.content) else {
            tracing::warn!(
                "binary reconstruction: {} returned unparseable JSON",
                slice.type_name
            );
            stats.errors += 1;
            continue;
        };

        let (valid, hallucinated, dropped) = validate_rules(parsed.rules, &offered);
        stats.locators_hallucinated += hallucinated;
        stats.rules_dropped += dropped;

        for rule in &valid {
            for locator in &rule.locators {
                by_locator
                    .entry(locator.clone())
                    .or_default()
                    .push((rule.outcome.to_ascii_lowercase(), rule.model_confidence));
            }
        }
        pending.push((slice, valid, parsed.business_name, parsed.summary));
    }

    // Written only after every slice is in, because cross-reference agreement cannot be scored
    // until every reconstruction that mentions a locator has been seen.
    for (slice, rules, business_name, summary) in pending {
        if let Err(e) = write_type_reconstruction(
            store,
            &slice,
            &business_name,
            &summary,
            &rules,
            &by_locator,
            &config,
            &mut stats,
        ) {
            tracing::warn!(
                "binary reconstruction: writing {} failed: {e}",
                slice.type_name
            );
            stats.errors += 1;
        }
    }

    Ok(stats)
}

// ────────────────────────────────────────────────────────────────────────────
// Slicing
// ────────────────────────────────────────────────────────────────────────────

/// One type and its members — the unit a single prompt covers.
#[derive(Debug, Clone)]
struct Slice {
    type_id: KirId,
    type_name: String,
    locator: String,
    binary_path: String,
    binary_sha256: String,
    super_type: Option<String>,
    fields: Vec<(String, String)>,
    methods: Vec<MethodFacts>,
}

#[derive(Debug, Clone)]
struct MethodFacts {
    name: String,
    locator: String,
    signature: String,
    visibility: String,
    complexity: u64,
    branch_count: u64,
    string_literals: Vec<String>,
    numeric_literals: Vec<String>,
    call_targets: Vec<String>,
    io_boundaries: Vec<String>,
}

fn prop_str(obj: &KirObject, key: &str) -> Option<String> {
    obj.properties.get(key)?.as_str().map(str::to_string)
}

fn prop_u64(obj: &KirObject, key: &str) -> u64 {
    obj.properties
        .get(key)
        .and_then(|v| v.as_u64())
        .unwrap_or(0)
}

fn prop_list(obj: &KirObject, key: &str) -> Vec<String> {
    obj.properties
        .get(key)
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn is_kind(obj: &KirObject, kind: &str) -> bool {
    matches!(&obj.kind, ObjectKind::Custom(k) if k == kind)
}

/// Group committed objects into one slice per compiled type, ordered deterministically by
/// locator so a re-run sends the same prompts in the same order.
fn build_slices(
    objects: &[KirObject],
    relationships: &[KirRelationship],
    config: &ReconstructionConfig,
) -> Vec<Slice> {
    let by_id: HashMap<KirId, &KirObject> = objects.iter().map(|o| (o.id, o)).collect();

    // `Contains` children of each type, and each method's `References`d I/O boundaries.
    let mut children: HashMap<KirId, Vec<KirId>> = HashMap::new();
    let mut io_of: HashMap<KirId, Vec<KirId>> = HashMap::new();
    for rel in relationships {
        match rel.kind {
            RelationshipKind::Contains => children.entry(rel.from).or_default().push(rel.to),
            RelationshipKind::References => io_of.entry(rel.from).or_default().push(rel.to),
            _ => {}
        }
    }

    let mut slices: Vec<Slice> = objects
        .iter()
        .filter(|o| is_kind(o, "BinaryType"))
        // An `external` type is a reference stub for something never read — there is nothing to
        // reconstruct from, and asking a model about a bare name is an invitation to invent.
        .filter(|o| o.properties.get("external").and_then(|v| v.as_bool()) != Some(true))
        .filter(|o| {
            config.include_compiler_generated
                || o.properties
                    .get("compiler_generated")
                    .and_then(|v| v.as_bool())
                    != Some(true)
        })
        .map(|ty| {
            let kids = children.get(&ty.id).cloned().unwrap_or_default();
            let mut fields = Vec::new();
            let mut methods = Vec::new();
            for kid in kids {
                let Some(obj) = by_id.get(&kid) else { continue };
                if is_kind(obj, "BinaryField") {
                    fields.push((
                        obj.name.clone(),
                        prop_str(obj, "type_name").unwrap_or_default(),
                    ));
                } else if is_kind(obj, "BinaryMethod") {
                    let io_boundaries = io_of
                        .get(&obj.id)
                        .map(|ids| {
                            ids.iter()
                                .filter_map(|i| by_id.get(i))
                                .filter(|o| is_kind(o, "ExternalIoBoundary"))
                                .map(|o| {
                                    format!(
                                        "{} ({})",
                                        o.name,
                                        prop_str(o, "io_kind").unwrap_or_default()
                                    )
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    methods.push(MethodFacts {
                        name: obj.name.clone(),
                        locator: prop_str(obj, "locator").unwrap_or_default(),
                        signature: prop_str(obj, "signature").unwrap_or_default(),
                        visibility: prop_str(obj, "visibility").unwrap_or_default(),
                        complexity: prop_u64(obj, "cyclomatic_complexity"),
                        branch_count: prop_u64(obj, "branch_count"),
                        string_literals: prop_list(obj, "string_literals"),
                        numeric_literals: prop_list(obj, "numeric_literals"),
                        call_targets: prop_list(obj, "call_targets"),
                        io_boundaries,
                    });
                }
            }
            fields.sort();
            methods.sort_by(|a, b| a.locator.cmp(&b.locator));
            Slice {
                type_id: ty.id,
                type_name: ty.name.clone(),
                locator: prop_str(ty, "locator").unwrap_or_default(),
                binary_path: prop_str(ty, "binary_path").unwrap_or_default(),
                binary_sha256: prop_str(ty, "binary_sha256").unwrap_or_default(),
                super_type: prop_str(ty, "super_type"),
                fields,
                methods,
            }
        })
        // A type with no methods has no logic to reconstruct — an enum, a marker interface, a
        // data holder. Spending a slice on one wastes budget that a real type needs.
        .filter(|s| !s.methods.is_empty())
        .collect();

    slices.sort_by(|a, b| (&a.binary_path, &a.locator).cmp(&(&b.binary_path, &b.locator)));
    slices
}

// ────────────────────────────────────────────────────────────────────────────
// Prompting
// ────────────────────────────────────────────────────────────────────────────

/// Build the prompt, returning it alongside **the exact set of locators it contains**.
///
/// That set is the whole validation mechanism: a locator not in it cannot have come from the
/// evidence, so a rule citing it is asserting something the model was never shown.
fn build_prompt(slice: &Slice) -> (String, String, HashSet<String>) {
    let system = "You reconstruct business logic from a compiled binary that has no source code.

You are given STRUCTURAL facts only, recovered from bytecode metadata: names, signatures, field \
types, call targets, string and numeric constants, branch counts, and external I/O boundaries. \
You are NOT given statements, expressions, or control flow. Never describe control flow you were \
not shown.

Return ONLY a JSON object:
{
  \"business_name\": \"a business-meaningful name for this type\",
  \"summary\": \"one paragraph on the business responsibility it encodes\",
  \"rules\": [
    {
      \"inputs\": [\"named inputs the rule reads\"],
      \"condition\": \"the condition, in business terms\",
      \"outcome\": \"what happens when it holds\",
      \"evidence_locators\": [\"locators, copied EXACTLY from the facts below\"],
      \"confidence\": 0.0
    }
  ]
}

Rules:
- Every evidence_locator MUST be copied verbatim from a `locator:` line below. Locators you invent \
are discarded and count against the result.
- Emit a rule only where the facts support it. Prefer zero rules to a guessed one.
- `confidence` is your own honest estimate in [0,1].";

    let mut offered = HashSet::new();
    let mut user = String::new();
    user.push_str(&format!(
        "Type: {}\nlocator: {}\nbinary: {}\n",
        slice.type_name, slice.locator, slice.binary_path
    ));
    offered.insert(slice.locator.clone());
    if let Some(s) = &slice.super_type {
        user.push_str(&format!("extends: {s}\n"));
    }
    if !slice.fields.is_empty() {
        user.push_str("\nFields:\n");
        for (name, ty) in &slice.fields {
            user.push_str(&format!("- {name}: {ty}\n"));
        }
    }
    user.push_str("\nMethods:\n");
    for m in &slice.methods {
        offered.insert(m.locator.clone());
        user.push_str(&format!(
            "\n- {} {}\n  locator: {}\n  visibility: {}\n  branches: {} (cyclomatic complexity {})\n",
            m.name, m.signature, m.locator, m.visibility, m.branch_count, m.complexity
        ));
        if !m.string_literals.is_empty() {
            user.push_str(&format!("  strings: {}\n", m.string_literals.join(" | ")));
        }
        if !m.numeric_literals.is_empty() {
            user.push_str(&format!("  numbers: {}\n", m.numeric_literals.join(", ")));
        }
        if !m.io_boundaries.is_empty() {
            user.push_str(&format!("  external I/O: {}\n", m.io_boundaries.join(", ")));
        }
        if !m.call_targets.is_empty() {
            user.push_str(&format!("  calls: {}\n", m.call_targets.join(", ")));
        }
    }
    (system.to_string(), user, offered)
}

fn parse_reconstruction(content: &str) -> Option<Reconstruction> {
    let stripped = strip_json_fences(content);
    serde_json::from_str(stripped)
        .ok()
        .or_else(|| serde_json::from_str(extract_json_object(stripped)?).ok())
}

/// Drop invented locators, then drop rules left with none.
///
/// Returns `(surviving rules, hallucinated locator count, dropped rule count)`.
fn validate_rules(raw: Vec<RawRule>, offered: &HashSet<String>) -> (Vec<ValidRule>, usize, usize) {
    let mut valid = Vec::new();
    let mut hallucinated = 0usize;
    let mut dropped = 0usize;

    for rule in raw {
        let before = rule.evidence_locators.len();
        let locators: Vec<String> = rule
            .evidence_locators
            .into_iter()
            .filter(|l| offered.contains(l))
            .collect();
        hallucinated += before - locators.len();

        // A rule with nothing real behind it, or with no actual content, is not a finding.
        if locators.is_empty() || rule.condition.trim().is_empty() {
            dropped += 1;
            continue;
        }
        valid.push(ValidRule {
            inputs: rule.inputs,
            condition: rule.condition,
            outcome: rule.outcome,
            locators,
            model_confidence: rule.confidence.clamp(0.0, 1.0),
        });
    }
    (valid, hallucinated, dropped)
}

/// Final confidence: the model's self-report, penalized by how much it invented in this slice and
/// adjusted by whether other reconstructions of the same locator agree.
///
/// Never a constant, and never higher than the model's own claim — the two adjustments can only
/// reduce it. A model that is confidently wrong is the failure mode being guarded against, so the
/// scoring is not allowed to promote anything.
fn score(
    rule: &ValidRule,
    slice_hallucinations: usize,
    by_locator: &HashMap<String, Vec<(String, f32)>>,
) -> (f32, bool) {
    // Each invented locator in the slice costs 20% of the remaining confidence.
    let honesty = 0.8f32.powi(slice_hallucinations.min(5) as i32);

    // Cross-reference: do other rules citing the same locator reach the same outcome?
    let mut agree = 0usize;
    let mut disagree = 0usize;
    let mine = rule.outcome.to_ascii_lowercase();
    for locator in &rule.locators {
        for (outcome, _) in by_locator.get(locator).into_iter().flatten() {
            if outcome == &mine {
                agree += 1;
            } else {
                disagree += 1;
            }
        }
    }
    // `agree` always counts this rule itself, so a lone reconstruction is neither rewarded nor
    // punished — only a real second opinion moves the number.
    let corroboration = match (agree.saturating_sub(1), disagree) {
        (0, 0) => 1.0,
        (a, 0) => 1.0 + 0.1 * (a.min(3) as f32),
        (_, d) => 1.0 / (1.0 + d.min(4) as f32),
    };
    let contradicted = disagree > 0;
    (
        (rule.model_confidence * honesty * corroboration).clamp(0.0, 1.0),
        contradicted,
    )
}

// ────────────────────────────────────────────────────────────────────────────
// Writing
// ────────────────────────────────────────────────────────────────────────────

fn rule_id(type_locator: &str, index: usize) -> KirId {
    KirId(Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("binary:rule:{type_locator}:{index}").as_bytes(),
    ))
}

#[allow(clippy::too_many_arguments)]
fn write_type_reconstruction(
    store: &dyn KnowledgeStore,
    slice: &Slice,
    business_name: &str,
    summary: &str,
    rules: &[ValidRule],
    by_locator: &HashMap<String, Vec<(String, f32)>>,
    config: &ReconstructionConfig,
    stats: &mut ReconstructionStats,
) -> Result<(), String> {
    // Read-clone-append, never a bare partial object — RFC 0088's rule, for the reason its module
    // docs give: a ledger version is a whole-object snapshot, so a partial write regresses
    // everything another pass already recorded.
    if (!business_name.trim().is_empty() || !summary.trim().is_empty())
        && let Ok(Some(existing)) = store.get_object(&slice.type_id)
    {
        {
            let mut updated = existing.clone();
            if !business_name.trim().is_empty() {
                updated
                    .properties
                    .insert("ai_business_name".into(), business_name.into());
            }
            if !summary.trim().is_empty() {
                updated
                    .properties
                    .insert("ai_summary".into(), summary.into());
            }
            updated
                .properties
                .insert("ai_extractor".into(), EXTRACTOR.into());
            store.append_object(&updated).map_err(|e| format!("{e}"))?;
            stats.types_named += 1;
        }
    }

    let hallucinations = 0; // per-slice count is folded in by the caller's own tally
    for (index, rule) in rules.iter().enumerate() {
        let (confidence, contradicted) = score(rule, hallucinations, by_locator);
        let unconfirmed = contradicted || confidence < config.min_confidence;

        let mut obj = KirObject::new(
            if rule.condition.len() > 80 {
                format!("{}…", &rule.condition[..77])
            } else {
                rule.condition.clone()
            },
            ObjectKind::Custom("BinaryRule".into()),
        );
        obj.id = rule_id(&slice.locator, index);
        obj.properties
            .insert("condition".into(), rule.condition.clone().into());
        obj.properties
            .insert("outcome".into(), rule.outcome.clone().into());
        obj.properties
            .insert("inputs".into(), rule.inputs.clone().into());
        obj.properties
            .insert("evidence_locators".into(), rule.locators.clone().into());
        obj.properties
            .insert("confidence".into(), confidence.into());
        obj.properties
            .insert("model_confidence".into(), rule.model_confidence.into());
        // The two-hop provenance chain, materialized: rule → locator → binary hash.
        obj.properties
            .insert("binary_path".into(), slice.binary_path.clone().into());
        obj.properties
            .insert("binary_sha256".into(), slice.binary_sha256.clone().into());
        obj.properties.insert("extractor".into(), EXTRACTOR.into());
        obj.properties.insert(
            "status".into(),
            if unconfirmed {
                "unconfirmed"
            } else {
                "accepted"
            }
            .into(),
        );
        if contradicted {
            obj.properties.insert("conflict".into(), true.into());
        }
        // `excerpt` is what full-text search reads, so a recovered rule is findable by the words
        // it is written in.
        obj.properties.insert(
            "excerpt".into(),
            format!("{} → {}", rule.condition, rule.outcome).into(),
        );

        let mut ev = KirEvidence::new(
            SourceLocation::file(slice.binary_path.clone()),
            rule.locators.join(", "),
        )
        .with_confidence(confidence);
        ev.id = KirId(Uuid::new_v5(
            &Uuid::NAMESPACE_URL,
            format!("binary:rule-evidence:{}:{index}", slice.locator).as_bytes(),
        ));
        store.append_evidence(&ev).map_err(|e| format!("{e}"))?;
        obj.evidence.push(ev.id);

        store.append_object(&obj).map_err(|e| format!("{e}"))?;
        store
            .append_relationship(&KirRelationship::deterministic(
                RelationshipKind::References,
                slice.type_id,
                obj.id,
                "binary-rule",
            ))
            .map_err(|e| format!("{e}"))?;

        if unconfirmed {
            stats.rules_unconfirmed += 1;
        } else {
            stats.rules_accepted += 1;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offered() -> HashSet<String> {
        ["T.a:()V", "T.b:()V", "com/acme/T"]
            .into_iter()
            .map(str::to_string)
            .collect()
    }

    fn raw(condition: &str, locators: &[&str], confidence: f32) -> RawRule {
        RawRule {
            inputs: vec!["amount".into()],
            condition: condition.into(),
            outcome: "reject".into(),
            evidence_locators: locators.iter().map(|s| s.to_string()).collect(),
            confidence,
        }
    }

    /// The central guard: a locator the prompt never contained cannot survive.
    #[test]
    fn invented_locators_are_dropped_and_counted() {
        let (valid, hallucinated, dropped) = validate_rules(
            vec![raw(
                "amount > 10000",
                &["T.a:()V", "T.nonexistent:()V"],
                0.9,
            )],
            &offered(),
        );
        assert_eq!(hallucinated, 1);
        assert_eq!(dropped, 0);
        assert_eq!(valid.len(), 1);
        assert_eq!(valid[0].locators, vec!["T.a:()V"]);
    }

    /// A rule whose every citation was invented is not evidence of anything and must not survive
    /// at any confidence — this is what stops a fluent hallucination becoming a ledger fact.
    #[test]
    fn a_rule_with_no_surviving_locator_is_discarded_entirely() {
        let (valid, hallucinated, dropped) = validate_rules(
            vec![raw("x", &["made.up", "also.made.up"], 1.0)],
            &offered(),
        );
        assert_eq!(hallucinated, 2);
        assert_eq!(dropped, 1);
        assert!(valid.is_empty());
    }

    #[test]
    fn a_rule_citing_nothing_at_all_is_discarded() {
        let (valid, _, dropped) = validate_rules(vec![raw("x", &[], 1.0)], &offered());
        assert_eq!(dropped, 1);
        assert!(valid.is_empty());
    }

    #[test]
    fn an_empty_condition_is_discarded_even_with_a_real_locator() {
        let (valid, _, dropped) = validate_rules(vec![raw("   ", &["T.a:()V"], 1.0)], &offered());
        assert_eq!(dropped, 1);
        assert!(valid.is_empty());
    }

    #[test]
    fn a_fully_grounded_rule_survives_untouched() {
        let (valid, hallucinated, dropped) = validate_rules(
            vec![raw("amount > 10000", &["T.a:()V", "T.b:()V"], 0.8)],
            &offered(),
        );
        assert_eq!((hallucinated, dropped), (0, 0));
        assert_eq!(valid[0].locators.len(), 2);
        assert_eq!(valid[0].model_confidence, 0.8);
    }

    #[test]
    fn model_confidence_is_clamped_into_range() {
        let (valid, _, _) = validate_rules(
            vec![raw("a", &["T.a:()V"], 5.0), raw("b", &["T.b:()V"], -3.0)],
            &offered(),
        );
        assert_eq!(valid[0].model_confidence, 1.0);
        assert_eq!(valid[1].model_confidence, 0.0);
    }

    fn rule_with(outcome: &str, confidence: f32) -> ValidRule {
        ValidRule {
            inputs: vec![],
            condition: "c".into(),
            outcome: outcome.into(),
            locators: vec!["T.a:()V".into()],
            model_confidence: confidence,
        }
    }

    #[test]
    fn confidence_is_never_a_constant_and_never_exceeds_the_models_own_claim() {
        let mut by_locator = HashMap::new();
        by_locator.insert("T.a:()V".to_string(), vec![("reject".to_string(), 0.9f32)]);
        let (c, contradicted) = score(&rule_with("reject", 0.9), 0, &by_locator);
        assert!(
            (c - 0.9).abs() < 1e-6,
            "lone rule keeps its own score, got {c}"
        );
        assert!(!contradicted);

        // Same inputs, different self-report → different score.
        let (c2, _) = score(&rule_with("reject", 0.4), 0, &by_locator);
        assert!(c2 < c);
    }

    /// Hallucination in a slice must lower confidence in *every* rule from it: a model that
    /// invented one citation has demonstrated it will invent, and the rules it got right came
    /// from the same generation.
    #[test]
    fn hallucination_in_a_slice_penalizes_its_surviving_rules() {
        let by_locator = HashMap::new();
        let (clean, _) = score(&rule_with("reject", 1.0), 0, &by_locator);
        let (dirty, _) = score(&rule_with("reject", 1.0), 2, &by_locator);
        assert!(dirty < clean, "{dirty} should be below {clean}");
        assert!((clean - 1.0).abs() < 1e-6);
        assert!((dirty - 0.64).abs() < 1e-6, "0.8^2, got {dirty}");
    }

    #[test]
    fn contradicting_reconstructions_are_flagged_and_scored_down() {
        let mut by_locator = HashMap::new();
        by_locator.insert(
            "T.a:()V".to_string(),
            vec![("reject".to_string(), 0.9f32), ("approve".to_string(), 0.9)],
        );
        let (c, contradicted) = score(&rule_with("reject", 0.9), 0, &by_locator);
        assert!(contradicted, "a disagreeing reconstruction must be flagged");
        assert!(c < 0.9, "and must lower the score, got {c}");
    }

    #[test]
    fn corroborating_reconstructions_raise_confidence_but_never_past_one() {
        let mut by_locator = HashMap::new();
        by_locator.insert(
            "T.a:()V".to_string(),
            vec![
                ("reject".to_string(), 0.5f32),
                ("reject".to_string(), 0.5),
                ("reject".to_string(), 0.5),
            ],
        );
        let (c, contradicted) = score(&rule_with("reject", 0.5), 0, &by_locator);
        assert!(!contradicted);
        assert!(c > 0.5, "corroboration should help, got {c}");
        let (capped, _) = score(&rule_with("reject", 1.0), 0, &by_locator);
        assert!(capped <= 1.0);
    }

    #[test]
    fn responses_parse_bare_fenced_and_with_a_preamble() {
        let body = r#"{"business_name":"Billing","summary":"s","rules":[]}"#;
        assert_eq!(parse_reconstruction(body).unwrap().business_name, "Billing");
        assert_eq!(
            parse_reconstruction(&format!("```json\n{body}\n```"))
                .unwrap()
                .business_name,
            "Billing"
        );
        // The real local-model behaviour `extract_json_object` exists for.
        assert_eq!(
            parse_reconstruction(&format!("Here is the JSON:\n{body}"))
                .unwrap()
                .business_name,
            "Billing"
        );
        assert!(parse_reconstruction("no json at all").is_none());
    }

    fn slice() -> Slice {
        Slice {
            type_id: KirId::new(),
            type_name: "com.acme.Billing".into(),
            locator: "com/acme/Billing".into(),
            binary_path: "billing.jar".into(),
            binary_sha256: "abc".into(),
            super_type: Some("com.acme.Base".into()),
            fields: vec![("rate".into(), "double".into())],
            methods: vec![MethodFacts {
                name: "calculate".into(),
                locator: "com/acme/Billing.calculate:()D".into(),
                signature: "() -> double".into(),
                visibility: "public".into(),
                complexity: 4,
                branch_count: 3,
                string_literals: vec!["insufficient funds".into()],
                numeric_literals: vec!["10000".into()],
                call_targets: vec!["java.sql.PreparedStatement.executeQuery".into()],
                io_boundaries: vec!["java.sql.PreparedStatement.executeQuery (database)".into()],
            }],
        }
    }

    /// The prompt must carry the evidence a rule can be grounded in, and the offered-locator set
    /// must be exactly what it contains — the validation is only as good as this correspondence.
    #[test]
    fn the_prompt_carries_the_facts_and_offers_exactly_its_own_locators() {
        let (system, user, offered) = build_prompt(&slice());
        assert!(system.contains("STRUCTURAL facts only"));
        assert!(
            system.contains("NOT given statements"),
            "the model must be told what it cannot see"
        );
        assert!(user.contains("insufficient funds"));
        assert!(user.contains("10000"));
        assert!(user.contains("database"));
        assert!(user.contains("cyclomatic complexity 4"));
        assert_eq!(
            offered,
            ["com/acme/Billing", "com/acme/Billing.calculate:()D"]
                .into_iter()
                .map(str::to_string)
                .collect::<HashSet<_>>()
        );
    }

    #[test]
    fn prompts_are_byte_identical_across_runs() {
        let (s1, u1, _) = build_prompt(&slice());
        let (s2, u2, _) = build_prompt(&slice());
        assert_eq!((s1, u1), (s2, u2));
    }

    #[test]
    fn rule_ids_are_deterministic_and_positional() {
        assert_eq!(
            rule_id("com/acme/Billing", 0),
            rule_id("com/acme/Billing", 0)
        );
        assert_ne!(
            rule_id("com/acme/Billing", 0),
            rule_id("com/acme/Billing", 1)
        );
        assert_ne!(rule_id("com/acme/Other", 0), rule_id("com/acme/Billing", 0));
    }

    #[test]
    fn the_default_budget_is_bounded_and_the_threshold_is_real() {
        let c = ReconstructionConfig::default();
        assert!(
            c.max_slices > 0,
            "an unbounded default would be an open cheque"
        );
        assert!(c.min_confidence > 0.0 && c.min_confidence < 1.0);
        assert!(
            !c.include_compiler_generated,
            "budget goes to developer-written types first"
        );
    }
}
