//! RFC 0164 — turning recovered logic into target logic, Phase 1.
//!
//! Four deterministic pieces, no LLM and no database connection:
//!
//! | Module | What it decides |
//! |---|---|
//! | [`lower`] | A PostgreSQL view becomes a ClickHouse `CREATE VIEW`, or a list of named blockers. |
//! | [`invention`] | Whether a candidate statement states anything its source does not. |
//! | [`gate`] | Whether a routine may be handed to a reconstruction step at all. |
//! | [`triggers`] | What a trigger of each RFC 0163 class is proposed to become. |
//!
//! The rule that runs through all four: **a construct this crate does not know is refused by
//! name, never passed through.** Every walk over a statement ends in a catch-all that blocks, so a
//! new SQL feature costs a lowering, not a silent change of meaning.
//!
//! Not here yet: IR-constrained reconstruction and V5 differential execution (RFC 0164's later
//! phases). Nothing in this crate runs the logic it emits.

pub mod gate;
pub mod invention;
pub mod lower;
pub mod triggers;

pub use lower::{Blocker, LowerOptions, Lowered, Lowering, lower_view};

pub use gate::{Ineligible, recorded_gate, routine_gate};
pub use invention::{
    Atom, AtomKind, InventionReport, RoutineAtoms, check_candidate, routine_atoms, sql_atoms,
};
pub use triggers::{TriggerOutcome, TriggerProposal, propose_trigger};
