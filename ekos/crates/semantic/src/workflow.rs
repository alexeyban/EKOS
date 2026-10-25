//! The seven-step operator workflow — RFC 0213 §3.5 (the step bar) and the sign-off gate.
//!
//! Pure: the CLI supplies step 1 (from the committed manifest and freshness), step 3 (from the
//! review subjects' sign-off states) and whatever an extension reports for steps 2 and 4–7;
//! [`assemble`] fills the gaps and applies the prerequisites. [`gate`] is what a step 4–7 command
//! calls before it writes anything.

use crate::review::{SignState, Subject};
use serde::{Deserialize, Serialize};

/// A step's state on the bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepState {
    Todo,
    Partial,
    Done,
    /// Was done, and what it was done on has changed since.
    Stale,
    /// A prerequisite is not done.
    Blocked,
}

/// One review subject on the bar: its counts and where its sign-off stands.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubjectStatus {
    pub subject: String,
    pub items: usize,
    pub decided: usize,
    pub outdated: usize,
    pub blocking: usize,
    pub digest: String,
    pub signoff: SignState,
}

/// One step of the bar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Step {
    pub n: u8,
    pub id: String,
    pub title: String,
    pub state: StepState,
    pub detail: String,
    /// Step 3 only.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subjects: Vec<SubjectStatus>,
}

impl Step {
    pub fn new(n: u8, state: StepState, detail: impl Into<String>) -> Self {
        let (id, title) = step_name(n);
        Self {
            n,
            id: id.into(),
            title: title.into(),
            state,
            detail: detail.into(),
            subjects: Vec::new(),
        }
    }
}

/// The seven steps: number, id, title (the plan's operator workflow).
pub const STEPS: [(u8, &str, &str); 7] = [
    (1, "analyse", "Analyse sources"),
    (2, "docs", "Documentation"),
    (3, "review", "Review and sign-off"),
    (4, "requirements", "Requirements"),
    (5, "extraction", "Extraction and landing"),
    (6, "build", "Bronze and silver"),
    (7, "marts", "Marts"),
];

/// The steps an extension may report; 1 and 3 are always computed here.
pub const EXTENSION_STEPS: [u8; 5] = [2, 4, 5, 6, 7];

fn step_name(n: u8) -> (&'static str, &'static str) {
    STEPS
        .iter()
        .find(|(m, _, _)| *m == n)
        .map_or(("unknown", "Unknown step"), |(_, id, title)| (*id, *title))
}

/// Step 3 from the subjects' sign-offs: `done` when every subject is validly signed, `stale` when
/// any signed one went stale, `partial` when some are signed, `todo` when none are.
pub fn review_step(subjects: Vec<SubjectStatus>) -> Step {
    let total = subjects.len();
    let valid = count(&subjects, "valid");
    let stale = count(&subjects, "stale");
    let blocking: usize = subjects.iter().map(|s| s.blocking).sum();
    let state = if total > 0 && valid == total {
        StepState::Done
    } else if stale > 0 {
        StepState::Stale
    } else if valid > 0 {
        StepState::Partial
    } else {
        StepState::Todo
    };
    let mut detail = format!("{valid} of {total} subjects signed");
    if stale > 0 {
        detail.push_str(&format!(", {stale} stale"));
    }
    if blocking > 0 {
        detail.push_str(&format!(", {blocking} blocking item(s)"));
    }
    let mut step = Step::new(3, state, detail);
    step.subjects = subjects;
    step
}

fn count(subjects: &[SubjectStatus], state: &str) -> usize {
    subjects
        .iter()
        .filter(|s| s.signoff.name() == state)
        .count()
}

/// The whole bar. `extension` steps fill 2 and 4–7 (any other number is ignored; the first
/// report of a number wins); a missing one is `todo` with "needs ekos-lakehouse". Then the
/// prerequisites: every later step is `blocked` while step 1 is `todo`, and steps 4–7 are
/// `blocked` while step 3 is not `done` — whatever the extension said.
pub fn assemble(analyse: Step, review: Step, extension: Vec<Step>) -> Vec<Step> {
    let review_done = review.state == StepState::Done;
    let analysed = analyse.state != StepState::Todo;
    let mut steps = vec![analyse];
    for n in 2..=7u8 {
        let step = if n == 3 {
            review.clone()
        } else {
            extension
                .iter()
                .find(|s| s.n == n && EXTENSION_STEPS.contains(&n))
                .cloned()
                .map(|mut s| {
                    let (id, title) = step_name(n);
                    s.id = id.into();
                    if s.title.is_empty() {
                        s.title = title.into();
                    }
                    s
                })
                .unwrap_or_else(|| Step::new(n, StepState::Todo, "needs ekos-lakehouse"))
        };
        steps.push(step);
    }
    for s in steps.iter_mut().skip(1) {
        if !analysed {
            block(s, "analyse the sources first (step 1)");
        } else if s.n >= 4 && !review_done {
            block(s, "needs a valid sign-off of every review subject (step 3)");
        }
    }
    steps
}

fn block(s: &mut Step, why: &str) {
    s.state = StepState::Blocked;
    s.detail = if s.detail.is_empty() {
        why.into()
    } else {
        format!("{why}; {}", s.detail)
    };
}

