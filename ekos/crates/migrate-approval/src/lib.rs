//! RFC 0161 — risk classes, approval requests and the human-only decision path.
//!
//! Approval workflows fail in three predictable ways. Each is closed mechanically here:
//!
//! 1. **Risk assigned by category** gives a trivial rename and a column drop that breaks eleven
//!    consumers the same dialog, and approvers learn the dialog means nothing. [`risk::assess`]
//!    computes from statement class, environment, lossiness, blast radius and affected rows, and
//!    records every escalation so an approver reads reasons rather than a letter.
//! 2. **Approval granted on evidence that then changes** — approved at 3 rows, executed at 30,000.
//!    [`request::EvidenceSnapshot`] freezes the facts and their hashes, and a mismatch makes the
//!    request *dead* rather than warned-about.
//! 3. **The agent approving its own proposal.** [`lifecycle`] is the only path, `commands/mcp.rs`
//!    may not reference it (enforced by a source scan), [`lifecycle::Actor`] has no `Agent` variant,
//!    and a requester cannot appear among the approvers.

pub mod lifecycle;
pub mod policy;
pub mod request;
pub mod risk;

pub use lifecycle::Actor;
pub use policy::{LoadedPolicy, Policy, load as load_policy};
pub use request::{
    ApprovalRequest, EvidenceRef, EvidenceSnapshot, RequestError, RequestStatus, identity_of,
};
pub use risk::{ActionFacts, Escalation, RiskAssessment, RiskClass, Thresholds, assess};
