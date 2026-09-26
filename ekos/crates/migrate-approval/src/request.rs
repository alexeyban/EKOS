//! RFC 0161 — approval requests, pinned to an evidence snapshot.
//!
//! The second failure mode of approval workflows: **approval granted on evidence that then
//! changes.** A lossy mapping is approved when it affects 3 rows; by execution it affects 30,000,
//! and the approval still reads as valid.
//!
//! So a request freezes the fact ids it rested on plus a hash over them, and the hash is recomputed
//! at approval and again at execution. A mismatch does not warn and does not re-validate — the
//! request is **dead**, and a new one must be raised showing the new numbers. "Re-validate" would
//! mean deciding which changes matter, which is the approver's judgement rather than the tool's.

use crate::risk::RiskAssessment;
use serde::{Deserialize, Serialize};

/// One fact the decision rested on, and its content hash at the time.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct EvidenceRef {
    pub fact_id: String,
    pub content_hash: String,
}

/// The frozen evidence behind a request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceSnapshot {
    /// Canonically ordered, so the hash is reproducible across processes (RFC 0135 Part C).
    pub refs: Vec<EvidenceRef>,
    pub hash: String,
}

impl EvidenceSnapshot {
    pub fn of(mut refs: Vec<EvidenceRef>) -> Self {
        refs.sort();
        refs.dedup();
        let hash = hash_refs(&refs);
        Self { refs, hash }
    }

    /// Recompute the hash from the refs as they are *now*.
    ///
    /// Takes the current hashes as a lookup rather than trusting the snapshot, because the snapshot
    /// is exactly what is being verified.
    pub fn still_matches(&self, current: &dyn Fn(&str) -> Option<String>) -> bool {
        let now: Vec<EvidenceRef> = self
            .refs
            .iter()
            .map(|r| EvidenceRef {
                fact_id: r.fact_id.clone(),
                // A fact that has disappeared cannot match; an absent hash is a change.
                content_hash: current(&r.fact_id).unwrap_or_default(),
            })
            .collect();
        hash_refs(&now) == self.hash
    }

    /// Which facts changed, for the message a human reads.
    pub fn changed(&self, current: &dyn Fn(&str) -> Option<String>) -> Vec<String> {
        self.refs
            .iter()
            .filter(|r| current(&r.fact_id).as_deref() != Some(r.content_hash.as_str()))
            .map(|r| r.fact_id.clone())
            .collect()
    }
}

fn hash_refs(refs: &[EvidenceRef]) -> String {
    let joined = refs
        .iter()
        .map(|r| format!("{}={}", r.fact_id, r.content_hash))
        .collect::<Vec<_>>()
        .join("\u{1f}");
    ekos_common::ContentHash::of_str(&joined)
        .as_str()
        .to_string()
}

/// Where a request is in its life.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "status")]
pub enum RequestStatus {
    Pending,
    Approved {
        approvers: Vec<String>,
        at: String,
    },
    Rejected {
        by: String,
        reason: String,
    },
    /// The evidence changed. Terminal: a new request must be raised.
    Dead {
        changed_facts: Vec<String>,
    },
}

/// A request for permission to do one specific thing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub id: String,
    /// What would execute: the artifact ids and their content hashes.
    pub artifacts: Vec<EvidenceRef>,
    pub risk: RiskAssessment,
    pub evidence: EvidenceSnapshot,
    /// An agent session id or a human's OIDC subject. Raising is not approving.
    pub requester: String,
    pub status: RequestStatus,
    /// Recorded when an R3+ request rendered its evidence to a human.
    pub evidence_shown: bool,
    /// The object name an R4 approver typed, verbatim.
    pub typed_confirmation: Option<String>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RequestError {
    #[error("request {id} is not pending (it is {status})")]
    NotPending { id: String, status: String },
    #[error(
        "the evidence behind request {id} has changed ({changed}). The request is dead — raise a \
         new one showing the new numbers. Re-validating would mean deciding which changes matter, \
         which is the approver's judgement, not the tool's."
    )]
    EvidenceChanged { id: String, changed: String },
    #[error(
        "{class} needs {required} distinct approver(s) and has {got}. The same person twice is one \
         approver."
    )]
    NotEnoughApprovers {
        class: &'static str,
        required: usize,
        got: usize,
    },
    #[error("{class} requires the evidence to be rendered and that to be recorded; it was not")]
    EvidenceNotShown { class: &'static str },
    #[error("{class} requires the approver to type {expected:?} to confirm; they typed {got:?}")]
    TypedConfirmationMismatch {
        class: &'static str,
        expected: String,
        got: String,
    },
    #[error(
        "an approver may not be the requester. {who} raised this request, so somebody else has to \
         approve it."
    )]
    SelfApproval { who: String },
}

