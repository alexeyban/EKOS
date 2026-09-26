//! RFC 0161 — risk, computed rather than assigned.
//!
//! Approval workflows fail in three predictable ways, and all three are avoidable mechanically.
//!
//! **Risk assigned by category.** "Schema changes need approval" gives the trivial rename and the
//! column drop that breaks eleven downstream consumers the same dialog. Approvers learn the dialog
//! means nothing and click through it. So risk here is a function of the *situation*: statement
//! class, environment, lossiness, blast radius, affected rows — and every escalation that fired is
//! recorded, so an approver sees *why* rather than a letter.
//!
//! **Approval granted on evidence that then changes.** Handled in [`crate::request`].
//!
//! **The agent approving its own proposal.** Handled in [`crate::lifecycle`], by there being no
//! path rather than a check that could be wrong.

use serde::{Deserialize, Serialize};

/// How much a proposed action can cost.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskClass {
    /// Catalog reads, cheap profiling, proposals. No gate.
    R0,
    /// Sandbox writes, scans within budget. Automatic within policy, logged.
    R1,
    /// DDL and loads in staging, starting incremental sync. One approver.
    R2,
    /// Lossy mapping, dedup, NULL-handling change, schema rename or split, logic change.
    /// One approver **plus** evidence review.
    R3,
    /// Anything touching production, `DROP`/`TRUNCATE`/`DELETE`, cutover, sign-off. Two approvers
    /// and a typed confirmation.
    R4,
}

impl RiskClass {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::R0 => "R0",
            Self::R1 => "R1",
            Self::R2 => "R2",
            Self::R3 => "R3",
            Self::R4 => "R4",
        }
    }

    /// How many distinct approvers this class needs.
    pub fn approvers_required(self) -> usize {
        match self {
            Self::R0 | Self::R1 => 0,
            Self::R2 | Self::R3 => 1,
            Self::R4 => 2,
        }
    }

    /// Whether the request must render its evidence — the IR diff, affected rows, blast radius —
    /// and record that it was shown.
    pub fn requires_evidence_review(self) -> bool {
        self >= Self::R3
    }

    /// Whether the approver must type the object's name rather than click a box.
    pub fn requires_typed_confirmation(self) -> bool {
        self == Self::R4
    }

    /// Whether a policy may approve this automatically.
    pub fn is_automatic(self) -> bool {
        self <= Self::R1
    }
}

/// What the action is, in the terms the risk function needs. Deliberately not the statement itself:
/// this crate has no SQL parser and no database, so it stays testable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionFacts {
    /// From RFC 0160's classifier: `read`, `ddl_create`, `dml_insert`, `destructive`, …
    pub statement_class: String,
    /// `sandbox` | `staging` | `production`.
    pub environment: String,
    /// From RFC 0159: `exact` | `widening` | `narrowing_safe` | `lossy` | `behavioural`.
    pub lossiness: Option<String>,
    /// How many objects depend on what this touches — `ekos_impact` over views, functions, ETL and
    /// application code, multi-hop. The number EKOS has and a schema-only tool does not.
    pub blast_radius: usize,
    /// Rows the action affects, where it was measured. `None` is **not** zero: an unmeasured action
    /// cannot be de-escalated by a measurement nobody took.
    pub affected_rows: Option<i64>,
}

/// Thresholds. Policy, not constants: the right blast radius for a 12-table application is not the
/// right one for a 900-table warehouse, and a constant in code is a constant somebody has to patch.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Thresholds {
    pub blast_radius: usize,
    pub affected_rows: i64,
}

impl Default for Thresholds {
    fn default() -> Self {
        Self {
            blast_radius: 10,
            affected_rows: 1000,
        }
    }
}

/// One reason the risk went up.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Escalation {
    pub to: RiskClass,
    pub because: String,
}

/// The computed assessment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RiskAssessment {
    pub class: RiskClass,
    pub base: RiskClass,
    /// Every escalation that fired, in order. This is what an approver reads instead of a letter.
    pub escalations: Vec<Escalation>,
}

impl RiskAssessment {
    /// A one-line summary naming the reasons, never just the class.
    pub fn summary(&self) -> String {
        if self.escalations.is_empty() {
            return format!("{} ({})", self.class.as_str(), self.base.as_str());
        }
        format!(
            "{} because: {}",
            self.class.as_str(),
            self.escalations
                .iter()
                .map(|e| e.because.as_str())
                .collect::<Vec<_>>()
                .join("; ")
        )
    }
}

