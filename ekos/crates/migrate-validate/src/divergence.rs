//! RFC 0156 — divergence classification.
//!
//! Every divergence lands in exactly one class, and only one of them blocks. The classes are not
//! severity labels: `expected` and `explained` each require a *fact* to point at, and "the
//! developer looked at it and it seemed fine" is not a class.

/// Why a difference between source and target is, or is not, acceptable.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case", tag = "class")]
pub enum Class {
    /// Matches an approved `MigrationDisposition`. Carries its id so the report can cite it.
    Expected { disposition: String },
    /// Matches a deterministic rule that proves why the difference exists.
    Explained { rule: String },
    /// Everything else. Blocks the unit.
    Unexplained,
}

impl Class {
    /// The only question the state machine asks.
    pub fn blocks(&self) -> bool {
        matches!(self, Self::Unexplained)
    }
}

/// One divergence, at whatever granularity the tier that found it works in.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Divergence {
    /// Bucket, or a masked key reference once bisect has narrowed it (RFC 0156 §bisect).
    pub locus: String,
    /// Column names only — never values. Values are compared in memory and discarded.
    pub columns: Vec<String>,
    pub detail: String,
    pub class: Class,
}

/// A rule that can promote an `Unexplained` divergence to `Explained`, deterministically.
///
/// Deliberately not a free-text annotation: a rule is a named, reusable predicate, so "this is
/// fine" can be reviewed once rather than re-argued per table.
pub trait ExplanationRule {
    fn id(&self) -> &str;
    fn explains(&self, d: &Divergence) -> bool;
}

/// Apply the approved dispositions and the explanation rules, in that order, and report what is
/// left.
///
/// Order matters: a human disposition outranks a rule, because the human may have decided
/// something the rule does not model.
pub fn classify(
    mut divergences: Vec<Divergence>,
    dispositions: &[(String, String)], // (locus, disposition id)
    rules: &[&dyn ExplanationRule],
) -> Vec<Divergence> {
    for d in &mut divergences {
        if !d.class.blocks() {
            continue;
        }
        if let Some((_, id)) = dispositions.iter().find(|(locus, _)| *locus == d.locus) {
            d.class = Class::Expected {
                disposition: id.clone(),
            };
            continue;
        }
        if let Some(rule) = rules.iter().find(|r| r.explains(d)) {
            d.class = Class::Explained {
                rule: rule.id().to_string(),
            };
        }
    }
    divergences
}

/// How many divergences block. The single number a unit's state depends on.
pub fn blocking_count(divergences: &[Divergence]) -> usize {
    divergences.iter().filter(|d| d.class.blocks()).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(locus: &str) -> Divergence {
        Divergence {
            locus: locus.into(),
            columns: vec!["amount".into()],
            detail: "differs".into(),
            class: Class::Unexplained,
        }
    }

    struct TzRule;
    impl ExplanationRule for TzRule {
        fn id(&self) -> &str {
            "EXPL.TZ.001"
        }
        fn explains(&self, x: &Divergence) -> bool {
            x.detail.contains("timezone")
        }
    }

    #[test]
    fn only_unexplained_blocks() {
        assert!(Class::Unexplained.blocks());
        assert!(
            !Class::Expected {
                disposition: "x".into()
            }
            .blocks()
        );
        assert!(!Class::Explained { rule: "y".into() }.blocks());
    }

    #[test]
    fn a_disposition_outranks_a_rule() {
        let mut x = d("bucket:7");
        x.detail = "timezone shift".into();
        let out = classify(
            vec![x],
            &[("bucket:7".into(), "DISP.42".into())],
            &[&TzRule],
        );
        assert_eq!(
            out[0].class,
            Class::Expected {
                disposition: "DISP.42".into()
            },
            "a human decision must not be overwritten by a rule that happens to match"
        );
    }

    #[test]
    fn a_rule_explains_what_no_disposition_covers() {
        let mut x = d("bucket:7");
        x.detail = "timezone shift".into();
        let out = classify(vec![x], &[], &[&TzRule]);
        assert_eq!(
            out[0].class,
            Class::Explained {
                rule: "EXPL.TZ.001".into()
            }
        );
        assert_eq!(blocking_count(&out), 0);
    }

    #[test]
    fn anything_unmatched_still_blocks() {
        let out = classify(vec![d("bucket:9")], &[], &[&TzRule]);
        assert_eq!(blocking_count(&out), 1);
    }

    /// An already-classified divergence is never reclassified: re-running the classifier must not
    /// quietly upgrade or downgrade a decision that is already recorded.
    #[test]
    fn classification_is_idempotent() {
        let out = classify(vec![d("b")], &[("b".into(), "D1".into())], &[]);
        let again = classify(out.clone(), &[("b".into(), "D2".into())], &[]);
        assert_eq!(
            again[0].class,
            Class::Expected {
                disposition: "D1".into()
            }
        );
    }
}