impl ApprovalRequest {
    /// Approve. Every gate the class demands is checked here, not by the caller.
    ///
    /// `confirm` is what an R4 approver typed; `subject` names the object they had to type.
    pub fn approve(
        &mut self,
        approvers: &[String],
        subject: &str,
        confirm: Option<&str>,
        current_hash: &dyn Fn(&str) -> Option<String>,
        now: &str,
    ) -> Result<(), RequestError> {
        if self.status != RequestStatus::Pending {
            return Err(RequestError::NotPending {
                id: self.id.clone(),
                status: status_label(&self.status).into(),
            });
        }

        // The evidence check comes first: an approval on stale evidence is the failure this whole
        // mechanism exists to prevent, and no other gate matters if it has happened.
        if !self.evidence.still_matches(current_hash) {
            let changed = self.evidence.changed(current_hash);
            self.status = RequestStatus::Dead {
                changed_facts: changed.clone(),
            };
            return Err(RequestError::EvidenceChanged {
                id: self.id.clone(),
                changed: changed.join(", "),
            });
        }

        let mut distinct: Vec<String> = approvers.to_vec();
        distinct.sort();
        distinct.dedup();

        // Compared by **identity**, not by label. The requester is recorded as `cli:alex` or
        // `agent:session-7` and an approver as `human:alex`, so a string comparison lets the same
        // person approve their own request — which is exactly what happened the first time this ran
        // end to end, while the unit tests passed because they happened to use different names.
        let requester = identity_of(&self.requester);
        if let Some(who) = distinct.iter().find(|a| identity_of(a) == requester) {
            return Err(RequestError::SelfApproval { who: who.clone() });
        }

        let required = self.risk.class.approvers_required();
        if distinct.len() < required {
            return Err(RequestError::NotEnoughApprovers {
                class: self.risk.class.as_str(),
                required,
                got: distinct.len(),
            });
        }

        if self.risk.class.requires_evidence_review() && !self.evidence_shown {
            return Err(RequestError::EvidenceNotShown {
                class: self.risk.class.as_str(),
            });
        }

        if self.risk.class.requires_typed_confirmation() {
            let typed = confirm.unwrap_or_default();
            if typed != subject {
                return Err(RequestError::TypedConfirmationMismatch {
                    class: self.risk.class.as_str(),
                    expected: subject.to_string(),
                    got: typed.to_string(),
                });
            }
            self.typed_confirmation = Some(typed.to_string());
        }

        self.status = RequestStatus::Approved {
            approvers: distinct,
            at: now.to_string(),
        };
        Ok(())
    }

    pub fn reject(&mut self, by: &str, reason: &str) -> Result<(), RequestError> {
        if self.status != RequestStatus::Pending {
            return Err(RequestError::NotPending {
                id: self.id.clone(),
                status: status_label(&self.status).into(),
            });
        }
        self.status = RequestStatus::Rejected {
            by: by.to_string(),
            reason: reason.to_string(),
        };
        Ok(())
    }

    /// Whether the artifact named may execute on the strength of this request.
    pub fn authorizes(&self, artifact_id: &str, artifact_hash: &str) -> bool {
        matches!(self.status, RequestStatus::Approved { .. })
            && self
                .artifacts
                .iter()
                .any(|a| a.fact_id == artifact_id && a.content_hash == artifact_hash)
    }
}

/// The identity inside a labelled actor: `human:alex` and `cli:alex` are both `alex`.
///
/// Comparing labels instead of identities is how a self-approval check becomes decoration — the
/// same person raising from the CLI and approving from the console produces two different strings
/// for one human.
pub fn identity_of(label: &str) -> &str {
    label.split_once(':').map_or(label, |(_, rest)| rest)
}

