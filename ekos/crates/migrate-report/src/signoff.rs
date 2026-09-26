//! RFC 0162 — the five sign-off preconditions.
//!
//! Sign-off is R4 (RFC 0161): two approvers and a typed confirmation. But before any of that, five
//! conditions have to hold, and **none of them is a matter of judgement at the moment of signing**.
//! That is the point: the decision a human makes at sign-off should be "do we switch the old system
//! off", not "is the evidence good enough", because the second question is answerable mechanically
//! and a tired person at the end of a project will answer it optimistically.

use crate::citation::Groundedness;
use serde::{Deserialize, Serialize};

/// What sign-off checks, and what each one found.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Preconditions {
    /// Units that are neither `validated` nor `signed_off`.
    pub incomplete_units: Vec<String>,
    /// Divergences with no approved disposition and no explaining rule.
    pub unexplained_divergences: usize,
    /// Units whose required validation tier has not run.
    pub tiers_not_run: Vec<String>,
    /// Planted controls that were expected and did not fire. **Includes the case of no controls at
    /// all**, because a tier that ran none has not demonstrated it can see anything.
    pub controls_missed: Vec<String>,
    /// Source objects with no translation and no disposition (RFC 0158's check).
    pub unclassified_objects: usize,
    pub groundedness: Groundedness,
    pub groundedness_threshold: f64,
}

/// One reason sign-off is refused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Blocker {
    pub condition: &'static str,
    pub detail: String,
}

impl Preconditions {
    /// Every condition that fails. Empty means sign-off may proceed to its R4 approval.
    ///
    /// Returns *all* of them rather than the first: somebody preparing a sign-off needs the whole
    /// list, and stopping at the first turns one review into five.
    pub fn blockers(&self) -> Vec<Blocker> {
        let mut out = Vec::new();

        if !self.incomplete_units.is_empty() {
            out.push(Blocker {
                condition: "units not validated",
                detail: format!(
                    "{} unit(s) are not validated or signed off: {}",
                    self.incomplete_units.len(),
                    preview(&self.incomplete_units)
                ),
            });
        }
        if self.unexplained_divergences > 0 {
            out.push(Blocker {
                condition: "unexplained divergences",
                detail: format!(
                    "{} divergence(s) have neither an approved disposition nor an explaining rule",
                    self.unexplained_divergences
                ),
            });
        }
        if !self.tiers_not_run.is_empty() {
            out.push(Blocker {
                condition: "required tier not run",
                detail: format!(
                    "{} unit(s) have not run their required tier: {}",
                    self.tiers_not_run.len(),
                    preview(&self.tiers_not_run)
                ),
            });
        }
        if !self.controls_missed.is_empty() {
            out.push(Blocker {
                condition: "planted control missed",
                detail: format!(
                    "{} control(s) did not fire: {}. A tier that cannot catch a planted defect does \
                     not get to report green.",
                    self.controls_missed.len(),
                    preview(&self.controls_missed)
                ),
            });
        }
        if self.unclassified_objects > 0 {
            out.push(Blocker {
                condition: "source coverage incomplete",
                detail: format!(
                    "{} source object(s) have neither a translation nor a disposition (RFC 0158). \
                     A migration reaching sign-off with an unclassified object is a defect, not a \
                     limitation.",
                    self.unclassified_objects
                ),
            });
        }
        let score = self.groundedness.score();
        if score < self.groundedness_threshold {
            out.push(Blocker {
                condition: "report not grounded",
                detail: format!(
                    "groundedness {score:.3} is below the {:.3} threshold (coverage {:.3} × \
                     validity {:.3} × grounded rate {:.3})",
                    self.groundedness_threshold,
                    self.groundedness.coverage,
                    self.groundedness.validity,
                    self.groundedness.grounded_rate
                ),
            });
        }
        out
    }

    pub fn passes(&self) -> bool {
        self.blockers().is_empty()
    }
}

