//! The round-based simulation loop (RFC 0050, source document §12). Reads
//! go through [`Runtime`] (`agent_observation`/`build_world`, unchanged and
//! still read-only); writes go directly through `&dyn KnowledgeStore` — the
//! same access level `commit.rs` already has — because `Runtime` is
//! documented and enforced read-only end to end (RFC 0005) and this engine
//! genuinely needs to write every round.

use crate::action::{Action, ActionKind, ValidationError, validate_action};
use crate::decision::{Decision, DecisionContext, DecisionEngine};
use chrono::{DateTime, Utc};
use ekos_kir::{
    EventKind, KirEvent, KirId, KirObject, KirRelationship, ObjectKind, RelationshipKind,
};
use ekos_ledger::{KnowledgeStore, LedgerError};
use ekos_runtime::{Runtime, RuntimeError};
use rand::SeedableRng;
use rand::rngs::StdRng;
use rand::seq::SliceRandom;
use std::collections::HashMap;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SimulationError {
    #[error("ledger error: {0}")]
    Ledger(#[from] LedgerError),
    #[error("runtime error: {0}")]
    Runtime(#[from] RuntimeError),
    #[error("no decision engine configured for agent {0}")]
    NoDecisionEngine(KirId),
}

/// A decision that passed validation but still didn't happen this round —
/// distinct from [`ValidationError`] (RFC 0050: a decision that was never
/// reasonable, given the pre-round snapshot) because a conflict-failed
/// decision *was* reasonable when it was made; it lost a same-round race
/// that didn't exist yet at validation time (RFC 0052).
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ConflictError {
    #[error("agent {agent} lacks enough '{resource}' (needed {needed}, has {available})")]
    InsufficientResource {
        agent: KirId,
        resource: String,
        needed: f64,
        available: f64,
    },
    #[error("channel {channel}'s capacity was exhausted earlier this round (had {available})")]
    ChannelCapacityExhausted { channel: KirId, available: f64 },
}

/// Every non-`DoNothing` action costs this much of an actor's own
/// `resources.energy`, if that resource is declared at all (opt-in — see
/// [`try_consume_resource`]). Deliberately uniform across all 11 kinds, not
/// per-kind-differentiated — see the RFC's Non-goals.
const ACTION_ENERGY_COST: f64 = 0.1;

/// Which agents participate, in what order priority ties break (the
/// `available_actions` on offer, and the seed that makes tie-breaking and
/// resource contention reproducible (RFC 0052) — `None` behaves exactly as
/// RFC 0050 always did.
pub struct SimulationConfig {
    pub agents: Vec<KirId>,
    pub available_actions: Vec<ActionKind>,
    pub seed: Option<u64>,
}

/// Everything that happened in one round, kept for audit/testing — not
/// persisted as its own entity (the events and relationships inside it
/// already are, via the ledger writes that produced them).
#[derive(Debug, Clone)]
pub struct RoundResult {
    pub round: u32,
    pub occurred_at: DateTime<Utc>,
    /// The action actually taken per agent, after validation (a rejected
    /// decision has already been downgraded to `DoNothing` here), in the
    /// priority order they were executed in (RFC 0052) — not necessarily
    /// `SimulationConfig.agents`' own order.
    pub decisions: Vec<(KirId, Decision)>,
    pub events: Vec<KirEvent>,
    /// Recorded, not discarded: which agent's original decision was
    /// rejected, and why.
    pub validation_failures: Vec<(KirId, ValidationError)>,
    /// Recorded, not discarded, and distinct from `validation_failures`
    /// (RFC 0052): a decision that passed the pre-round validation check but
    /// still failed at execution time because of same-round contention over
    /// a shared, scarce resource.
    pub conflict_failures: Vec<(KirId, ConflictError)>,
}

pub struct Simulation<'a> {
    store: &'a dyn KnowledgeStore,
    config: SimulationConfig,
    engines: HashMap<KirId, Box<dyn DecisionEngine>>,
}

impl<'a> Simulation<'a> {
    pub fn new(
        store: &'a dyn KnowledgeStore,
        config: SimulationConfig,
        engines: HashMap<KirId, Box<dyn DecisionEngine>>,
    ) -> Self {
        Self {
            store,
            config,
            engines,
        }
    }

    /// The full 8-step lifecycle for one round (source document §12). Every
    /// agent's observation is taken from the same pre-round cut before any
    /// decision is made, and every decision is made before any round-N
    /// action is executed — satisfying the source document's own warning
    /// not to let Agent A's action leak into Agent B's same-round decision.
    pub fn run_round(&self, round: u32) -> Result<RoundResult, SimulationError> {
        let runtime = Runtime::over(self.store);
        let occurred_at = Utc::now();

        // 1. Observe — one cut, shared by every agent this round.
        let mut observations = HashMap::new();
        for agent_id in &self.config.agents {
            let observation = runtime.agent_observation(agent_id, Some(occurred_at))?;
            observations.insert(*agent_id, observation);
        }

        // 2. Decide — every agent decides from its own step-1 observation
        // only; none has seen any other agent's round-N action yet.
        let mut raw_decisions = Vec::new();
        for agent_id in &self.config.agents {
            let engine = self
                .engines
                .get(agent_id)
                .ok_or(SimulationError::NoDecisionEngine(*agent_id))?;
            let ctx = DecisionContext {
                agent_id: *agent_id,
                observation: &observations[agent_id],
                available_actions: &self.config.available_actions,
            };
            raw_decisions.push((*agent_id, engine.decide(&ctx)));
        }

        // 3. Validate — a rejected action is recorded, then downgraded to
        // DoNothing rather than silently dropped.
        let mut validation_failures = Vec::new();
        let mut decisions = Vec::new();
        for (agent_id, decision) in raw_decisions {
            match validate_action(&agent_id, &decision.action, &observations[&agent_id]) {
                Ok(()) => decisions.push((agent_id, decision)),
                Err(err) => {
                    validation_failures.push((agent_id, err));
                    decisions.push((
                        agent_id,
                        Decision {
                            action: Action::new(ActionKind::DoNothing),
                            reasoning_summary:
                                "original decision failed validation; downgraded to DoNothing"
                                    .to_string(),
                            confidence: 0.0,
                        },
                    ));
                }
            }
        }

        // 4. Resolve conflicts — real as of RFC 0052: sort by
        // Decision.confidence descending (priority), breaking ties with a
        // seeded, reproducible shuffle so execution order (and therefore who
        // wins a same-round resource race, below) is deterministic for a
        // given (seed, round) without being a fixed agent-list order.
        let mut rng = seeded_rng(self.config.seed.unwrap_or(0), round);
        let ordered = order_by_priority(decisions, &mut rng);

        // 5. Execute + 6. Persist (each ledger append below is immediate,
        // no batching) + 8. Update agent memories. Resource checks inside
        // execute_action happen here, in priority order — the only point at
        // which same-round contention over a shared resource can actually be
        // observed (validation, above, saw only the shared pre-round cut).
        // The log root is resolved once per round (RFC 0054), not once per
        // action — a small, deliberate optimization over the naturally
        // small, throwaway scenario ledgers this engine already assumes.
        let log_root = find_or_create_log(self.store)?;
        let mut events = Vec::new();
        let mut conflict_failures = Vec::new();
        for (agent_id, decision) in &ordered {
            match execute_action(
                self.store,
                agent_id,
                decision,
                round,
                occurred_at,
                &log_root,
            )? {
                ExecutionOutcome::Executed(event) => {
                    for observer in observers_for(&decision.action, agent_id, &self.config.agents) {
                        append_knows(self.store, &observer, &event.id)?;
                    }
                    events.push(event);
                }
                ExecutionOutcome::Conflict(err) => {
                    conflict_failures.push((*agent_id, err));
                }
            }
        }

        // 7. Update world — nothing to do: `World` (RFC 0048) is a
        // projection, so the next round's `agent_observation` already sees
        // everything step 5 wrote.

        Ok(RoundResult {
            round,
            occurred_at,
            decisions: ordered,
            events,
            validation_failures,
            conflict_failures,
        })
    }

    pub fn run(&self, rounds: u32) -> Result<Vec<RoundResult>, SimulationError> {
        (0..rounds).map(|round| self.run_round(round)).collect()
    }
}

/// Who should gain a `Knows` edge to a just-executed action's event: every
/// configured agent for a public action, actor + target for a private one,
/// actor only for a self-only one.
fn observers_for(action: &Action, actor_id: &KirId, all_agents: &[KirId]) -> Vec<KirId> {
    if action.kind.is_public() {
        all_agents.to_vec()
    } else if action.kind.is_private() {
        let mut observers = vec![*actor_id];
        if let Some(target) = action.target {
            observers.push(target);
        }
        observers
    } else {
        vec![*actor_id]
    }
}

/// A seeded RNG for one round's priority tie-break, derived from the
/// simulation's own seed (`0` if none was given, preserving RFC 0050's
/// original fully-deterministic-by-default behavior) combined with the
/// round number — the same `(seed, round)` pair always shuffles a tied
/// group identically; different rounds get different shuffles even under a
/// fixed seed.
fn seeded_rng(seed: u64, round: u32) -> StdRng {
    StdRng::seed_from_u64(seed.wrapping_add(u64::from(round)))
}

/// Sorts decisions by `Decision.confidence` descending (priority — RFC
/// 0052, source document §14's "action priority"/"ordering" bullets),
/// breaking ties within each contiguous equal-confidence run with a seeded
/// shuffle rather than leaving them in `SimulationConfig.agents`' incidental
/// order. A stable sort first, so an untied entry never moves relative to
/// its confidence peers, and a tied group's *starting* order (before the
/// shuffle) is the original agent order — deterministic input to a
/// deterministic shuffle, not two sources of nondeterminism stacked.
fn order_by_priority(
    mut decisions: Vec<(KirId, Decision)>,
    rng: &mut StdRng,
) -> Vec<(KirId, Decision)> {
    decisions.sort_by(|a, b| {
        b.1.confidence
            .partial_cmp(&a.1.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut start = 0;
    while start < decisions.len() {
        let confidence = decisions[start].1.confidence;
        let mut end = start + 1;
        while end < decisions.len() && decisions[end].1.confidence == confidence {
            end += 1;
        }
        decisions[start..end].shuffle(rng);
        start = end;
    }
    decisions
}

/// What happened when one decision reached execution: either it went
/// through (an audit event exists, `Knows` fanout should happen), or it lost
/// a same-round race over a shared, scarce resource (RFC 0052) — distinct
/// from a validation failure because this decision was entirely reasonable
/// when it was made against the shared pre-round snapshot.
enum ExecutionOutcome {
    Executed(KirEvent),
    Conflict(ConflictError),
}

/// Whether consuming `amount` of `entity_id`'s `resources[key]` succeeded.
/// Opt-in by construction: an entity with no `resources` property, or no
/// `key` inside it, is unconstrained (`NoSuchResource`) — never blocks an
/// agent or channel that never declared the resource, so every RFC
/// 0050/0051 fixture keeps working unmodified.
pub(crate) enum ConsumeResult {
    Consumed,
    Insufficient { available: f64 },
    NoSuchResource,
}

pub(crate) fn try_consume_resource(
    store: &dyn KnowledgeStore,
    entity_id: &KirId,
    key: &str,
    amount: f64,
) -> Result<ConsumeResult, SimulationError> {
    let Some(mut obj) = store.get_object(entity_id)? else {
        return Ok(ConsumeResult::NoSuchResource);
    };
    let Some(current) = obj
        .properties
        .get("resources")
        .and_then(|r| r.get(key))
        .and_then(|v| v.as_f64())
    else {
        return Ok(ConsumeResult::NoSuchResource);
    };
    if current < amount {
        return Ok(ConsumeResult::Insufficient { available: current });
    }
    if let Some(resources) = obj
        .properties
        .get_mut("resources")
        .and_then(|r| r.as_object_mut())
    {
        resources.insert(key.to_string(), serde_json::json!(current - amount));
    }
    store.append_object(&obj)?;
    Ok(ConsumeResult::Consumed)
}

/// Appends the audit-trail event for one executed action, and — for
/// `FormAlliance` only, this RFC's one worked effect beyond the event itself
/// — bumps the actor's `Trusts` relationship toward the target. Two
/// resource checks happen first, both at execution time (RFC 0052) — the
/// only point at which same-round contention over a shared resource is
/// actually observable, since every agent's earlier validation pass saw the
/// same pre-round snapshot: the actor's own `resources.energy` (every
/// non-`DoNothing` action), and — only for `PostMessage` targeting a
/// `Custom("Channel")` object — that channel's shared `resources.capacity`,
/// this RFC's one worked example of a genuine same-round collision.
fn execute_action(
    store: &dyn KnowledgeStore,
    actor_id: &KirId,
    decision: &Decision,
    round: u32,
    occurred_at: DateTime<Utc>,
    log_root: &KirId,
) -> Result<ExecutionOutcome, SimulationError> {
    let action = &decision.action;

    if action.kind != ActionKind::DoNothing
        && let ConsumeResult::Insufficient { available } =
            try_consume_resource(store, actor_id, "energy", ACTION_ENERGY_COST)?
    {
        return Ok(ExecutionOutcome::Conflict(
            ConflictError::InsufficientResource {
                agent: *actor_id,
                resource: "energy".to_string(),
                needed: ACTION_ENERGY_COST,
                available,
            },
        ));
    }

    if action.kind == ActionKind::PostMessage
        && let Some(target) = action.target
        && let Some(target_obj) = store.get_object(&target)?
        && target_obj.kind == ObjectKind::Custom("Channel".to_string())
        && let ConsumeResult::Insufficient { available } =
            try_consume_resource(store, &target, "capacity", 1.0)?
    {
        return Ok(ExecutionOutcome::Conflict(
            ConflictError::ChannelCapacityExhausted {
                channel: target,
                available,
            },
        ));
    }

    let payload = serde_json::json!({
        "actor": actor_id.to_string(),
        "kind": format!("{:?}", action.kind),
        "target": action.target.map(|t| t.to_string()),
        "content": action.content,
        "reply_to": action.reply_to.map(|t| t.to_string()),
        "round": round,
        "reasoning_summary": decision.reasoning_summary,
        "confidence": decision.confidence,
    });

    let event = KirEvent {
        id: KirId::new(),
        kind: EventKind::Custom("ActionExecuted".to_string()),
        subject: *actor_id,
        payload,
        evidence: Vec::new(),
        occurred_at,
    };
    store.append_event(&event)?;

    if action.kind == ActionKind::FormAlliance
        && let Some(target) = action.target
    {
        bump_trust(store, actor_id, &target, occurred_at)?;
    }

    // RFC 0053: index this message under its channel so VirtualForum's
    // read_messages (built on a relationship index, since KnowledgeStore
    // has no bulk "every event" query) sees messages posted through a
    // normal round the same way it sees ones posted through the direct API.
    if action.kind == ActionKind::PostMessage
        && let Some(channel) = action.target
        && let Some(channel_obj) = store.get_object(&channel)?
        && channel_obj.kind == ObjectKind::Custom("Channel".to_string())
    {
        crate::forum::index_message_in_channel(store, &event.id, &channel)?;
    }

    // RFC 0054: every executed action — DoNothing included — is indexed
    // under this ledger's one SimulationLog root, the durable record
    // Replay reads from. Unlike PostedIn (channel-scoped), this applies
    // unconditionally, matching the source document's own "every simulation
    // action must produce an immutable event [in the log]" instruction.
    index_event_in_log(store, &event.id, log_root)?;

    Ok(ExecutionOutcome::Executed(event))
}

/// Where a ledger's single `SimulationLog` root object lives, if this
/// ledger has ever recorded a simulation round — `all_objects()` (already
/// on `KnowledgeStore`) is enough to find it; no bulk event query is
/// needed to locate the log itself, only to walk what's indexed under it.
pub(crate) fn find_log_object(
    store: &dyn KnowledgeStore,
) -> Result<Option<KirObject>, LedgerError> {
    Ok(store
        .all_objects()?
        .into_iter()
        .find(|o| o.kind == ObjectKind::Custom("SimulationLog".to_string())))
}

pub(crate) fn find_or_create_log(store: &dyn KnowledgeStore) -> Result<KirId, SimulationError> {
    if let Some(existing) = find_log_object(store)? {
        return Ok(existing.id);
    }
    let log = KirObject::new(
        "simulation-log",
        ObjectKind::Custom("SimulationLog".to_string()),
    );
    store.append_object(&log)?;
    Ok(log.id)
}

fn index_event_in_log(
    store: &dyn KnowledgeStore,
    event_id: &KirId,
    log_root: &KirId,
) -> Result<(), SimulationError> {
    let rel = KirRelationship::new(
        RelationshipKind::Custom("LoggedIn".to_string()),
        *event_id,
        *log_root,
    );
    store.append_relationship(&rel)?;
    Ok(())
}

/// Every agent with a `Knows` edge pointing at `event_id` — a query over
/// data `execute_action`'s own visibility fanout (`observers_for`/
/// `append_knows`) already produces, not a second copy of it (RFC 0054's
/// closure of Phase 12's `observed_by` field).
pub fn observed_by(
    store: &dyn KnowledgeStore,
    event_id: &KirId,
) -> Result<Vec<KirId>, SimulationError> {
    Ok(store
        .relationships_for(event_id)?
        .into_iter()
        .filter(|r| {
            r.to == *event_id && matches!(&r.kind, RelationshipKind::Custom(k) if k == "Knows")
        })
        .map(|r| r.from)
        .collect())
}

/// Appends a new version of the actor's `Trusts` relationship toward
/// `target` (same relationship id if one already exists, matching how RFC
/// 0047's `relationship_history` already understands "a new version of this
/// fact"), incrementing `properties["value"]` by `0.2`, clamped to `1.0` —
/// the source document's own worked `FormAlliance` effect (§11).
fn bump_trust(
    store: &dyn KnowledgeStore,
    actor_id: &KirId,
    target: &KirId,
    occurred_at: DateTime<Utc>,
) -> Result<(), SimulationError> {
    let existing = store.relationships_for(actor_id)?.into_iter().find(|r| {
        r.from == *actor_id
            && r.to == *target
            && matches!(&r.kind, RelationshipKind::Custom(k) if k == "Trusts")
    });

    let mut rel = existing.unwrap_or_else(|| {
        KirRelationship::new(
            RelationshipKind::Custom("Trusts".to_string()),
            *actor_id,
            *target,
        )
    });

    let current = rel
        .properties
        .get("value")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    rel.properties.insert(
        "value".to_string(),
        serde_json::json!((current + 0.2).min(1.0)),
    );
    rel.properties
        .insert("status".to_string(), serde_json::json!("confirmed"));
    rel.created_at = occurred_at;

    store.append_relationship(&rel)?;
    Ok(())
}

/// Appends a `Knows` edge from `agent_id` to `event_id`, unless one already
/// exists — keeps repeated rounds from piling up duplicate edges for an
/// agent that keeps witnessing the same kind of public action.
fn append_knows(
    store: &dyn KnowledgeStore,
    agent_id: &KirId,
    event_id: &KirId,
) -> Result<(), SimulationError> {
    let already_knows = store.relationships_for(agent_id)?.into_iter().any(|r| {
        r.from == *agent_id
            && r.to == *event_id
            && matches!(&r.kind, RelationshipKind::Custom(k) if k == "Knows")
    });
    if already_knows {
        return Ok(());
    }
    let rel = KirRelationship::new(
        RelationshipKind::Custom("Knows".to_string()),
        *agent_id,
        *event_id,
    );
    store.append_relationship(&rel)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::Action;
    use ekos_kir::{KirObject, ObjectKind};
    use ekos_ledger::Ledger;
    use tempfile::tempdir;

    fn decision(confidence: f32) -> Decision {
        Decision {
            action: Action::new(ActionKind::DoNothing),
            reasoning_summary: "test".to_string(),
            confidence,
        }
    }

    #[test]
    fn order_by_priority_sorts_by_confidence_descending_with_no_ties() {
        let a = KirId::new();
        let b = KirId::new();
        let c = KirId::new();
        let decisions = vec![(a, decision(0.5)), (b, decision(1.0)), (c, decision(0.8))];
        let mut rng = seeded_rng(0, 0);
        let ordered = order_by_priority(decisions, &mut rng);
        assert_eq!(
            ordered.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            vec![b, c, a]
        );
    }

    #[test]
    fn order_by_priority_tie_break_is_deterministic_for_same_seed_and_round() {
        let ids: Vec<KirId> = (0..5).map(|_| KirId::new()).collect();
        let decisions: Vec<(KirId, Decision)> = ids.iter().map(|id| (*id, decision(0.5))).collect();

        let mut rng_a = seeded_rng(42, 3);
        let ordered_a = order_by_priority(decisions.clone(), &mut rng_a);

        let mut rng_b = seeded_rng(42, 3);
        let ordered_b = order_by_priority(decisions, &mut rng_b);

        assert_eq!(
            ordered_a.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            ordered_b.iter().map(|(id, _)| *id).collect::<Vec<_>>()
        );
    }

    #[test]
    fn order_by_priority_tie_break_can_differ_across_seeds() {
        let ids: Vec<KirId> = (0..5).map(|_| KirId::new()).collect();
        let decisions: Vec<(KirId, Decision)> = ids.iter().map(|id| (*id, decision(0.5))).collect();

        let mut rng_a = seeded_rng(1, 0);
        let ordered_a = order_by_priority(decisions.clone(), &mut rng_a);

        let mut rng_b = seeded_rng(2, 0);
        let ordered_b = order_by_priority(decisions, &mut rng_b);

        assert_ne!(
            ordered_a.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            ordered_b.iter().map(|(id, _)| *id).collect::<Vec<_>>()
        );
    }

    #[test]
    fn seeded_rng_differs_by_round_under_a_fixed_seed() {
        let ids: Vec<KirId> = (0..5).map(|_| KirId::new()).collect();
        let decisions: Vec<(KirId, Decision)> = ids.iter().map(|id| (*id, decision(0.5))).collect();

        let mut rng_round_0 = seeded_rng(9, 0);
        let ordered_0 = order_by_priority(decisions.clone(), &mut rng_round_0);

        let mut rng_round_1 = seeded_rng(9, 1);
        let ordered_1 = order_by_priority(decisions, &mut rng_round_1);

        assert_ne!(
            ordered_0.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            ordered_1.iter().map(|(id, _)| *id).collect::<Vec<_>>()
        );
    }

    #[test]
    fn try_consume_resource_is_unconstrained_when_key_is_absent() {
        let dir = tempdir().unwrap();
        let ledger = Ledger::open(&dir.path().join("ledger.db")).unwrap();
        let agent = KirObject::new("Alice", ObjectKind::Custom("SimulatedAgent".to_string()));
        ledger.append_object(&agent).unwrap();

        let result = try_consume_resource(&ledger, &agent.id, "energy", 0.1).unwrap();
        assert!(matches!(result, ConsumeResult::NoSuchResource));
    }

    #[test]
    fn try_consume_resource_deducts_and_persists_when_sufficient() {
        let dir = tempdir().unwrap();
        let ledger = Ledger::open(&dir.path().join("ledger.db")).unwrap();
        let mut agent = KirObject::new("Alice", ObjectKind::Custom("SimulatedAgent".to_string()));
        agent.properties.insert(
            "resources".to_string(),
            serde_json::json!({ "energy": 1.0 }),
        );
        ledger.append_object(&agent).unwrap();

        let result = try_consume_resource(&ledger, &agent.id, "energy", 0.1).unwrap();
        assert!(matches!(result, ConsumeResult::Consumed));

        let reloaded = ledger.get_object(&agent.id).unwrap().unwrap();
        let remaining = reloaded.properties["resources"]["energy"].as_f64().unwrap();
        assert!((remaining - 0.9).abs() < 1e-9);
    }

    #[test]
    fn try_consume_resource_reports_insufficient_without_mutating() {
        let dir = tempdir().unwrap();
        let ledger = Ledger::open(&dir.path().join("ledger.db")).unwrap();
        let mut agent = KirObject::new("Alice", ObjectKind::Custom("SimulatedAgent".to_string()));
        agent.properties.insert(
            "resources".to_string(),
            serde_json::json!({ "energy": 0.05 }),
        );
        ledger.append_object(&agent).unwrap();

        let result = try_consume_resource(&ledger, &agent.id, "energy", 0.1).unwrap();
        assert!(
            matches!(result, ConsumeResult::Insufficient { available } if (available - 0.05).abs() < 1e-9)
        );

        let reloaded = ledger.get_object(&agent.id).unwrap().unwrap();
        let remaining = reloaded.properties["resources"]["energy"].as_f64().unwrap();
        assert!(
            (remaining - 0.05).abs() < 1e-9,
            "an insufficient check must not deduct anything"
        );
    }

    #[test]
    fn find_or_create_log_is_idempotent() {
        let dir = tempdir().unwrap();
        let ledger = Ledger::open(&dir.path().join("ledger.db")).unwrap();

        let first = find_or_create_log(&ledger).unwrap();
        let second = find_or_create_log(&ledger).unwrap();
        assert_eq!(
            first, second,
            "a second call must find the same root, not create another"
        );

        let all_logs = ledger
            .all_objects()
            .unwrap()
            .into_iter()
            .filter(|o| o.kind == ObjectKind::Custom("SimulationLog".to_string()))
            .count();
        assert_eq!(all_logs, 1);
    }

    #[test]
    fn observed_by_matches_visibility_fanout_for_each_bucket() {
        use ekos_kir::RelationshipKind;

        let dir = tempdir().unwrap();
        let ledger = Ledger::open(&dir.path().join("ledger.db")).unwrap();
        let alice = KirId::new();
        let bob = KirId::new();
        let charlie = KirId::new();
        let all_agents = [alice, bob, charlie];

        // A public action's event: every agent should be an observer.
        let public_event = KirEvent {
            id: KirId::new(),
            kind: EventKind::Custom("ActionExecuted".to_string()),
            subject: alice,
            payload: serde_json::json!({}),
            evidence: Vec::new(),
            occurred_at: Utc::now(),
        };
        ledger.append_event(&public_event).unwrap();
        for observer in observers_for(
            &Action::new(ActionKind::Support).with_target(bob),
            &alice,
            &all_agents,
        ) {
            ledger
                .append_relationship(&KirRelationship::new(
                    RelationshipKind::Custom("Knows".to_string()),
                    observer,
                    public_event.id,
                ))
                .unwrap();
        }
        let observers: std::collections::HashSet<KirId> = observed_by(&ledger, &public_event.id)
            .unwrap()
            .into_iter()
            .collect();
        let expected: std::collections::HashSet<KirId> =
            [alice, bob, charlie].into_iter().collect();
        assert_eq!(observers, expected);
    }
}