fn status_label(s: &RequestStatus) -> &'static str {
    match s {
        RequestStatus::Pending => "pending",
        RequestStatus::Approved { .. } => "approved",
        RequestStatus::Rejected { .. } => "rejected",
        RequestStatus::Dead { .. } => "dead",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::risk::{ActionFacts, RiskClass, Thresholds, assess};
    use std::collections::BTreeMap;

    fn ev(id: &str, h: &str) -> EvidenceRef {
        EvidenceRef {
            fact_id: id.into(),
            content_hash: h.into(),
        }
    }

    fn world(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect()
    }

    fn request(class_env: (&str, &str), lossiness: Option<&str>) -> ApprovalRequest {
        let risk = assess(
            &ActionFacts {
                statement_class: class_env.0.into(),
                environment: class_env.1.into(),
                lossiness: lossiness.map(str::to_string),
                blast_radius: 0,
                affected_rows: Some(0),
            },
            &Thresholds::default(),
        );
        ApprovalRequest {
            id: "REQ.1".into(),
            artifacts: vec![ev("ART.1", "h-art")],
            risk,
            evidence: EvidenceSnapshot::of(vec![ev("F1", "h1"), ev("F2", "h2")]),
            requester: "agent:session-7".into(),
            status: RequestStatus::Pending,
            evidence_shown: false,
            typed_confirmation: None,
        }
    }

    #[test]
    fn the_snapshot_hash_is_order_independent() {
        let a = EvidenceSnapshot::of(vec![ev("F1", "h1"), ev("F2", "h2")]);
        let b = EvidenceSnapshot::of(vec![ev("F2", "h2"), ev("F1", "h1")]);
        assert_eq!(a.hash, b.hash);
    }

    /// The failure this mechanism exists to prevent: approved at 3 rows, executed at 30,000.
    #[test]
    fn an_approval_on_changed_evidence_kills_the_request() {
        let mut r = request(("dml_insert", "staging"), None);
        let w = world(&[("F1", "h1"), ("F2", "CHANGED")]);
        let err = r
            .approve(
                &["human:alex".into()],
                "public.orders",
                None,
                &|id| w.get(id).cloned(),
                "now",
            )
            .unwrap_err();
        assert!(
            matches!(err, RequestError::EvidenceChanged { .. }),
            "{err:?}"
        );
        assert!(err.to_string().contains("F2"), "{err}");
        assert!(matches!(r.status, RequestStatus::Dead { .. }));
        // And a dead request cannot be revived by a second attempt with correct evidence.
        let good = world(&[("F1", "h1"), ("F2", "h2")]);
        assert!(matches!(
            r.approve(
                &["human:alex".into()],
                "public.orders",
                None,
                &|id| good.get(id).cloned(),
                "now"
            ),
            Err(RequestError::NotPending { .. })
        ));
    }

    /// A fact that disappeared is a change, not an absence of one.
    #[test]
    fn a_vanished_fact_is_a_change() {
        let mut r = request(("dml_insert", "staging"), None);
        let w = world(&[("F1", "h1")]);
        assert!(
            r.approve(
                &["human:alex".into()],
                "s",
                None,
                &|id| w.get(id).cloned(),
                "now"
            )
            .is_err()
        );
    }

    #[test]
    fn a_clean_approval_records_its_approvers() {
        let mut r = request(("dml_insert", "staging"), None);
        let w = world(&[("F1", "h1"), ("F2", "h2")]);
        r.approve(
            &["human:alex".into()],
            "public.orders",
            None,
            &|id| w.get(id).cloned(),
            "t0",
        )
        .unwrap();
        assert_eq!(
            r.status,
            RequestStatus::Approved {
                approvers: vec!["human:alex".into()],
                at: "t0".into()
            }
        );
        assert!(r.authorizes("ART.1", "h-art"));
        assert!(
            !r.authorizes("ART.1", "different-hash"),
            "the artifact must match too"
        );
        assert!(!r.authorizes("ART.OTHER", "h-art"));
    }

    /// The third failure mode, closed structurally: whoever raised the request cannot approve it.
    #[test]
    fn a_requester_cannot_approve_their_own_request() {
        let mut r = request(("dml_insert", "staging"), None);
        let w = world(&[("F1", "h1"), ("F2", "h2")]);
        let err = r
            .approve(
                &["agent:session-7".into()],
                "s",
                None,
                &|id| w.get(id).cloned(),
                "t0",
            )
            .unwrap_err();
        assert!(matches!(err, RequestError::SelfApproval { .. }), "{err:?}");
    }

    /// The hole the first version had, found only when the CLI ran end to end: the requester is
    /// labelled `cli:alex` and the approver `human:alex`, so a string comparison let the same person
    /// approve their own request. The unit test above passed throughout, because it used two names
    /// that differ.
    #[test]
    fn the_same_person_cannot_approve_under_a_different_label() {
        let w = world(&[("F1", "h1"), ("F2", "h2")]);
        for (requester, approver) in [
            ("cli:alex", "human:alex"),
            ("human:alex", "cli:alex"),
            ("agent:alex", "human:alex"),
            ("alex", "human:alex"),
        ] {
            let mut r = request(("dml_insert", "staging"), None);
            r.requester = requester.into();
            let err = r
                .approve(
                    &[approver.into()],
                    "s",
                    None,
                    &|id| w.get(id).cloned(),
                    "t0",
                )
                .unwrap_err();
            assert!(
                matches!(err, RequestError::SelfApproval { .. }),
                "{requester} approving as {approver} must be refused, got {err:?}"
            );
        }

        // And a genuinely different person still can.
        let mut r = request(("dml_insert", "staging"), None);
        r.requester = "cli:alex".into();
        assert!(
            r.approve(
                &["human:sam".into()],
                "s",
                None,
                &|id| w.get(id).cloned(),
                "t0"
            )
            .is_ok()
        );
    }

    #[test]
    fn identity_strips_exactly_one_scheme() {
        assert_eq!(identity_of("human:alex"), "alex");
        assert_eq!(identity_of("cli:alex"), "alex");
        assert_eq!(identity_of("alex"), "alex");
        // An OIDC subject can contain colons; only the leading scheme is stripped.
        assert_eq!(identity_of("human:https://idp/x:y"), "https://idp/x:y");
    }

    #[test]
    fn r4_needs_two_distinct_approvers() {
        let mut r = request(("dml_insert", "production"), None);
        let w = world(&[("F1", "h1"), ("F2", "h2")]);
        r.evidence_shown = true;

        // The same person twice is one approver.
        let err = r
            .approve(
                &["human:alex".into(), "human:alex".into()],
                "public.orders",
                Some("public.orders"),
                &|id| w.get(id).cloned(),
                "t0",
            )
            .unwrap_err();
        assert!(
            matches!(
                err,
                RequestError::NotEnoughApprovers {
                    required: 2,
                    got: 1,
                    ..
                }
            ),
            "{err:?}"
        );

        r.approve(
            &["human:alex".into(), "human:sam".into()],
            "public.orders",
            Some("public.orders"),
            &|id| w.get(id).cloned(),
            "t0",
        )
        .unwrap();
        assert!(matches!(r.status, RequestStatus::Approved { .. }));
    }

    #[test]
    fn r3_refuses_until_the_evidence_was_actually_shown() {
        let mut r = request(("dml_insert", "staging"), Some("lossy"));
        assert_eq!(r.risk.class, RiskClass::R3);
        let w = world(&[("F1", "h1"), ("F2", "h2")]);
        assert!(matches!(
            r.approve(
                &["human:alex".into()],
                "s",
                None,
                &|id| w.get(id).cloned(),
                "t0"
            ),
            Err(RequestError::EvidenceNotShown { .. })
        ));
        r.evidence_shown = true;
        assert!(
            r.approve(
                &["human:alex".into()],
                "s",
                None,
                &|id| w.get(id).cloned(),
                "t0"
            )
            .is_ok()
        );
    }

    /// A typed confirmation is not a checkbox: the approver types the object's name or it fails.
    #[test]
    fn r4_requires_the_object_name_typed_exactly() {
        let mut r = request(("destructive", "production"), None);
        r.evidence_shown = true;
        let w = world(&[("F1", "h1"), ("F2", "h2")]);
        let approvers = ["human:alex".to_string(), "human:sam".to_string()];

        for wrong in [None, Some(""), Some("yes"), Some("public.order")] {
            let mut attempt = r.clone();
            let err = attempt
                .approve(
                    &approvers,
                    "public.orders",
                    wrong,
                    &|id| w.get(id).cloned(),
                    "t0",
                )
                .unwrap_err();
            assert!(
                matches!(err, RequestError::TypedConfirmationMismatch { .. }),
                "{wrong:?} should not confirm: {err:?}"
            );
        }
        r.approve(
            &approvers,
            "public.orders",
            Some("public.orders"),
            &|id| w.get(id).cloned(),
            "t0",
        )
        .unwrap();
        assert_eq!(r.typed_confirmation.as_deref(), Some("public.orders"));
    }

    #[test]
    fn a_rejection_is_terminal_and_records_its_reason() {
        let mut r = request(("dml_insert", "staging"), None);
        r.reject("human:alex", "wait for the quarter to close")
            .unwrap();
        assert_eq!(
            r.status,
            RequestStatus::Rejected {
                by: "human:alex".into(),
                reason: "wait for the quarter to close".into()
            }
        );
        assert!(!r.authorizes("ART.1", "h-art"));
        assert!(r.reject("human:sam", "again").is_err());
    }

    #[test]
    fn an_r1_action_needs_no_approver_but_still_checks_its_evidence() {
        let mut r = request(("dml_insert", "sandbox"), None);
        assert_eq!(r.risk.class, RiskClass::R1);
        let stale = world(&[("F1", "CHANGED"), ("F2", "h2")]);
        assert!(
            r.approve(&[], "s", None, &|id| stale.get(id).cloned(), "t0")
                .is_err()
        );

        let mut fresh = request(("dml_insert", "sandbox"), None);
        let w = world(&[("F1", "h1"), ("F2", "h2")]);
        fresh
            .approve(&[], "s", None, &|id| w.get(id).cloned(), "t0")
            .unwrap();
        assert!(matches!(fresh.status, RequestStatus::Approved { .. }));
    }
}
