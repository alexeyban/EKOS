//! RFC 0164 — turning recovered logic into target logic, Phase 1.
//!
//! Four deterministic pieces, no LLM and no database connection:
//!
//! | Module | What it decides |
//! |---|---|
//! | [`lower`] | A PostgreSQL view becomes a ClickHouse `CREATE VIEW`, or a list of named blockers. |
//! | [`invention`] | Whether a candidate statement states anything its source does not. |
//! | [`gate`] | Whether a routine may be handed to a reconstruction step at all. |
//! | [`reconstruct`] | The prompt for a routine, and the verdict on a model's answer. No model is called here. |
//! | [`triggers`] | What a trigger of each RFC 0163 class is proposed to become. |
//! | [`v5`] | Whether a lowered view returns what its source returns, on a fixture built from the view's own comparisons. |
//!
//! The rule that runs through all four: **a construct this crate does not know is refused by
//! name, never passed through.** Every walk over a statement ends in a catch-all that blocks, so a
//! new SQL feature costs a lowering, not a silent change of meaning.
//!
//! The engines V5 runs on are behind a trait; this crate opens no connection itself.

pub mod gate;
pub mod invention;
pub mod lower;
pub mod reconstruct;
pub mod triggers;
pub mod v5;

pub use lower::{Blocker, LowerOptions, Lowered, Lowering, lower_view};

pub use gate::{Ineligible, recorded_gate, routine_gate};
pub use invention::{
    Atom, AtomKind, InventionReport, RoutineAtoms, check_candidate, routine_atoms, sql_atoms,
};
pub use triggers::{TriggerOutcome, TriggerProposal, propose_trigger};
