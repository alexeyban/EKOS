//! RFC 0162 — the evidence-backed migration report.
//!
//! The report is compiled from ledger facts, not written by a model. Every factual sentence carries
//! the fact ids it rests on; the verifier resolves each one and checks it is the *kind* of fact that
//! can support the claim; and a report below the groundedness gate cannot be signed off.
//!
//! Three properties matter more than the content:
//!
//! 1. **Deterministic.** Recompiling from the same snapshot produces byte-identical output, so a
//!    signed report is re-derivable rather than merely archived.
//! 2. **Produced even when it fails.** A report below the gate is rendered with its blockers first —
//!    the person who has to close the gaps needs to see them.
//! 3. **Nothing is a matter of judgement at sign-off.** The five preconditions are mechanical, so
//!    the human decision is "do we switch the old system off", not "is this good enough".

pub mod citation;
pub mod render;
pub mod signoff;

pub use citation::{CitationProblem, Claim, ClaimKind, FactKinds, Groundedness, verify};
pub use render::{Report, Section, markdown};
pub use signoff::{Blocker, Preconditions};
