//! `evaluate_architecture` — RFC 0065 Phase 3 (§32-39, "Evaluation Layer"). An independent,
//! deterministic reviewer over the already-compiled object set: no LLM call here, matching §4.5
//! ("Deterministic Analysis Before LLM Reasoning") — the two dimensions this phase has real signal
//! for (whether every `Crate` got classified, whether every `Claim`/`ArchitectureGap` actually
//! carries evidence) are both plain counts, not judgment calls an LLM would be needed for.
//!
//! Deliberately a plain function, not a `CompilerPass`: it runs *after* `ekos compile`, over the
//! compiled object set, matching §32's "the evaluator should behave as an independent architecture
//! reviewer" — a pipeline stage's own output being marked, not one more thing that output produces.
//!
//! Only two dimensions are computed, not RFC 0065 §34's full list (`consistency`,
//! `cross_view_consistency`, ...): this phase's data has real signal for `completeness` (is every
//! crate classified) and `evidence_coverage` (does every claim/gap actually carry evidence) and
//! nothing else yet — inventing scores for dimensions with no real underlying signal would be
//! exactly the "unsupported precision" §4.6 warns against.

use ekos_kir::{KirId, KirObject, ObjectKind};

/// One evaluation finding — RFC 0065 §34's shape (`type`/`severity`/`description`), restricted to
/// the two issue types this phase can honestly detect.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EvaluationIssue {
    pub issue_type: EvaluationIssueType,
    pub severity: IssueSeverity,
    pub description: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvaluationIssueType {
    /// A `Custom("Crate")` object with no `has_role` `Claim` — RFC 0065 Phase 2 never classified
    /// it (reasoning disabled, LLM call failed/was rejected, or a targeted re-run hasn't reached
    /// it yet).
    MissingClassification,
    /// A `Custom("ArchitectureGap")` object — an explicit, unresolved knowledge gap (RFC 0065
    /// §17). Not an error; still worth surfacing as something the evaluation didn't consider
    /// "done".
    OpenQuestion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueSeverity {
    Medium,
    Low,
}

/// RFC 0065 §34's evaluation result shape, restricted to the dimensions this phase has real
/// signal for.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EvaluationReport {
    /// Weighted average of `completeness`/`evidence_coverage`, in `[0.0, 1.0]`.
    pub score: f32,
    /// Fraction of `Custom("Crate")` objects that have a `has_role` `Claim`.
    pub completeness: f32,
    /// Fraction of `Custom("Claim")`/`Custom("ArchitectureGap")` objects that carry at least one
    /// evidence id — computed for real, not assumed 1.0, even though by this project's own
    /// construction it always should be.
    pub evidence_coverage: f32,
    pub crates_total: usize,
    pub crates_classified: usize,
    pub issues: Vec<EvaluationIssue>,
}

const COMPLETENESS_WEIGHT: f32 = 0.6;
const EVIDENCE_COVERAGE_WEIGHT: f32 = 0.4;

/// The `Custom("Crate")` ids with no corresponding `has_role` `Claim` — the real input to both
/// the evaluator's `missing_classification` issues and `ekos architecture investigate`'s targeted
/// re-collection task list (RFC 0065 §36), so the two never drift out of sync with each other.
pub fn crates_missing_classification(objects: &[KirObject]) -> Vec<&KirObject> {
    let classified: std::collections::HashSet<KirId> = objects
        .iter()
        .filter(|o| o.kind == ObjectKind::Custom("Claim".to_string()))
        .filter(|o| o.properties.get("predicate").and_then(|v| v.as_str()) == Some("has_role"))
        .filter_map(|o| {
            let id_str = o.properties.get("subject_id")?.as_str()?;
            id_str.parse::<KirId>().ok()
        })
        .collect();

    objects
        .iter()
        .filter(|o| o.kind == ObjectKind::Custom("Crate".to_string()))
        .filter(|o| !classified.contains(&o.id))
        .collect()
}

