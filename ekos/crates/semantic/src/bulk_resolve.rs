//! RFC 0247 §3.5 — deciding conflicts **by rule**.
//!
//! Step 3 of the operator workflow can leave hundreds of conflicts that share one explanation
//! ("the database is the authority for nullability"). Deciding each by hand records the same
//! reasoning 195 times; deciding them with a rule records it once, as a reviewable object, and
//! still produces one ordinary [`Resolution`] per conflict.
//!
//! The planner is pure: it reads conflicts and says what each decision *would* be, and why a
//! conflict was left alone. Nothing here writes. The guard rails are the point:
//!
//! - a rule needs a **rationale** (it becomes every decision's note) and at least one **match
//!   criterion** — there is no match-everything rule;
//! - only **open** conflicts are touched;
//! - `pick_source` picks a claim only when **exactly one** claim comes from that source; zero or
//!   several are left for a person, with the reason, never guessed;
//! - the number of decisions is capped (`max`), so a too-wide rule is refused, not applied.

use crate::conflicts::{Resolution, is_conflict};
use ekos_kir::KirObject;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Default cap on decisions one rule may make.
pub const DEFAULT_MAX: usize = 500;

/// A conflict matcher. Every set field must match (AND); at least one must be set.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Match {
    /// Exactly this `conflict_type` (`mandatory_but_nullable`, `table_without_model`, …).
    #[serde(default)]
    pub conflict_type: Option<String>,
    /// The subject's name contains this (case-insensitive).
    #[serde(default)]
    pub subject_contains: Option<String>,
    /// Exactly this attribute.
    #[serde(default)]
    pub attribute: Option<String>,
}

impl Match {
    fn is_empty(&self) -> bool {
        self.conflict_type.is_none() && self.subject_contains.is_none() && self.attribute.is_none()
    }

    fn matches(&self, c: &KirObject) -> bool {
        let text = |k: &str| c.properties.get(k).and_then(Value::as_str).unwrap_or("");
        self.conflict_type
            .as_deref()
            .is_none_or(|t| text("conflict_type") == t)
            && self
                .attribute
                .as_deref()
                .is_none_or(|a| text("attribute") == a)
            && self.subject_contains.as_deref().is_none_or(|s| {
                text("subject_name")
                    .to_lowercase()
                    .contains(&s.to_lowercase())
            })
    }
}

/// What a rule decides.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    /// The claim from the source whose `source` contains this text is right (case-insensitive).
    PickSource(String),
    /// Both claims are valid (the rationale is the note).
    BothValid,
}

/// A rule for deciding conflicts in bulk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BulkRule {
    pub name: String,
    pub rationale: String,
    #[serde(rename = "match")]
    pub matcher: Match,
    pub decision: Decision,
    #[serde(default)]
    pub max: Option<usize>,
}

impl BulkRule {
    /// Why this rule cannot be used, if it cannot.
    pub fn check(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("a rule needs a name".into());
        }
        if self.rationale.trim().len() < 10 {
            return Err(
                "a rule needs a rationale (at least a sentence): it is every decision's note"
                    .into(),
            );
        }
        if self.matcher.is_empty() {
            return Err("a rule must narrow what it matches: set conflict_type, subject_contains or attribute".into());
        }
        if matches!(&self.decision, Decision::PickSource(s) if s.trim().is_empty()) {
            return Err("pick_source needs the source to prefer".into());
        }
        if self.max == Some(0) {
            return Err("max 0 decides nothing".into());
        }
        Ok(())
    }

    /// The text recorded as the rule's instruction annotation.
    pub fn describe(&self, decided: usize) -> String {
        format!(
            "Bulk conflict rule `{}`: {} (decided {decided} conflict(s))",
            self.name,
            self.rationale.trim()
        )
    }
}

/// One conflict the rule would decide.
#[derive(Debug, Clone)]
pub struct Planned {
    pub conflict: KirObject,
    pub resolution: Resolution,
}

/// A matched conflict the rule leaves to a person, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    pub name: String,
    pub reason: String,
}

/// What applying a rule would do.
#[derive(Debug, Clone, Default)]
pub struct Plan {
    pub decide: Vec<Planned>,
    pub skipped: Vec<Skipped>,
    /// Open conflicts that matched in the first place.
    pub matched: usize,
}

