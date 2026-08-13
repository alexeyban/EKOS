//! The finite action vocabulary (RFC 0050, source document §11) and its
//! validation rules.

use ekos_kir::{KirId, RelationshipKind};
use ekos_runtime::World;
use serde::{Deserialize, Serialize};

/// One of the twelve actions an agent may choose (source document §11). No
/// escape hatch here, unlike `ObjectKind`/`RelationshipKind`/`EventKind` —
/// this vocabulary is closed by design; a new action kind is a scope
/// decision, not something a caller should be able to smuggle in as a
/// string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActionKind {
    PostMessage,
    SendMessage,
    Support,
    Oppose,
    ShareInformation,
    WithholdInformation,
    FormAlliance,
    BreakAlliance,
    RequestInformation,
    ChangeGoal,
    ChangeBelief,
    DoNothing,
}

impl ActionKind {
    /// Every action kind, in the order the source document lists them
    /// (§11) — the default vocabulary a scenario gets unless something
    /// narrows it (RFC 0051's Non-goals: no scenario-level vocabulary
    /// restriction yet).
    pub fn all() -> Vec<ActionKind> {
        vec![
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
        ]
    }

    /// Every action needs a target except `DoNothing`.
    pub fn requires_target(&self) -> bool {
        !matches!(self, ActionKind::DoNothing)
    }

    /// Whether executing this action should be knowable by every simulation
    /// participant (fanned out to all configured agents) or only by those
    /// directly involved (actor, and target if any). `ChangeGoal`/
    /// `ChangeBelief`/`DoNothing` affect no one but the actor, so they are
    /// self-only — not "public" (everyone) and not "private" (actor +
    /// target), since there is no target to share visibility with.
    pub fn is_public(&self) -> bool {
        matches!(
            self,
            ActionKind::PostMessage
                | ActionKind::Support
                | ActionKind::Oppose
                | ActionKind::FormAlliance
                | ActionKind::BreakAlliance
        )
    }

    /// The complement of `is_public` for target-bearing actions — a direct,
    /// targeted exchange only the actor and target learn about.
    pub fn is_private(&self) -> bool {
        matches!(
            self,
            ActionKind::SendMessage
                | ActionKind::ShareInformation
                | ActionKind::WithholdInformation
                | ActionKind::RequestInformation
        )
    }

    /// `ChangeGoal`/`ChangeBelief`/`DoNothing` — visible to no one but the
    /// actor.
    pub fn is_self_only(&self) -> bool {
        !self.is_public() && !self.is_private()
    }
}

/// A decided action: what kind, against whom (if any), with what content.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Action {
    pub kind: ActionKind,
    pub target: Option<KirId>,
    pub content: Option<String>,
    /// The message event this one is a reply to (RFC 0053) — meaningful only
    /// for `PostMessage`; any other kind carrying it is simply ignored by
    /// `execute_action`, not an error. Additive, not a new `ActionKind`
    /// variant: `ActionKind` stays exactly the 12 kinds RFC 0050 closed it
    /// at, unmodified by this field's existence.
    #[serde(default)]
    pub reply_to: Option<KirId>,
}

impl Action {
    pub fn new(kind: ActionKind) -> Self {
        Self {
            kind,
            target: None,
            content: None,
            reply_to: None,
        }
    }

    pub fn with_target(mut self, target: KirId) -> Self {
        self.target = Some(target);
        self
    }

    pub fn with_content(mut self, content: impl Into<String>) -> Self {
        self.content = Some(content.into());
        self
    }

    pub fn with_reply_to(mut self, reply_to: KirId) -> Self {
        self.reply_to = Some(reply_to);
        self
    }
}

/// Why an action was rejected by [`validate_action`]. A rejected action is
/// not silently dropped by the simulation loop — the reason is recorded and
/// the action is downgraded to `DoNothing` (see `simulation.rs`).
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ValidationError {
    #[error("{0:?} requires a target but none was given")]
    MissingTarget(ActionKind),
    #[error("target {0} is not in the acting agent's own observation")]
    TargetNotObserved(KirId),
    #[error("precondition failed for {kind:?}: {reason}")]
    PreconditionFailed { kind: ActionKind, reason: String },
}

/// Structural validation (target required/observed) plus the one real
/// precondition this RFC implements: `FormAlliance` requires an existing
/// `Custom("Trusts")` relationship from actor to target with
/// `properties["value"] > 0.4` (source document §11's own worked example).
/// Richer per-kind preconditions are named, deferred work — see the RFC's
/// Non-goals.
pub fn validate_action(
    actor_id: &KirId,
    action: &Action,
    observation: &World,
) -> Result<(), ValidationError> {
    if action.kind.requires_target() && action.target.is_none() {
        return Err(ValidationError::MissingTarget(action.kind));
    }

    if let Some(target) = action.target {
        let observed = observation.objects.iter().any(|o| o.id == target)
            || observation.events.iter().any(|e| e.id == target);
        if !observed {
            return Err(ValidationError::TargetNotObserved(target));
        }
    }

    if action.kind == ActionKind::FormAlliance {
        let target = action.target.expect("checked above");
        let trust_value = observation
            .relationships
            .iter()
            .find(|r| {
                r.from == *actor_id
                    && r.to == target
                    && matches!(&r.kind, RelationshipKind::Custom(k) if k == "Trusts")
            })
            .and_then(|r| r.properties.get("value"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        if trust_value <= 0.4 {
            return Err(ValidationError::PreconditionFailed {
                kind: ActionKind::FormAlliance,
                reason: format!("trust {trust_value} is not greater than 0.4"),
            });
        }
    }

    Ok(())
}
