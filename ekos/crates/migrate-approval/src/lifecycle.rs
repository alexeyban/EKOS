//! RFC 0161 — the human-only decision path.
//!
//! **Nothing in `commands/mcp.rs` may reference this module.** A source-scanning test in the CLI
//! enforces it, exactly as RFC 0151's `session::lifecycle` is enforced, and for the same reason: an
//! allowlist in a subagent's prompt is not a control, and a correct permission check is only
//! probable where an absent capability is verifiable.
//!
//! The [`Actor`] enum has no `Agent` variant. That is not an oversight — an absent variant cannot be
//! constructed by a future caller who has not read this RFC.

use crate::request::{ApprovalRequest, RequestError};

/// The only actor that may approve, reject or sign off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Actor {
    Human,
}

impl Actor {
    /// The identity recorded on the fact — an OIDC subject from the console, or the local user from
    /// the CLI.
    pub fn label(self, subject: &str) -> String {
        match self {
            Self::Human => format!("human:{subject}"),
        }
    }
}

/// Approve a request. The only way to reach [`ApprovalRequest::approve`].
#[allow(clippy::too_many_arguments)]
pub fn approve(
    request: &mut ApprovalRequest,
    actor: Actor,
    subjects: &[String],
    object_name: &str,
    typed: Option<&str>,
    current_hash: &dyn Fn(&str) -> Option<String>,
    now: &str,
) -> Result<(), RequestError> {
    let approvers: Vec<String> = subjects.iter().map(|s| actor.label(s)).collect();
    request.approve(&approvers, object_name, typed, current_hash, now)
}

/// Reject a request.
pub fn reject(
    request: &mut ApprovalRequest,
    actor: Actor,
    subject: &str,
    reason: &str,
) -> Result<(), RequestError> {
    request.reject(&actor.label(subject), reason)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request::{EvidenceRef, EvidenceSnapshot, RequestStatus};
    use crate::risk::{ActionFacts, Thresholds, assess};

    fn pending() -> ApprovalRequest {
        ApprovalRequest {
            id: "REQ.1".into(),
            artifacts: vec![],
            risk: assess(
                &ActionFacts {
                    statement_class: "dml_insert".into(),
                    environment: "staging".into(),
                    lossiness: None,
                    blast_radius: 0,
                    affected_rows: Some(0),
                },
                &Thresholds::default(),
            ),
            evidence: EvidenceSnapshot::of(vec![EvidenceRef {
                fact_id: "F1".into(),
                content_hash: "h1".into(),
            }]),
            requester: "agent:session-7".into(),
            status: RequestStatus::Pending,
            evidence_shown: false,
            typed_confirmation: None,
        }
    }

    /// The exact CLI path that approved its own R3 request in the LedgerSMB demo: raised as
    /// `cli:legion`, approved with `--as cli:legion`, which `approve` labels `human:cli:legion`.
    #[test]
    fn a_requester_cannot_self_approve_by_typing_their_own_label() {
        let mut r = pending();
        r.requester = "cli:legion".into();
        let err = approve(
            &mut r,
            Actor::Human,
            &["cli:legion".into()],
            "s",
            None,
            &|_| Some("h1".into()),
            "t0",
        )
        .unwrap_err();
        assert!(
            matches!(err, crate::request::RequestError::SelfApproval { .. }),
            "{err:?}"
        );
        assert!(matches!(r.status, RequestStatus::Pending), "still pending");
    }

    #[test]
    fn an_approval_is_labelled_as_human() {
        let mut r = pending();
        approve(
            &mut r,
            Actor::Human,
            &["alex".into()],
            "public.orders",
            None,
            &|_| Some("h1".into()),
            "t0",
        )
        .unwrap();
        match r.status {
            RequestStatus::Approved { approvers, .. } => {
                assert_eq!(approvers, vec!["human:alex"]);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_rejection_is_labelled_as_human() {
        let mut r = pending();
        reject(&mut r, Actor::Human, "alex", "not yet").unwrap();
        assert_eq!(
            r.status,
            RequestStatus::Rejected {
                by: "human:alex".into(),
                reason: "not yet".into()
            }
        );
    }

    /// The structural guarantee. If this enum ever grows a second variant, the test fails and
    /// whoever added it has to explain themselves in a review.
    #[test]
    fn the_actor_enum_has_exactly_one_variant() {
        let src = include_str!("lifecycle.rs");
        let body = src
            .split("pub enum Actor {")
            .nth(1)
            .expect("Actor enum")
            .split('}')
            .next()
            .unwrap();
        assert_eq!(body.trim(), "Human,", "Actor gained a variant: {body:?}");
    }
}
