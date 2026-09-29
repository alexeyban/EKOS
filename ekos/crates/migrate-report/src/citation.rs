//! RFC 0162 — citations, and verifying them.
//!
//! The report is the deliverable: what a data owner reads before agreeing a decade-old system can be
//! switched off, and what an auditor reads two years later when a number looks wrong.
//!
//! A generated narrative that is 95% accurate is worthless for that, because the reader cannot tell
//! which 5%. The only useful property is that every claim is traceable — so the report is compiled
//! from facts, and every factual sentence carries the ids it rests on.
//!
//! # On reuse
//!
//! RFC 0162 said this metric would be reused from `crates/evals`. That was too strong: the evals
//! groundedness evaluator is built around a `Scenario` — expected facts, refusal phrasings, an
//! answer from a model — and a compiled report has none of those. What is shared is the **shape** of
//! the metric (coverage × validity × grounded rate) and its reasoning, not the code. Saying so is
//! cheaper than a wrapper that pretends otherwise.

use serde::{Deserialize, Serialize};

/// What kind of thing a claim asserts, and therefore what kind of fact can support it.
///
/// The support check is **structural, not semantic**, and deliberately so: it catches the common
/// failure where a plausible nearby id gets attached to a sentence, without needing a model in the
/// verification loop — which would reintroduce the problem one level up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimKind {
    /// A row count, a divergence count, a validation outcome.
    ValidationResult,
    /// A data-quality or compatibility finding, and how many rows it affects.
    Finding,
    /// A profile measurement.
    Profile,
    /// A type mapping or table design decision.
    Design,
    /// Who approved what.
    Approval,
    /// A unit's state or a transition.
    UnitState,
    /// Connective prose. Carries no citation and asserts no fact — and the verifier checks that it
    /// asserts none, rather than trusting the author.
    Narrative,
}

impl ClaimKind {
    /// The fact kinds that can support a claim of this kind.
    pub fn supported_by(self) -> &'static [&'static str] {
        match self {
            Self::ValidationResult => &["MigrationValidationRun", "MigrationValidationResult"],
            Self::Finding => &["MigrationFinding", "MigrationDrift"],
            Self::Profile => &["MigrationTableProfile", "MigrationColumnProfile"],
            Self::Design => &["MigrationTargetDesign", "MigrationTypeMapping"],
            Self::Approval => &["MigrationApproval"],
            Self::UnitState => &["MigrationUnit", "MigrationProject"],
            Self::Narrative => &[],
        }
    }

    pub fn requires_citation(self) -> bool {
        self != Self::Narrative
    }
}

/// One sentence in the report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claim {
    pub text: String,
    pub kind: ClaimKind,
    /// The fact ids this rests on. Empty is only valid for [`ClaimKind::Narrative`].
    pub cites: Vec<String>,
}

impl Claim {
    pub fn narrative(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            kind: ClaimKind::Narrative,
            cites: Vec::new(),
        }
    }

    pub fn fact(text: impl Into<String>, kind: ClaimKind, cites: Vec<String>) -> Self {
        Self {
            text: text.into(),
            kind,
            cites,
        }
    }
}

/// Why a citation failed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "problem")]
pub enum CitationProblem {
    /// A factual claim with no citation at all. The most likely shape of an LLM-introduced number.
    Uncited { text: String },
    /// The cited id is not in the snapshot. A hard error: the report cannot be signed.
    Unresolvable { text: String, fact_id: String },
    /// The cited fact exists but is the wrong kind to support the claim.
    Unsupported {
        text: String,
        fact_id: String,
        fact_kind: String,
        expected: Vec<String>,
    },
    /// Connective prose asserting a number. Narrative may explain; it may not state.
    NarrativeAssertsFact { text: String },
}

/// The three components, and their product.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Groundedness {
    /// Factual claims that carry at least one citation, over all factual claims.
    pub coverage: f64,
    /// Citations that resolve *and* support their claim, over all citations.
    pub validity: f64,
    /// Factual claims where every citation resolved and supported, over all factual claims.
    pub grounded_rate: f64,
}

impl Groundedness {
    pub fn score(&self) -> f64 {
        self.coverage * self.validity * self.grounded_rate
    }
}

/// A snapshot of the facts a report was compiled from: id → kind.
pub type FactKinds = std::collections::BTreeMap<String, String>;

/// Numbers in prose are how an LLM-introduced fact gets in. A narrative sentence containing a digit
/// is treated as asserting something.
///
/// Deliberately blunt: a false positive costs an author one rewrite, and a false negative is an
/// uncited number in a document somebody signs.
fn narrative_asserts_fact(text: &str) -> bool {
    text.chars().any(|c| c.is_ascii_digit())
}