pub fn evaluate_architecture(objects: &[KirObject]) -> EvaluationReport {
    let crates: Vec<&KirObject> = objects
        .iter()
        .filter(|o| o.kind == ObjectKind::Custom("Crate".to_string()))
        .collect();
    let unclassified = crates_missing_classification(objects);
    let crates_total = crates.len();
    let crates_classified = crates_total.saturating_sub(unclassified.len());

    let completeness = if crates_total == 0 {
        1.0
    } else {
        crates_classified as f32 / crates_total as f32
    };

    let evidenced_kinds: Vec<&KirObject> = objects
        .iter()
        .filter(
            |o| matches!(&o.kind, ObjectKind::Custom(k) if k == "Claim" || k == "ArchitectureGap"),
        )
        .collect();
    let evidence_coverage = if evidenced_kinds.is_empty() {
        1.0
    } else {
        let with_evidence = evidenced_kinds
            .iter()
            .filter(|o| !o.evidence.is_empty())
            .count();
        with_evidence as f32 / evidenced_kinds.len() as f32
    };

    let mut issues: Vec<EvaluationIssue> = unclassified
        .iter()
        .map(|c| EvaluationIssue {
            issue_type: EvaluationIssueType::MissingClassification,
            severity: IssueSeverity::Medium,
            description: format!("'{}' has no architectural role classification.", c.name),
        })
        .collect();

    for gap in objects
        .iter()
        .filter(|o| o.kind == ObjectKind::Custom("ArchitectureGap".to_string()))
    {
        let question = gap
            .properties
            .get("question")
            .and_then(|v| v.as_str())
            .unwrap_or(&gap.name);
        issues.push(EvaluationIssue {
            issue_type: EvaluationIssueType::OpenQuestion,
            severity: IssueSeverity::Low,
            description: question.to_string(),
        });
    }

    let score = COMPLETENESS_WEIGHT * completeness + EVIDENCE_COVERAGE_WEIGHT * evidence_coverage;

    EvaluationReport {
        score,
        completeness,
        evidence_coverage,
        crates_total,
        crates_classified,
        issues,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_kir::{KirEvidence, ObjectKind, SourceLocation};

    fn crate_obj(name: &str) -> KirObject {
        KirObject::new(name, ObjectKind::Custom("Crate".to_string()))
    }

    fn role_claim(subject_id: KirId, has_evidence: bool) -> KirObject {
        let mut obj = KirObject::new("x has_role y", ObjectKind::Custom("Claim".to_string()))
            .with_property("subject_id", serde_json::json!(subject_id.to_string()))
            .with_property("predicate", serde_json::json!("has_role"))
            .with_property("value", serde_json::json!("core library"));
        if has_evidence {
            obj = obj.with_evidence(KirId::new());
        }
        obj
    }

    #[test]
    fn all_crates_classified_and_evidenced_scores_1_0() {
        let a = crate_obj("a");
        let claim = role_claim(a.id, true);
        let objects = vec![a, claim];

        let report = evaluate_architecture(&objects);
        assert_eq!(report.completeness, 1.0);
        assert_eq!(report.evidence_coverage, 1.0);
        assert_eq!(report.score, 1.0);
        assert!(report.issues.is_empty());
    }

    #[test]
    fn unclassified_crate_produces_a_missing_classification_issue() {
        let a = crate_obj("a");
        let b = crate_obj("b");
        let claim = role_claim(a.id, true);
        let a_id = a.id;
        let objects = vec![a, b, claim];

        let report = evaluate_architecture(&objects);
        assert_eq!(report.crates_total, 2);
        assert_eq!(report.crates_classified, 1);
        assert_eq!(report.completeness, 0.5);
        assert_eq!(report.issues.len(), 1);
        assert_eq!(
            report.issues[0].issue_type,
            EvaluationIssueType::MissingClassification
        );
        assert!(report.issues[0].description.contains('b'));

        let missing = crates_missing_classification(&objects);
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].name, "b");
        assert_ne!(missing[0].id, a_id);
    }

    #[test]
    fn claim_without_evidence_lowers_evidence_coverage() {
        let a = crate_obj("a");
        let claim = role_claim(a.id, false);
        let objects = vec![a, claim];

        let report = evaluate_architecture(&objects);
        assert_eq!(report.evidence_coverage, 0.0);
        assert!(report.score < 1.0);
    }

    #[test]
    fn architecture_gap_produces_an_open_question_issue() {
        let ev = KirEvidence::new(SourceLocation::file("Cargo.toml"), "unresolved dep");
        let ev_id = ev.id;
        let gap = KirObject::new(
            "unresolved dependency 'foo' for x",
            ObjectKind::Custom("ArchitectureGap".to_string()),
        )
        .with_property("question", serde_json::json!("What does 'foo' resolve to?"))
        .with_evidence(ev_id);
        let objects = vec![gap];

        let report = evaluate_architecture(&objects);
        assert_eq!(report.issues.len(), 1);
        assert_eq!(
            report.issues[0].issue_type,
            EvaluationIssueType::OpenQuestion
        );
        assert_eq!(report.issues[0].description, "What does 'foo' resolve to?");
    }

    #[test]
    fn empty_input_scores_1_0_honestly_not_as_a_failure() {
        // No crates to classify and no claims to lack evidence is not the same as "bad
        // architecture" — an empty workspace scores perfectly rather than being penalized for
        // having nothing to evaluate.
        let report = evaluate_architecture(&[]);
        assert_eq!(report.score, 1.0);
        assert!(report.issues.is_empty());
    }
}
