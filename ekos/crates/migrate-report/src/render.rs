//! RFC 0162 — rendering, deterministically.
//!
//! The report is **compiled**: a sequence of sections, each a query plus a template. No section's
//! factual content originates in a model. An LLM may write section introductions and transitions,
//! and [`crate::citation`] checks that such prose asserts nothing.
//!
//! Recompiling from the same snapshot must produce byte-identical output — the same determinism
//! requirement `docs-gen` already meets, and the reason a signed report can be *re-derived* rather
//! than merely archived.

use crate::citation::{CitationProblem, Claim, Groundedness};
use crate::signoff::Preconditions;
use serde::{Deserialize, Serialize};

/// One section of the report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Section {
    pub heading: String,
    pub claims: Vec<Claim>,
}

/// A whole report, before rendering.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Report {
    pub project: String,
    /// The ledger state this was compiled from: an as-of timestamp and a content hash.
    pub snapshot: String,
    pub sections: Vec<Section>,
    pub groundedness: Groundedness,
    pub problems: Vec<CitationProblem>,
    pub preconditions: Preconditions,
}

impl Report {
    /// Whether this report may be signed off.
    ///
    /// A report below the gate is still *produced* — the person who has to fix the gaps needs to see
    /// them — it simply cannot be signed. Withholding it would make the gaps harder to close.
    pub fn signable(&self) -> bool {
        self.problems.is_empty() && self.preconditions.passes()
    }
}

/// Render to Markdown. Deterministic: same input, same bytes.
pub fn markdown(r: &Report) -> String {
    let mut out = String::new();
    out.push_str(&format!("# Migration report — {}\n\n", r.project));
    out.push_str(&format!("**Snapshot:** `{}`\n\n", r.snapshot));

    // The verdict first. A reader who stops after one screen should know whether this is signable.
    if r.signable() {
        out.push_str(
            "**Signable.** Every precondition holds and every claim resolves to a fact.\n\n",
        );
    } else {
        out.push_str("**Not signable.** The blockers are listed below, before the content.\n\n");
        out.push_str("## Blockers\n\n");
        for b in r.preconditions.blockers() {
            out.push_str(&format!("- **{}** — {}\n", b.condition, b.detail));
        }
        for p in &r.problems {
            out.push_str(&format!("- **citation** — {}\n", describe(p)));
        }
        out.push('\n');
    }

    out.push_str(&format!(
        "**Groundedness:** {:.3} (coverage {:.3} × validity {:.3} × grounded rate {:.3}), threshold {:.3}\n\n",
        r.groundedness.score(),
        r.groundedness.coverage,
        r.groundedness.validity,
        r.groundedness.grounded_rate,
        r.preconditions.groundedness_threshold
    ));

    for s in &r.sections {
        out.push_str(&format!("## {}\n\n", s.heading));
        if s.claims.is_empty() {
            // An empty section says so. Omitting it would let a reader assume it was inspected and
            // had nothing to report, which is a different statement.
            out.push_str("_Nothing recorded for this section._\n\n");
            continue;
        }
        for c in &s.claims {
            if c.cites.is_empty() {
                out.push_str(&format!("{}\n\n", c.text));
            } else {
                out.push_str(&format!("{} [{}]\n\n", c.text, c.cites.join("] [")));
            }
        }
    }
    out
}

fn describe(p: &CitationProblem) -> String {
    match p {
        CitationProblem::Uncited { text } => format!("no citation: {text:?}"),
        CitationProblem::Unresolvable { text, fact_id } => {
            format!("{fact_id} does not exist in the snapshot, cited by {text:?}")
        }
        CitationProblem::Unsupported {
            text,
            fact_id,
            fact_kind,
            expected,
        } => format!(
            "{fact_id} is a {fact_kind} and cannot support {text:?} (expected one of {})",
            expected.join(", ")
        ),
        CitationProblem::NarrativeAssertsFact { text } => {
            format!("narrative asserts a fact without citing one: {text:?}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::citation::ClaimKind;

    fn report(signable: bool) -> Report {
        let g = Groundedness {
            coverage: 1.0,
            validity: 1.0,
            grounded_rate: 1.0,
        };
        Report {
            project: "ledgersmb".into(),
            snapshot: "as-of 2026-09-26T00:00:00Z #abc123".into(),
            sections: vec![Section {
                heading: "Validation".into(),
                claims: vec![Claim::fact(
                    "public.orders validated at V3 with no unexplained divergences.",
                    ClaimKind::ValidationResult,
                    vec!["V1".into()],
                )],
            }],
            groundedness: g,
            problems: vec![],
            preconditions: Preconditions {
                incomplete_units: if signable {
                    vec![]
                } else {
                    vec!["public.audit".into()]
                },
                unexplained_divergences: 0,
                tiers_not_run: vec![],
                controls_missed: vec![],
                unclassified_objects: 0,
                groundedness: g,
                groundedness_threshold: 0.95,
            },
        }
    }

    /// The property that makes a signed report re-derivable rather than merely archived.
    #[test]
    fn rendering_is_byte_identical_for_the_same_input() {
        let r = report(true);
        assert_eq!(markdown(&r), markdown(&r));
    }

    #[test]
    fn a_signable_report_says_so_in_the_first_screen() {
        let out = markdown(&report(true));
        assert!(out.starts_with("# Migration report — ledgersmb"), "{out}");
        assert!(out.contains("**Signable.**"), "{out}");
        assert!(out.contains("[V1]"), "citations must be visible: {out}");
    }

    /// The blockers come before the content: a reader who stops after one screen must know.
    #[test]
    fn an_unsignable_report_leads_with_its_blockers() {
        let out = markdown(&report(false));
        assert!(out.contains("**Not signable.**"), "{out}");
        let blockers_at = out.find("## Blockers").unwrap();
        let content_at = out.find("## Validation").unwrap();
        assert!(blockers_at < content_at, "blockers must precede content");
        assert!(out.contains("public.audit"), "{out}");
    }

    /// A report below the gate is still produced — the person fixing the gaps needs to see them.
    #[test]
    fn an_ungrounded_report_is_produced_not_withheld() {
        let mut r = report(true);
        r.problems = vec![CitationProblem::Uncited {
            text: "500 rows loaded.".into(),
        }];
        assert!(!r.signable());
        let out = markdown(&r);
        assert!(out.contains("no citation"), "{out}");
        assert!(
            out.contains("## Validation"),
            "the content must still be there: {out}"
        );
    }

    /// An omitted section reads as "inspected and clean". An empty one says what it is.
    #[test]
    fn an_empty_section_says_it_is_empty() {
        let mut r = report(true);
        r.sections.push(Section {
            heading: "Divergences".into(),
            claims: vec![],
        });
        let out = markdown(&r);
        assert!(out.contains("## Divergences"), "{out}");
        assert!(
            out.contains("_Nothing recorded for this section._"),
            "{out}"
        );
    }

    #[test]
    fn every_citation_problem_renders_something_a_human_can_act_on() {
        for p in [
            CitationProblem::Uncited { text: "x".into() },
            CitationProblem::Unresolvable {
                text: "x".into(),
                fact_id: "GHOST".into(),
            },
            CitationProblem::Unsupported {
                text: "x".into(),
                fact_id: "D1".into(),
                fact_kind: "MigrationTargetDesign".into(),
                expected: vec!["MigrationValidationResult".into()],
            },
            CitationProblem::NarrativeAssertsFact { text: "x".into() },
        ] {
            let d = describe(&p);
            assert!(!d.is_empty());
            assert!(d.len() > 10, "{d}");
        }
    }
}