/// RFC 0213 §3.5: what a step 4–7 command checks before it writes anything. `Ok` only when every
/// subject in `required` has a valid sign-off; otherwise the error names each one that has not,
/// with its state. A subject missing from `subjects` counts as never signed.
pub fn gate(subjects: &[SubjectStatus], required: &[Subject]) -> Result<(), String> {
    let missing: Vec<String> = required
        .iter()
        .filter_map(|r| {
            let state = subjects
                .iter()
                .find(|s| s.subject == r.as_str())
                .map_or("none", |s| s.signoff.name());
            (state != "valid").then(|| {
                let why = match state {
                    "none" => "not signed off",
                    "stale" => "sign-off is stale (its facts changed)",
                    "revoked" => "sign-off was revoked",
                    _ => state,
                };
                format!("{}: {why}", r.as_str())
            })
        })
        .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "the source model is not signed off — {}. Review with `ekos review show <subject>`, then `ekos signoff approve`",
            missing.join("; ")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn subject(name: &str, signoff: SignState) -> SubjectStatus {
        SubjectStatus {
            subject: name.into(),
            items: 1,
            decided: 0,
            outdated: 0,
            blocking: 0,
            digest: "d".into(),
            signoff,
        }
    }

    fn valid() -> SignState {
        SignState::Valid {
            signed_by: "ann".into(),
            signed_at: "t".into(),
        }
    }

    fn stale() -> SignState {
        SignState::Stale {
            signed_by: "ann".into(),
            signed_at: "t".into(),
            added: vec!["x".into()],
            removed: vec![],
            changed: vec![],
        }
    }

    fn all(state: impl Fn() -> SignState) -> Vec<SubjectStatus> {
        Subject::ALL
            .iter()
            .map(|s| subject(s.as_str(), state()))
            .collect()
    }

    #[test]
    fn review_step_states() {
        assert_eq!(review_step(all(valid)).state, StepState::Done);
        assert_eq!(review_step(all(|| SignState::None)).state, StepState::Todo);
        let mut some = all(|| SignState::None);
        some[0].signoff = valid();
        let step = review_step(some.clone());
        assert_eq!(step.state, StepState::Partial);
        assert_eq!(step.detail, "1 of 6 subjects signed");
        some[1].signoff = stale();
        some[2].blocking = 4;
        let step = review_step(some);
        assert_eq!(step.state, StepState::Stale);
        assert_eq!(
            step.detail,
            "1 of 6 subjects signed, 1 stale, 4 blocking item(s)"
        );
        assert_eq!(review_step(Vec::new()).state, StepState::Todo);
    }

    #[test]
    fn gate_names_every_subject_that_is_not_validly_signed() {
        assert_eq!(gate(&all(valid), &Subject::ALL), Ok(()));
        let mut s = all(valid);
        s[1].signoff = stale();
        s.retain(|x| x.subject != "glossary");
        let err = gate(&s, &Subject::ALL).unwrap_err();
        assert!(err.contains("pii: sign-off is stale"), "{err}");
        assert!(err.contains("glossary: not signed off"), "{err}");
        assert!(!err.contains("scope:"), "{err}");
        // Only the required subjects count.
        assert_eq!(gate(&s, &[Subject::Scope]), Ok(()));
    }

    #[test]
    fn assemble_fills_gaps_and_blocks_on_prerequisites() {
        let analysed = Step::new(1, StepState::Done, "committed");
        let ext = vec![
            Step::new(4, StepState::Done, "2 specs"),
            Step::new(2, StepState::Partial, "generated"),
            // Step 3 is never the extension's.
            Step::new(3, StepState::Done, "ext"),
        ];
        let bar = assemble(analysed.clone(), review_step(all(valid)), ext.clone());
        assert_eq!(
            bar.iter().map(|s| s.n).collect::<Vec<_>>(),
            [1, 2, 3, 4, 5, 6, 7]
        );
        assert_eq!(bar[1].state, StepState::Partial);
        assert_eq!(bar[2].detail, "6 of 6 subjects signed");
        assert_eq!(bar[3].state, StepState::Done);
        assert_eq!(bar[4].detail, "needs ekos-lakehouse");
        assert_eq!(bar[6].id, "marts");

        let bar = assemble(analysed, review_step(all(stale)), ext.clone());
        assert_eq!(bar[1].state, StepState::Partial, "docs need only step 1");
        for s in &bar[3..] {
            assert_eq!(s.state, StepState::Blocked, "step {}", s.n);
        }
        assert!(bar[3].detail.ends_with("; 2 specs"), "{}", bar[3].detail);

        let bar = assemble(
            Step::new(1, StepState::Todo, "nothing committed"),
            review_step(all(valid)),
            ext,
        );
        assert!(bar[1..].iter().all(|s| s.state == StepState::Blocked));
    }

    #[test]
    fn the_bar_round_trips_through_json() {
        let bar = assemble(
            Step::new(1, StepState::Done, "x"),
            review_step(all(stale)),
            vec![],
        );
        let back: Vec<Step> = serde_json::from_value(serde_json::to_value(&bar).unwrap()).unwrap();
        assert_eq!(back, bar);
    }
}
