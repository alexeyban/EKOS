//! The provider-independent decision contract (RFC 0050, source document
//! §10) plus two deterministic reference implementations. No LLM-backed
//! implementation ships here — see the RFC's Non-goals.

use crate::action::{Action, ActionKind};
use ekos_kir::KirId;
use ekos_runtime::World;
use serde::{Deserialize, Serialize};

/// What one agent's [`DecisionEngine`] sees when asked to act: its own
/// observation (RFC 0049's `agent_observation`) and the actions it's
/// allowed to choose from this round.
pub struct DecisionContext<'a> {
    pub agent_id: KirId,
    pub observation: &'a World,
    pub available_actions: &'a [ActionKind],
}

/// An agent's chosen action plus concise, auditable justification —
/// deliberately never a hidden chain-of-thought (source document §10's own
/// instruction, matching this project's existing treatment of LLM output as
/// auditable text elsewhere, e.g. RFC 0019/0021).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Decision {
    pub action: Action,
    pub reasoning_summary: String,
    pub confidence: f32,
}

/// A provider-independent decision policy — `RuleBasedAgent`, a future
/// `LocalLLMAgent`/`CloudLLMAgent`, etc. can all implement this the same way
/// `LlmProvider` (RFC 0021/0046) lets multiple LLM backends share one call
/// site.
pub trait DecisionEngine {
    fn decide(&self, ctx: &DecisionContext) -> Decision;
}

/// The trivial baseline: always does nothing. Useful as a default and for
/// isolated action-system tests that don't want a acting agent to affect the
/// round.
pub struct AlwaysDoNothing;

impl DecisionEngine for AlwaysDoNothing {
    fn decide(&self, _ctx: &DecisionContext) -> Decision {
        Decision {
            action: Action::new(ActionKind::DoNothing),
            reasoning_summary: "no decision policy configured".to_string(),
            confidence: 1.0,
        }
    }
}

/// A deterministic, goal-driven reference engine — proves the decision
/// contract works without an LLM. Reads the acting agent's own `goals`
/// property (RFC 0049's `SimulatedAgent` convention) off its object in
/// `ctx.observation`, and understands one additional sub-convention that
/// only this engine interprets (not a `SimulatedAgent` schema requirement):
/// a goal string of the shape `"support:<name>"` / `"oppose:<name>"` names
/// another object in the observation by its `name` field — an exact,
/// case-sensitive match against `KirObject.name` (e.g. `"Bob"`, not `"bob"`
/// even if that object's scenario-local reference id, RFC 0051, happens to
/// be lowercase `bob`; those are two different fields). The first goal that
/// resolves to an observed target wins; if nothing resolves, or the
/// resolved action isn't in `ctx.available_actions`, falls back to
/// `DoNothing`.
pub struct RuleBasedAgent;

impl DecisionEngine for RuleBasedAgent {
    fn decide(&self, ctx: &DecisionContext) -> Decision {
        let agent_object = ctx
            .observation
            .objects
            .iter()
            .find(|o| o.id == ctx.agent_id);

        let goals = agent_object
            .and_then(|o| o.properties.get("goals"))
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
            .unwrap_or_default();

        for goal in goals {
            let (kind, name) = if let Some(name) = goal.strip_prefix("support:") {
                (ActionKind::Support, name)
            } else if let Some(name) = goal.strip_prefix("oppose:") {
                (ActionKind::Oppose, name)
            } else {
                continue;
            };

            if !ctx.available_actions.contains(&kind) {
                continue;
            }

            if let Some(target) = ctx
                .observation
                .objects
                .iter()
                .find(|o| o.id != ctx.agent_id && o.name == name)
            {
                return Decision {
                    action: Action::new(kind).with_target(target.id),
                    reasoning_summary: format!("goal '{goal}' resolved to observed target"),
                    confidence: 0.8,
                };
            }
        }

        Decision {
            action: Action::new(ActionKind::DoNothing),
            reasoning_summary: "no goal resolved to a currently observed target".to_string(),
            confidence: 0.5,
        }
    }
}