/// Plan a rule over the current conflicts. Deterministic: the result does not depend on the order
/// `conflicts` arrives in. Refuses when more than `max` conflicts would be decided.
pub fn plan(rule: &BulkRule, conflicts: &[KirObject]) -> Result<Plan, String> {
    rule.check()?;
    let mut open: Vec<&KirObject> = conflicts
        .iter()
        .filter(|c| is_conflict(c))
        .filter(|c| {
            c.properties.get("status").and_then(Value::as_str) == Some(crate::conflicts::OPEN)
        })
        .filter(|c| rule.matcher.matches(c))
        .collect();
    open.sort_by(|a, b| (&a.name, a.id.0).cmp(&(&b.name, b.id.0)));
    let mut plan = Plan {
        matched: open.len(),
        ..Plan::default()
    };
    for c in open {
        match decide_one(rule, c) {
            Ok(resolution) => plan.decide.push(Planned {
                conflict: c.clone(),
                resolution,
            }),
            Err(reason) => plan.skipped.push(Skipped {
                name: c.name.clone(),
                reason,
            }),
        }
    }
    let cap = rule.max.unwrap_or(DEFAULT_MAX);
    if plan.decide.len() > cap {
        return Err(format!(
            "the rule would decide {} conflicts, more than max {cap}: narrow it or raise max",
            plan.decide.len()
        ));
    }
    Ok(plan)
}