/// Verify every claim against the snapshot.
pub fn verify(claims: &[Claim], facts: &FactKinds) -> (Groundedness, Vec<CitationProblem>) {
    let mut problems = Vec::new();
    let mut factual = 0usize;
    let mut cited = 0usize;
    let mut fully_grounded = 0usize;
    let mut citations = 0usize;
    let mut valid_citations = 0usize;

    for c in claims {
        if !c.kind.requires_citation() {
            if narrative_asserts_fact(&c.text) {
                problems.push(CitationProblem::NarrativeAssertsFact {
                    text: c.text.clone(),
                });
            }
            continue;
        }
        factual += 1;
        if c.cites.is_empty() {
            problems.push(CitationProblem::Uncited {
                text: c.text.clone(),
            });
            continue;
        }
        cited += 1;

        let mut all_ok = true;
        for id in &c.cites {
            citations += 1;
            match facts.get(id) {
                None => {
                    all_ok = false;
                    problems.push(CitationProblem::Unresolvable {
                        text: c.text.clone(),
                        fact_id: id.clone(),
                    });
                }
                Some(kind) if !c.kind.supported_by().contains(&kind.as_str()) => {
                    all_ok = false;
                    problems.push(CitationProblem::Unsupported {
                        text: c.text.clone(),
                        fact_id: id.clone(),
                        fact_kind: kind.clone(),
                        expected: c
                            .kind
                            .supported_by()
                            .iter()
                            .map(std::string::ToString::to_string)
                            .collect(),
                    });
                }
                Some(_) => valid_citations += 1,
            }
        }
        if all_ok {
            fully_grounded += 1;
        }
    }

    // A report with no factual claims is not perfectly grounded — it is empty, and scoring it 1.0
    // would let a report that says nothing pass a gate a real one has to clear.
    let ratio = |n: usize, d: usize| if d == 0 { 0.0 } else { n as f64 / d as f64 };
    (
        Groundedness {
            coverage: ratio(cited, factual),
            validity: ratio(valid_citations, citations),
            grounded_rate: ratio(fully_grounded, factual),
        },
        problems,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(pairs: &[(&str, &str)]) -> FactKinds {
        pairs
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect()
    }

    #[test]
    fn a_fully_cited_report_scores_one() {
        let claims = [
            Claim::fact(
                "500 rows loaded.",
                ClaimKind::ValidationResult,
                vec!["V1".into()],
            ),
            Claim::narrative("The load proceeded in three chunks."),
        ];
        let (g, p) = verify(&claims, &facts(&[("V1", "MigrationValidationResult")]));
        assert!(p.is_empty(), "{p:?}");
        assert_eq!(g.score(), 1.0);
    }

    #[test]
    fn an_unresolvable_citation_is_a_problem_and_drags_every_component() {
        let claims = [Claim::fact(
            "500 rows loaded.",
            ClaimKind::ValidationResult,
            vec!["GHOST".into()],
        )];
        let (g, p) = verify(&claims, &facts(&[]));
        assert!(matches!(p[0], CitationProblem::Unresolvable { .. }));
        assert_eq!(g.coverage, 1.0, "it did carry a citation");
        assert_eq!(g.validity, 0.0);
        assert_eq!(g.grounded_rate, 0.0);
        assert_eq!(g.score(), 0.0);
    }

    /// The common failure: a plausible nearby id attached to the wrong kind of sentence.
    #[test]
    fn a_citation_of_the_wrong_kind_does_not_support_the_claim() {
        let claims = [Claim::fact(
            "500 rows loaded.",
            ClaimKind::ValidationResult,
            vec!["D1".into()],
        )];
        let (g, p) = verify(&claims, &facts(&[("D1", "MigrationTargetDesign")]));
        match &p[0] {
            CitationProblem::Unsupported {
                fact_kind,
                expected,
                ..
            } => {
                assert_eq!(fact_kind, "MigrationTargetDesign");
                assert!(expected.contains(&"MigrationValidationResult".to_string()));
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(g.validity, 0.0);
    }

    #[test]
    fn an_uncited_factual_claim_is_caught() {
        let claims = [Claim::fact(
            "500 rows loaded.",
            ClaimKind::ValidationResult,
            vec![],
        )];
        let (g, p) = verify(&claims, &facts(&[]));
        assert!(matches!(p[0], CitationProblem::Uncited { .. }));
        assert_eq!(g.coverage, 0.0);
    }

    /// Narrative may explain; it may not state. A number in prose is how an LLM-introduced fact
    /// gets into a document somebody signs.
    #[test]
    fn narrative_containing_a_number_is_refused() {
        let claims = [Claim::narrative("All 500 rows arrived intact.")];
        let (_, p) = verify(&claims, &facts(&[]));
        assert!(
            matches!(p[0], CitationProblem::NarrativeAssertsFact { .. }),
            "{p:?}"
        );
        // And prose without a number is fine.
        let ok = [Claim::narrative("The load proceeded without incident.")];
        assert!(verify(&ok, &facts(&[])).1.is_empty());
    }

    /// An empty report must not score 1.0 — that would let a report saying nothing clear a gate a
    /// real one has to.
    #[test]
    fn an_empty_report_is_not_perfectly_grounded() {
        let (g, p) = verify(&[], &facts(&[]));
        assert_eq!(g.score(), 0.0);
        assert!(p.is_empty());
        // Nor does a report of pure narrative.
        let (g2, _) = verify(&[Claim::narrative("Everything went well.")], &facts(&[]));
        assert_eq!(g2.score(), 0.0);
    }

    #[test]
    fn partial_grounding_scores_between() {
        let claims = [
            Claim::fact("a", ClaimKind::ValidationResult, vec!["V1".into()]),
            Claim::fact("b", ClaimKind::Finding, vec!["GHOST".into()]),
        ];
        let (g, _) = verify(&claims, &facts(&[("V1", "MigrationValidationResult")]));
        assert_eq!(g.coverage, 1.0);
        assert_eq!(g.validity, 0.5);
        assert_eq!(g.grounded_rate, 0.5);
        assert_eq!(g.score(), 0.25);
    }

    #[test]
    fn every_claim_kind_declares_what_supports_it() {
        for k in [
            ClaimKind::ValidationResult,
            ClaimKind::Finding,
            ClaimKind::Profile,
            ClaimKind::Design,
            ClaimKind::Approval,
            ClaimKind::UnitState,
        ] {
            assert!(!k.supported_by().is_empty(), "{k:?} supports nothing");
            assert!(k.requires_citation());
        }
        assert!(ClaimKind::Narrative.supported_by().is_empty());
        assert!(!ClaimKind::Narrative.requires_citation());
    }
}