fn preview(items: &[String]) -> String {
    let shown: Vec<&str> = items.iter().take(3).map(String::as_str).collect();
    if items.len() > shown.len() {
        format!(
            "{}, … and {} more",
            shown.join(", "),
            items.len() - shown.len()
        )
    } else {
        shown.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clean() -> Preconditions {
        Preconditions {
            incomplete_units: vec![],
            unexplained_divergences: 0,
            tiers_not_run: vec![],
            controls_missed: vec![],
            unclassified_objects: 0,
            groundedness: Groundedness {
                coverage: 1.0,
                validity: 1.0,
                grounded_rate: 1.0,
            },
            groundedness_threshold: 0.95,
        }
    }

    #[test]
    fn a_clean_migration_may_be_signed_off() {
        assert!(clean().passes());
        assert!(clean().blockers().is_empty());
    }

    /// Each of the five, independently. A precondition that only fails in combination with another
    /// is not a precondition.
    #[test]
    fn each_precondition_blocks_on_its_own() {
        let cases: Vec<(&str, Preconditions)> = vec![
            (
                "units not validated",
                Preconditions {
                    incomplete_units: vec!["public.orders".into()],
                    ..clean()
                },
            ),
            (
                "unexplained divergences",
                Preconditions {
                    unexplained_divergences: 1,
                    ..clean()
                },
            ),
            (
                "required tier not run",
                Preconditions {
                    tiers_not_run: vec!["public.orders".into()],
                    ..clean()
                },
            ),
            (
                "planted control missed",
                Preconditions {
                    controls_missed: vec!["column_swap".into()],
                    ..clean()
                },
            ),
            (
                "source coverage incomplete",
                Preconditions {
                    unclassified_objects: 9,
                    ..clean()
                },
            ),
            (
                "report not grounded",
                Preconditions {
                    groundedness: Groundedness {
                        coverage: 0.5,
                        validity: 1.0,
                        grounded_rate: 0.5,
                    },
                    ..clean()
                },
            ),
        ];
        for (condition, p) in cases {
            assert!(!p.passes(), "{condition} should block");
            let b = p.blockers();
            assert_eq!(b.len(), 1, "{condition}: {b:?}");
            assert_eq!(b[0].condition, condition);
        }
    }

    /// Somebody preparing a sign-off needs the whole list, not the first item five times.
    #[test]
    fn every_failing_condition_is_reported_at_once() {
        let p = Preconditions {
            incomplete_units: vec!["a".into()],
            unexplained_divergences: 2,
            controls_missed: vec!["dropped_row".into()],
            unclassified_objects: 3,
            ..clean()
        };
        assert_eq!(p.blockers().len(), 4);
    }

    #[test]
    fn a_missed_control_says_why_it_matters() {
        let p = Preconditions {
            controls_missed: vec!["column_swap".into()],
            ..clean()
        };
        assert!(
            p.blockers()[0]
                .detail
                .contains("does not get to report green"),
            "{:?}",
            p.blockers()[0]
        );
    }

    #[test]
    fn the_groundedness_blocker_shows_all_three_components() {
        let p = Preconditions {
            groundedness: Groundedness {
                coverage: 0.9,
                validity: 0.8,
                grounded_rate: 0.7,
            },
            ..clean()
        };
        let d = &p.blockers()[0].detail;
        assert!(d.contains("0.900"), "{d}");
        assert!(d.contains("0.800"), "{d}");
        assert!(d.contains("0.700"), "{d}");
    }

    #[test]
    fn a_long_list_is_previewed_with_a_count() {
        let p = Preconditions {
            incomplete_units: (0..10).map(|i| format!("t{i}")).collect(),
            ..clean()
        };
        let d = &p.blockers()[0].detail;
        assert!(d.contains("10 unit(s)"), "{d}");
        assert!(d.contains("and 7 more"), "{d}");
    }
}