fn decide_one(rule: &BulkRule, c: &KirObject) -> Result<Resolution, String> {
    match &rule.decision {
        Decision::BothValid => Ok(Resolution::BothValid),
        Decision::PickSource(want) => {
            let want = want.to_lowercase();
            let claims = c
                .properties
                .get("claims")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let hits: Vec<usize> = claims
                .iter()
                .enumerate()
                .filter(|(_, claim)| {
                    claim
                        .get("source")
                        .and_then(Value::as_str)
                        .is_some_and(|s| s.to_lowercase().contains(&want))
                })
                .map(|(i, _)| i + 1)
                .collect();
            match hits.as_slice() {
                [one] => Ok(Resolution::Pick(*one)),
                [] => Err(format!("no claim comes from a source containing `{want}`")),
                many => Err(format!(
                    "{} claims come from a source containing `{want}`",
                    many.len()
                )),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conflicts::{DISMISSED, OPEN, RESOLVED};
    use ekos_kir::ObjectKind;
    use serde_json::json;

    fn conflict(name: &str, ctype: &str, status: &str, sources: &[&str]) -> KirObject {
        let mut o = KirObject::new(name, ObjectKind::Custom("ConflictingEvidence".into()));
        o.properties.insert("subject_name".into(), json!(name));
        o.properties.insert("attribute".into(), json!("nullable"));
        o.properties.insert("conflict_type".into(), json!(ctype));
        o.properties.insert("status".into(), json!(status));
        o.properties.insert(
            "claims".into(),
            json!(
                sources
                    .iter()
                    .map(|s| json!({"value": "x", "path": "p", "line": 1, "source": s}))
                    .collect::<Vec<_>>()
            ),
        );
        o
    }

    fn rule(decision: Decision) -> BulkRule {
        BulkRule {
            name: "db-is-authority".into(),
            rationale: "The database is the authority for nullability".into(),
            matcher: Match {
                conflict_type: Some("mandatory_but_nullable".into()),
                ..Match::default()
            },
            decision,
            max: None,
        }
    }

    #[test]
    fn a_rule_without_a_rationale_or_a_matcher_is_refused() {
        let mut r = rule(Decision::BothValid);
        r.rationale = "ok".into();
        assert!(r.check().unwrap_err().contains("rationale"));
        let mut r = rule(Decision::BothValid);
        r.matcher = Match::default();
        assert!(
            r.check().unwrap_err().contains("narrow"),
            "there is no match-everything rule"
        );
        let mut r = rule(Decision::PickSource(" ".into()));
        assert!(r.check().is_err());
        r.decision = Decision::BothValid;
        r.max = Some(0);
        assert!(r.check().is_err());
        r.name = " ".into();
        r.max = None;
        assert!(r.check().is_err());
    }

    #[test]
    fn unknown_fields_in_a_rule_file_are_refused() {
        let bad = r#"{"name":"n","rationale":"a long enough reason","match":{"conflict_type":"x","typo":1},"decision":{"pick_source":"db"}}"#;
        assert!(serde_json::from_str::<BulkRule>(bad).is_err());
        let ok = r#"{"name":"n","rationale":"a long enough reason","match":{"conflict_type":"x"},"decision":{"pick_source":"db"}}"#;
        assert!(serde_json::from_str::<BulkRule>(ok).is_ok());
    }

    #[test]
    fn it_decides_only_open_matching_conflicts() {
        let cs = vec![
            conflict("a", "mandatory_but_nullable", OPEN, &["code", "db/oracle"]),
            conflict(
                "b",
                "mandatory_but_nullable",
                RESOLVED,
                &["code", "db/oracle"],
            ),
            conflict(
                "c",
                "mandatory_but_nullable",
                DISMISSED,
                &["code", "db/oracle"],
            ),
            conflict("d", "table_without_model", OPEN, &["code", "db/oracle"]),
        ];
        let p = plan(&rule(Decision::PickSource("db".into())), &cs).unwrap();
        assert_eq!((p.matched, p.decide.len(), p.skipped.len()), (1, 1, 0));
        assert_eq!(p.decide[0].conflict.name, "a");
        assert_eq!(p.decide[0].resolution, Resolution::Pick(2));
    }

    #[test]
    fn an_ambiguous_or_missing_source_is_left_to_a_person_not_guessed() {
        let cs = vec![
            conflict("none", "mandatory_but_nullable", OPEN, &["code", "docs"]),
            conflict("both", "mandatory_but_nullable", OPEN, &["db/a", "db/b"]),
            conflict("one", "mandatory_but_nullable", OPEN, &["docs", "db/a"]),
        ];
        let p = plan(&rule(Decision::PickSource("db".into())), &cs).unwrap();
        assert_eq!(p.decide.len(), 1);
        assert_eq!(p.decide[0].conflict.name, "one");
        let reasons: Vec<_> = p
            .skipped
            .iter()
            .map(|s| (s.name.as_str(), s.reason.as_str()))
            .collect();
        assert_eq!(reasons.len(), 2);
        assert!(
            reasons
                .iter()
                .any(|(n, r)| *n == "none" && r.contains("no claim"))
        );
        assert!(
            reasons
                .iter()
                .any(|(n, r)| *n == "both" && r.contains("2 claims"))
        );
    }

    #[test]
    fn both_valid_decides_every_match() {
        let cs = vec![
            conflict("a", "mandatory_but_nullable", OPEN, &["x", "y"]),
            conflict("b", "mandatory_but_nullable", OPEN, &["x", "y"]),
        ];
        let p = plan(&rule(Decision::BothValid), &cs).unwrap();
        assert!(
            p.decide
                .iter()
                .all(|d| d.resolution == Resolution::BothValid)
        );
        assert_eq!(p.decide.len(), 2);
    }

    #[test]
    fn matcher_fields_combine_with_and() {
        let cs = vec![
            conflict("ORDERS", "mandatory_but_nullable", OPEN, &["x", "y"]),
            conflict("INVOICES", "mandatory_but_nullable", OPEN, &["x", "y"]),
        ];
        let mut r = rule(Decision::BothValid);
        r.matcher.subject_contains = Some("order".into());
        assert_eq!(
            plan(&r, &cs).unwrap().decide.len(),
            1,
            "case-insensitive, AND with the type"
        );
        r.matcher.attribute = Some("other".into());
        assert_eq!(plan(&r, &cs).unwrap().matched, 0);
    }

    #[test]
    fn a_rule_that_is_too_wide_is_refused_not_applied() {
        let cs: Vec<_> = (0..5)
            .map(|i| {
                conflict(
                    &format!("t{i}"),
                    "mandatory_but_nullable",
                    OPEN,
                    &["x", "y"],
                )
            })
            .collect();
        let mut r = rule(Decision::BothValid);
        r.max = Some(4);
        assert!(plan(&r, &cs).unwrap_err().contains("more than max 4"));
        r.max = Some(5);
        assert_eq!(plan(&r, &cs).unwrap().decide.len(), 5);
    }

    #[test]
    fn the_plan_does_not_depend_on_input_order() {
        let cs: Vec<_> = ["b", "a", "c"]
            .iter()
            .map(|n| conflict(n, "mandatory_but_nullable", OPEN, &["code", "db"]))
            .collect();
        let mut rev = cs.clone();
        rev.reverse();
        let names = |p: Plan| {
            p.decide
                .into_iter()
                .map(|d| d.conflict.name)
                .collect::<Vec<_>>()
        };
        let r = rule(Decision::PickSource("db".into()));
        assert_eq!(
            names(plan(&r, &cs).unwrap()),
            names(plan(&r, &rev).unwrap())
        );
        assert_eq!(names(plan(&r, &cs).unwrap()), vec!["a", "b", "c"]);
    }

    #[test]
    fn foreign_objects_are_ignored() {
        let other = KirObject::new("t", ObjectKind::Custom("Ticket".into()));
        let p = plan(&rule(Decision::BothValid), &[other]).unwrap();
        assert_eq!(p.matched, 0);
    }
}