/// The base class for a `(statement class, environment)` pair.
fn base_class(statement_class: &str, environment: &str) -> RiskClass {
    use RiskClass as R;
    let production = environment == "production";
    let staging = environment == "staging";
    match statement_class {
        "read" => {
            if production {
                R::R1
            } else {
                R::R0
            }
        }
        "destructive" => {
            if production {
                R::R4
            } else if staging {
                R::R3
            } else {
                R::R1
            }
        }
        // Every other write.
        _ => {
            if production {
                R::R4
            } else if staging {
                R::R2
            } else {
                R::R1
            }
        }
    }
}

/// Compute the risk of an action.
///
/// Every condition that fires is recorded, and the class is the maximum of the base and everything
/// recorded. The two are kept separate deliberately: a first version raised the class and appended a
/// reason in one step, so a second reason at the same class was **silently dropped** — an approver
/// saw "R3 because the mapping is lossy" and never learned that 40 consumers depend on it and 17,000
/// rows are affected. The whole point of this module is that an approver reads the reasons.
pub fn assess(facts: &ActionFacts, t: &Thresholds) -> RiskAssessment {
    let base = base_class(&facts.statement_class, &facts.environment);
    let writes = facts.statement_class != "read";
    let mut escalations: Vec<Escalation> = Vec::new();

    // Lossiness. `behavioural` escalates as hard as `lossy` on purpose: losing foreign-key
    // enforcement breaks nothing on load day and everything six months later, and it is the finding
    // people skip.
    match facts.lossiness.as_deref() {
        Some("lossy") => escalations.push(Escalation {
            to: RiskClass::R3,
            because: "the mapping is lossy: values will be changed or lost".into(),
        }),
        Some("behavioural") => escalations.push(Escalation {
            to: RiskClass::R3,
            because: "the target behaves differently even though it holds every value — nothing \
                      breaks on load day"
                .into(),
        }),
        _ => {}
    }

    if facts.blast_radius > t.blast_radius {
        escalations.push(Escalation {
            to: RiskClass::R3,
            because: format!(
                "{} downstream consumers depend on this (threshold {})",
                facts.blast_radius, t.blast_radius
            ),
        });
    }

    match facts.affected_rows {
        Some(n) if n > t.affected_rows => escalations.push(Escalation {
            to: RiskClass::R3,
            because: format!("{n} rows affected (threshold {})", t.affected_rows),
        }),
        // An unmeasured action is not a harmless one. It cannot be *escalated* by a number nobody
        // has, so this records the gap at the class already reached rather than raising it — the
        // assessment says the measurement is missing instead of implying zero.
        None if writes && base > RiskClass::R1 => escalations.push(Escalation {
            to: base,
            because: "affected rows were not measured, so no row-count escalation could be \
                      evaluated"
                .into(),
        }),
        _ => {}
    }

    // Production is absolute for anything that writes. A *read* in production is R1 — real load on
    // somebody's live system, worth logging, and not a two-approver event.
    if facts.environment == "production" && writes {
        escalations.push(Escalation {
            to: RiskClass::R4,
            because: "the action touches production".into(),
        });
    }

    let class = escalations.iter().map(|e| e.to).fold(base, std::cmp::max);

    RiskAssessment {
        class,
        base,
        escalations,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use RiskClass as R;

    fn facts(class: &str, env: &str) -> ActionFacts {
        ActionFacts {
            statement_class: class.into(),
            environment: env.into(),
            lossiness: None,
            blast_radius: 0,
            affected_rows: Some(0),
        }
    }

    #[test]
    fn the_same_statement_is_a_different_risk_in_a_different_environment() {
        assert_eq!(
            assess(&facts("destructive", "sandbox"), &Thresholds::default()).class,
            R::R1
        );
        assert_eq!(
            assess(&facts("destructive", "staging"), &Thresholds::default()).class,
            R::R3
        );
        assert_eq!(
            assess(&facts("destructive", "production"), &Thresholds::default()).class,
            R::R4
        );
    }

    #[test]
    fn a_read_is_ungated_outside_production() {
        assert_eq!(
            assess(&facts("read", "sandbox"), &Thresholds::default()).class,
            R::R0
        );
        assert_eq!(
            assess(&facts("read", "staging"), &Thresholds::default()).class,
            R::R0
        );
        // Even a read in production is logged, because it is load on somebody's live system.
        assert_eq!(
            assess(&facts("read", "production"), &Thresholds::default()).class,
            R::R1
        );
    }

    /// The failure mode this design exists to avoid: a trivial change and a wide one getting the
    /// same dialog.
    #[test]
    fn blast_radius_separates_a_trivial_change_from_a_wide_one() {
        let t = Thresholds::default();
        let narrow = ActionFacts {
            blast_radius: 1,
            ..facts("ddl_alter", "staging")
        };
        let wide = ActionFacts {
            blast_radius: 23,
            ..facts("ddl_alter", "staging")
        };
        assert_eq!(assess(&narrow, &t).class, R::R2);
        let w = assess(&wide, &t);
        assert_eq!(w.class, R::R3);
        assert!(
            w.summary().contains("23 downstream consumers"),
            "{}",
            w.summary()
        );
    }

    /// `behavioural` escalates as hard as `lossy`, because it is the class people dismiss.
    #[test]
    fn a_behavioural_change_escalates_as_hard_as_a_lossy_one() {
        let t = Thresholds::default();
        for l in ["lossy", "behavioural"] {
            let f = ActionFacts {
                lossiness: Some(l.into()),
                ..facts("dml_insert", "staging")
            };
            assert_eq!(assess(&f, &t).class, R::R3, "{l}");
        }
        // And a safe mapping does not escalate.
        for l in ["exact", "widening", "narrowing_safe"] {
            let f = ActionFacts {
                lossiness: Some(l.into()),
                ..facts("dml_insert", "staging")
            };
            assert_eq!(assess(&f, &t).class, R::R2, "{l}");
        }
    }

    /// An unmeasured action cannot be de-escalated by a measurement nobody took, and the assessment
    /// says the measurement is missing rather than implying zero.
    #[test]
    fn an_unmeasured_action_records_that_it_was_unmeasured() {
        let f = ActionFacts {
            affected_rows: None,
            ..facts("dml_mutate", "staging")
        };
        let a = assess(&f, &Thresholds::default());
        assert_eq!(a.class, R::R2);
        assert!(
            a.escalations
                .iter()
                .any(|e| e.because.contains("not measured")),
            "{:?}",
            a.escalations
        );
    }

    #[test]
    fn production_is_absolute_and_nothing_de_escalates_it() {
        let f = ActionFacts {
            lossiness: Some("exact".into()),
            blast_radius: 0,
            affected_rows: Some(0),
            ..facts("dml_insert", "production")
        };
        let a = assess(&f, &Thresholds::default());
        assert_eq!(a.class, R::R4);
        assert!(
            a.summary().contains("touches production"),
            "{}",
            a.summary()
        );
    }

    #[test]
    fn the_gates_scale_with_the_class() {
        assert_eq!(R::R0.approvers_required(), 0);
        assert_eq!(R::R1.approvers_required(), 0);
        assert_eq!(R::R2.approvers_required(), 1);
        assert_eq!(R::R3.approvers_required(), 1);
        assert_eq!(R::R4.approvers_required(), 2);
        assert!(!R::R2.requires_evidence_review());
        assert!(R::R3.requires_evidence_review());
        assert!(R::R4.requires_typed_confirmation());
        assert!(!R::R3.requires_typed_confirmation());
        assert!(R::R1.is_automatic());
        assert!(!R::R2.is_automatic());
    }

    /// The bug this guards: a first version raised the class and appended the reason in one step,
    /// so a second reason at the same class was silently dropped and the approver saw only the
    /// first one.
    #[test]
    fn the_summary_names_every_reason() {
        let f = ActionFacts {
            lossiness: Some("lossy".into()),
            blast_radius: 40,
            affected_rows: Some(17_000),
            ..facts("ddl_alter", "staging")
        };
        let a = assess(&f, &Thresholds::default());
        let s = a.summary();
        assert!(s.contains("lossy"), "{s}");
        assert!(s.contains("40 downstream"), "{s}");
        assert!(s.contains("17000 rows"), "{s}");
    }

    #[test]
    fn thresholds_are_policy_not_constants() {
        let lenient = Thresholds {
            blast_radius: 100,
            affected_rows: 1_000_000,
        };
        let f = ActionFacts {
            blast_radius: 40,
            affected_rows: Some(17_000),
            ..facts("ddl_alter", "staging")
        };
        assert_eq!(assess(&f, &lenient).class, R::R2);
        assert_eq!(assess(&f, &Thresholds::default()).class, R::R3);
    }
}
