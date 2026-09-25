//! RFC 0157 — the live PostgreSQL connector.
//!
//! Three jobs: connect safely, read the catalog completely, and never let a secret or a row value
//! escape into the ledger.
//!
//! # Why a synchronous driver
//!
//! `postgres`, not `tokio-postgres` or `sqlx`. RFC 0154 Phase 0 left one question open — how a
//! chunk-parallel executor coexists with a `KnowledgeStore` that is not `Sync`, in a CLI whose
//! async work is driven by `block_on` and never spawned. An async driver forces that question to be
//! answered now, for profiling, which does not need it.
//!
//! The synchronous driver sidesteps it entirely: [`ekos_migrate_validate::EngineReader`] is already a
//! synchronous trait, so a session implements it directly with no runtime and no bridging. When
//! RFC 0160 does need chunk parallelism, the answer is a pool of independent sessions on separate
//! threads — each owning its own connection, results collected before anything touches the store —
//! which is a better shape than sharing one async client anyway, and it does not make the ledger's
//! thread-safety anyone else's problem.
//!
//! `postgres` is the official synchronous wrapper over `tokio-postgres` from the same authors, so
//! this is a choice of API surface rather than of implementation.

pub mod catalog;
pub mod session;

pub use catalog::{CatalogObject, CatalogSnapshot, ObjectKind, introspect, reconcile};
pub use session::{PgSource, SessionPolicy, WriteCheck};

#[derive(Debug, thiserror::Error)]
pub enum PgError {
    #[error("cannot connect: {0}")]
    Connect(String),
    #[error("cannot apply session setting {setting}: {message}")]
    Session { setting: String, message: String },
    #[error("query failed: {message}\nsql: {sql}")]
    Query { sql: String, message: String },
    #[error(
        "the environment variable {0} names this connection's password but is not set. The ledger \
         stores the variable's name, never its value — export it before connecting."
    )]
    MissingSecret(String),
    #[error("refusing to build a SET statement from an unsafe value: {0:?}")]
    UnsafeSetting(String),
    #[error(
        "catalog introspection found {got} {kind} but the server reports {expected}. A short \
         catalog silently shrinks the completeness check's denominator (RFC 0158), so this is an \
         error rather than a smaller list."
    )]
    CatalogIncomplete {
        kind: &'static str,
        expected: usize,
        got: usize,
    },
    #[error(
        "unknown pg_class.relkind {0:?} — add it to catalog::ObjectKind rather than skipping it"
    )]
    UnknownRelkind(String),
    #[error("unknown pg_constraint.contype {0:?}")]
    UnknownConstraintType(String),
    #[error("unknown pg_proc.prokind {0:?}")]
    UnknownRoutineKind(String),
    #[error("unknown pg_type.typtype {0:?}")]
    UnknownTypeKind(String),
}
