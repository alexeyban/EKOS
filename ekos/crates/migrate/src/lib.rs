//! RFC 0154 — EKOS Migrate: the migration project model, migration units and the append-only
//! state machine.
//!
//! **Position in the architecture.** Migrate is an *auxiliary subsystem*, beside the compiler, in
//! the same position `simulation`, `marketing` and `session` occupy. It is not a `CompilerPass`,
//! it never writes through `Runtime`, and it runs *after* `commit`, over the compiled CKM. The
//! compiler's invariants — deterministic side-effect-free passes, a read-only runtime, an
//! append-only ledger — are untouched by it.
//!
//! That matters because Migrate is the first component in EKOS that will write to systems outside
//! the workspace. Stating the position explicitly is what keeps those invariants true rather than
//! quietly broken.
//!
//! Phase 0 (this crate today) ships the foundation only: the project model, units, the state
//! machine and its two write shapes, connection references with no credentials, environments, and
//! the human-only lifecycle seam. Profiling, rules, mapping, execution, validation, approval and
//! reporting are RFCs 0155–0167.

pub mod connection;
pub mod kinds;
pub mod lifecycle;
pub mod profile_facts;
pub mod project;
pub mod state;

pub use connection::{ConnectionError, ConnectionRef, EngineKind, Environment};
pub use profile_facts::{ColumnProfileFact, DriftFact, DriftKind, TableProfileFact};
pub use project::{Project, Unit, transition};
pub use state::{ALL_STATES, UnitState};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Ledger(#[from] ekos_ledger::LedgerError),
    #[error("cannot serialize a fact: {0}")]
    Serialize(#[from] serde_json::Error),
    #[error(transparent)]
    Connection(#[from] ConnectionError),
    #[error("not a migration unit: {0}")]
    NotAUnit(String),
    #[error("not a migration object: {0}")]
    NotAMigrationObject(String),
    #[error("no such object: {0}")]
    NotFound(String),
    #[error("{0}")]
    UnknownState(String),
    #[error("illegal state transition: {from} -> {to}")]
    IllegalTransition { from: UnitState, to: UnitState },
    #[error("a reason is required for this decision")]
    ReasonRequired,
    #[error("migration project not found: {0}")]
    NoSuchProject(String),
}
