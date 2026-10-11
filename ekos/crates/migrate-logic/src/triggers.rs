//! Trigger redesign proposals.
//!
//! No trigger is translated. The targets have no row triggers, and every replacement moves *when*
//! the logic runs: a validation that rejected a bad row on write becomes a check that finds it
//! afterwards. That is a behavioural change someone has to accept, so each class gets a proposed
//! shape that needs a disposition, and the two classes that cannot be read as one thing get none
//! (RFC 0164).

use serde::{Deserialize, Serialize};

/// What a trigger of one class is proposed to become on the target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TriggerProposal {
    /// Stable id of the shape, e.g. `TRIGGER.AUDIT.LOAD_WRITE`.
    pub shape: String,
    /// The replacement, in a sentence.
    pub proposal: String,
    /// What changes about when the logic runs. Never empty: this is what the reviewer accepts.
    pub behaviour_change: String,
}

/// The outcome for one trigger.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "outcome")]
pub enum TriggerOutcome {
    Proposed(TriggerProposal),
    /// No shape is proposed; a person decides. `why` says what stopped it.
    HumanDecision {
        why: String,
    },
}

fn proposed(shape: &str, proposal: &str, behaviour_change: &str) -> TriggerOutcome {
    TriggerOutcome::Proposed(TriggerProposal {
        shape: shape.into(),
        proposal: proposal.into(),
        behaviour_change: behaviour_change.into(),
    })
}

/// The proposal for a trigger of `classification` (RFC 0163's label, as recorded on the object).
pub fn propose_trigger(classification: &str) -> TriggerOutcome {
    match classification {
        "Audit" => proposed(
            "TRIGGER.AUDIT.LOAD_WRITE",
            "write the audit row explicitly in the load path, in the same batch as the row it records",
            "the audit row is written by the loader, not by the database: a write that bypasses \
             the loader leaves no audit row",
        ),
        "DerivedColumn" => proposed(
            "TRIGGER.DERIVED.COMPUTED",
            "compute the value in the load, or as a MATERIALIZED column on the target table",
            "the value is fixed when the row is loaded; a later change to its inputs does not \
             recompute it unless the row is loaded again",
        ),
        "Validation" => proposed(
            "TRIGGER.VALIDATION.DQ_RULE",
            "a data-quality rule measured after load, plus the same check in the writing application",
            "a bad row is no longer rejected on write: it arrives and is detected afterwards",
        ),
        "Cascade" => proposed(
            "TRIGGER.CASCADE.ORDERED_WRITES",
            "explicit downstream writes in the load, ordered after the row that causes them",
            "the dependent rows are written by the loader in a later step, so they are briefly \
             absent and are missing entirely if that step fails",
        ),
        "Mixed" => TriggerOutcome::HumanDecision {
            why: "the trigger does more than one kind of thing; each part needs its own decision"
                .into(),
        },
        "Unknown" => TriggerOutcome::HumanDecision {
            why: "what the trigger does could not be classified from its function".into(),
        },
        other => TriggerOutcome::HumanDecision {
            why: format!("no proposal is defined for the class {other:?}"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_single_purpose_class_has_its_own_shape_and_names_what_changes() {
        let mut shapes = Vec::new();
        for class in ["Audit", "DerivedColumn", "Validation", "Cascade"] {
            let TriggerOutcome::Proposed(p) = propose_trigger(class) else {
                panic!("{class} has no proposal");
            };
            assert!(!p.behaviour_change.is_empty(), "{class}");
            shapes.push(p.shape);
        }
        shapes.sort();
        shapes.dedup();
        assert_eq!(shapes.len(), 4);
    }

    #[test]
    fn mixed_unknown_and_unrecognised_classes_get_no_proposal() {
        for class in ["Mixed", "Unknown", "", "Auditt"] {
            assert!(
                matches!(propose_trigger(class), TriggerOutcome::HumanDecision { .. }),
                "{class}"
            );
        }
    }
}
