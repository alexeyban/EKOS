//! RFC 0151 Phase 1 — the agent session inbox.
//!
//! A safe staging area for notes an agent wants to remember. Nothing here touches the ledger:
//! notes are redacted, capped, and appended to a per-session JSONL file, and only a later
//! observe → compile → commit pass (Phase 2) can turn them into knowledge.

pub mod anchor;
pub mod capture;
pub mod commit;
pub mod eval;
pub mod inbox;
pub mod lifecycle;
pub mod map;
pub mod read;

pub use inbox::{
    Anchor, DEFAULT_INBOX_DIR, Inbox, InboxError, InboxLimits, NoteInput, NoteKind, NoteOutcome,
    Redactor, SessionEntry, SessionStatus,
};
