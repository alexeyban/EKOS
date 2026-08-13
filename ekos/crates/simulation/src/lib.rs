//! Deterministic multi-round simulation engine (RFC 0050): a finite action
//! vocabulary, a provider-independent decision contract, and a round-based
//! loop that reads through [`ekos_runtime::Runtime`] (`agent_observation`/
//! `build_world`, RFC 0048/0049) and writes through
//! [`ekos_ledger::KnowledgeStore`] directly.
//!
//! Builds on three prior RFCs without adding new storage or ledger trait
//! methods: RFC 0047 (claims/temporal validity), RFC 0048 (World Model), RFC
//! 0049 (Agent Model). This crate is the first genuinely new subsystem in
//! that continuation — see the RFC's Motivation for why it earns its own
//! crate rather than another extension to `runtime`.

pub mod action;
pub mod decision;
pub mod forum;
pub mod ingest;
pub mod replay;
pub mod scenario;
pub mod simulation;

pub use action::{Action, ActionKind, ValidationError, validate_action};
pub use decision::{AlwaysDoNothing, Decision, DecisionContext, DecisionEngine, RuleBasedAgent};
pub use forum::{ForumError, VirtualForum};
pub use ingest::IngestError;
pub use replay::{Replay, ReplayError};
pub use scenario::{
    AgentDefinition, AgentRef, EventDef, LoadedScenario, RelationshipDef, ScenarioDefinition,
    ScenarioError, SimulationSettings, load_scenario, load_scenario_from_path,
};
pub use simulation::{
    ConflictError, RoundResult, Simulation, SimulationConfig, SimulationError, observed_by,
};

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use ekos_kir::{KirId, KirObject, ObjectKind};
    use ekos_runtime::World;
    use std::collections::HashMap;

    fn world_with(objects: Vec<KirObject>) -> World {
        World {
            name: "test".to_string(),
            time: Utc::now(),
            round: None,
            objects,
            relationships: Vec::new(),
            events: Vec::new(),
            metadata: HashMap::new(),
        }
    }

    // ── ActionKind ───────────────────────────────────────────────────────

    #[test]
    fn do_nothing_is_the_only_action_not_requiring_a_target() {
        let all = [
            ActionKind::PostMessage,
            ActionKind::SendMessage,
            ActionKind::Support,
            ActionKind::Oppose,
            ActionKind::ShareInformation,
            ActionKind::WithholdInformation,
            ActionKind::FormAlliance,
            ActionKind::BreakAlliance,
            ActionKind::RequestInformation,
            ActionKind::ChangeGoal,
            ActionKind::ChangeBelief,
            ActionKind::DoNothing,
        ];
        for kind in all {
            let expected = kind != ActionKind::DoNothing;
            assert_eq!(kind.requires_target(), expected, "{kind:?}");
        }
    }

    #[test]
    fn every_action_kind_is_exactly_one_of_public_private_self_only() {
        let all = [
            ActionKind::PostMessage,
            ActionKind::SendMessage,
            ActionKind::Support,
            ActionKind::Oppose,
            ActionKind::ShareInformation,
            ActionKind::WithholdInformation,
            ActionKind::FormAlliance,
            ActionKind::BreakAlliance,
            ActionKind::RequestInformation,
            ActionKind::ChangeGoal,
            ActionKind::ChangeBelief,
            ActionKind::DoNothing,
        ];
        for kind in all {
            let bucket_count = [kind.is_public(), kind.is_private(), kind.is_self_only()]
                .into_iter()
                .filter(|b| *b)
                .count();
            assert_eq!(
                bucket_count, 1,
                "{kind:?} must be in exactly one visibility bucket"
            );
        }
    }

    // ── validate_action ─────────────────────────────────────────────────

    #[test]
    fn missing_target_is_rejected_for_a_target_requiring_action() {
        let actor = KirId::new();
        let observation = world_with(vec![]);
        let action = Action::new(ActionKind::Support);
        let err = validate_action(&actor, &action, &observation).unwrap_err();
        assert_eq!(err, ValidationError::MissingTarget(ActionKind::Support));
    }

    #[test]
    fn target_outside_observation_is_rejected() {
        let actor = KirId::new();
        let target = KirId::new();
        let observation = world_with(vec![]);
        let action = Action::new(ActionKind::Support).with_target(target);
        let err = validate_action(&actor, &action, &observation).unwrap_err();
        assert_eq!(err, ValidationError::TargetNotObserved(target));
    }

    #[test]
    fn do_nothing_needs_no_target_and_always_validates() {
        let actor = KirId::new();
        let observation = world_with(vec![]);
        let action = Action::new(ActionKind::DoNothing);
        assert!(validate_action(&actor, &action, &observation).is_ok());
    }

    #[test]
    fn form_alliance_rejected_below_trust_threshold() {
        use ekos_kir::{KirRelationship, RelationshipKind};

        let actor = KirId::new();
        let target_obj = KirObject::new("bob", ObjectKind::Custom("SimulatedAgent".to_string()));
        let target = target_obj.id;
        let mut observation = world_with(vec![target_obj]);
        let mut trust = KirRelationship::new(
            RelationshipKind::Custom("Trusts".to_string()),
            actor,
            target,
        );
        trust
            .properties
            .insert("value".to_string(), serde_json::json!(0.3));
        observation.relationships.push(trust);

        let action = Action::new(ActionKind::FormAlliance).with_target(target);
        let err = validate_action(&actor, &action, &observation).unwrap_err();
        assert!(matches!(
            err,
            ValidationError::PreconditionFailed {
                kind: ActionKind::FormAlliance,
                ..
            }
        ));
    }

    #[test]
    fn form_alliance_accepted_above_trust_threshold() {
        use ekos_kir::{KirRelationship, RelationshipKind};

        let actor = KirId::new();
        let target_obj = KirObject::new("bob", ObjectKind::Custom("SimulatedAgent".to_string()));
        let target = target_obj.id;
        let mut observation = world_with(vec![target_obj]);
        let mut trust = KirRelationship::new(
            RelationshipKind::Custom("Trusts".to_string()),
            actor,
            target,
        );
        trust
            .properties
            .insert("value".to_string(), serde_json::json!(0.5));
        observation.relationships.push(trust);

        let action = Action::new(ActionKind::FormAlliance).with_target(target);
        assert!(validate_action(&actor, &action, &observation).is_ok());
    }

    // ── DecisionEngine reference implementations ────────────────────────

    #[test]
    fn always_do_nothing_never_picks_another_action() {
        let agent_id = KirId::new();
        let observation = world_with(vec![]);
        let available = [ActionKind::Support, ActionKind::DoNothing];
        let ctx = DecisionContext {
            agent_id,
            observation: &observation,
            available_actions: &available,
        };
        let decision = AlwaysDoNothing.decide(&ctx);
        assert_eq!(decision.action.kind, ActionKind::DoNothing);
    }

    #[test]
    fn rule_based_agent_resolves_a_support_goal_to_an_observed_target() {
        let mut agent_obj =
            KirObject::new("alice", ObjectKind::Custom("SimulatedAgent".to_string()));
        agent_obj
            .properties
            .insert("goals".to_string(), serde_json::json!(["support:acme"]));
        let agent_id = agent_obj.id;
        let target_obj = KirObject::new("acme", ObjectKind::Custom("Organization".to_string()));
        let target_id = target_obj.id;

        let observation = world_with(vec![agent_obj, target_obj]);
        let available = [ActionKind::Support, ActionKind::DoNothing];
        let ctx = DecisionContext {
            agent_id,
            observation: &observation,
            available_actions: &available,
        };
        let decision = RuleBasedAgent.decide(&ctx);
        assert_eq!(decision.action.kind, ActionKind::Support);
        assert_eq!(decision.action.target, Some(target_id));
    }

    #[test]
    fn rule_based_agent_falls_back_to_do_nothing_when_goal_target_unobserved() {
        let mut agent_obj =
            KirObject::new("alice", ObjectKind::Custom("SimulatedAgent".to_string()));
        agent_obj
            .properties
            .insert("goals".to_string(), serde_json::json!(["support:acme"]));
        let agent_id = agent_obj.id;

        let observation = world_with(vec![agent_obj]);
        let available = [ActionKind::Support, ActionKind::DoNothing];
        let ctx = DecisionContext {
            agent_id,
            observation: &observation,
            available_actions: &available,
        };
        let decision = RuleBasedAgent.decide(&ctx);
        assert_eq!(decision.action.kind, ActionKind::DoNothing);
    }

    #[test]
    fn rule_based_agent_skips_goal_action_not_in_available_actions() {
        let mut agent_obj =
            KirObject::new("alice", ObjectKind::Custom("SimulatedAgent".to_string()));
        agent_obj
            .properties
            .insert("goals".to_string(), serde_json::json!(["support:acme"]));
        let agent_id = agent_obj.id;
        let target_obj = KirObject::new("acme", ObjectKind::Custom("Organization".to_string()));

        let observation = world_with(vec![agent_obj, target_obj]);
        let available = [ActionKind::DoNothing];
        let ctx = DecisionContext {
            agent_id,
            observation: &observation,
            available_actions: &available,
        };
        let decision = RuleBasedAgent.decide(&ctx);
        assert_eq!(decision.action.kind, ActionKind::DoNothing);
    }
}
