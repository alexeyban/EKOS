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
pub mod pii;
pub mod profile;
pub mod session;

pub use catalog::{CatalogObject, CatalogSnapshot, ObjectKind, introspect, reconcile};
pub use pii::{Classification, PiiClass};
pub use profile::{ColumnProfile, ProfileTier, TableProfile};
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
        "replica lag is {lag_seconds:.1}s, above the {max_seconds:.1}s policy limit. A run against \
         a lagging replica produces divergences that are really just lag — wait, or point at the \
         primary deliberately."
    )]
    ReplicaLagTooHigh { lag_seconds: f64, max_seconds: f64 },
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

/// Convert a connector profile into the source-independent fact shape `ekos-migrate` persists.
///
/// The conversion is where the "no row values" rule is re-checked rather than assumed: `min`/`max`
/// are copied only when the connector did not suppress them, so a future change that forgets to
/// clear a bound cannot leak one through this path either.
impl From<&profile::TableProfile> for ekos_migrate::TableProfileFact {
    fn from(p: &profile::TableProfile) -> Self {
        Self {
            qualified_name: p.qualified_name.clone(),
            tier: format!("{:?}", p.tier).to_lowercase(),
            row_count: p.row_count,
            row_count_is_exact: p.row_count_is_exact,
            total_bytes: p.total_bytes,
            last_analyze: p.last_analyze.clone(),
            inserts: p.inserts,
            updates: p.updates,
            deletes: p.deletes,
        }
    }
}

impl From<&profile::ColumnProfile> for ekos_migrate::ColumnProfileFact {
    fn from(c: &profile::ColumnProfile) -> Self {
        let suppressed = c.values_suppressed;
        Self {
            qualified_name: c.qualified_name.clone(),
            tier: format!("{:?}", c.tier).to_lowercase(),
            data_type: c.data_type.clone(),
            null_fraction: c.null_fraction,
            distinct_estimate: c.distinct_estimate,
            // Belt and braces: the connector already clears these for a suppressed column.
            min: if suppressed { None } else { c.min.clone() },
            max: if suppressed { None } else { c.max.clone() },
            numeric_precision_used: c.numeric_precision_used,
            numeric_scale_used: c.numeric_scale_used,
            monotonic: c.monotonic,
            pii_class: c
                .pii
                .as_ref()
                .map(|p| format!("{:?}", p.class).to_lowercase()),
            pii_method: c
                .pii
                .as_ref()
                .map(|p| format!("{:?}", p.method).to_lowercase()),
            values_suppressed: suppressed,
        }
    }
}

#[cfg(test)]
mod conversion_tests {
    use super::*;

    #[test]
    fn a_suppressed_column_cannot_carry_bounds_through_the_conversion() {
        let c = profile::ColumnProfile {
            qualified_name: "s.t.email".into(),
            tier: profile::ProfileTier::P1,
            data_type: "text".into(),
            null_fraction: 0.0,
            distinct_estimate: None,
            avg_width: 32,
            // Deliberately populated, as a change upstream might leave them.
            min: Some("aaa@example.com".into()),
            max: Some("zzz@example.com".into()),
            numeric_precision_used: None,
            numeric_scale_used: None,
            monotonic: None,
            pii: Some(pii::Classification {
                class: pii::PiiClass::Email,
                method: pii::Method::ColumnName,
                confidence: 0.6,
            }),
            values_suppressed: true,
        };
        let f: ekos_migrate::ColumnProfileFact = (&c).into();
        assert_eq!(
            f.min, None,
            "a suppressed bound must not survive the conversion"
        );
        assert_eq!(f.max, None);
        assert_eq!(f.pii_class.as_deref(), Some("email"));
        assert!(f.values_suppressed);
    }

    #[test]
    fn an_unsuppressed_numeric_column_keeps_its_bounds() {
        let c = profile::ColumnProfile {
            qualified_name: "s.t.amount".into(),
            tier: profile::ProfileTier::P0,
            data_type: "numeric".into(),
            null_fraction: 0.0,
            distinct_estimate: Some(-1.0),
            avg_width: 8,
            min: Some("1.00".into()),
            max: Some("99.00".into()),
            numeric_precision_used: Some(4),
            numeric_scale_used: Some(2),
            monotonic: None,
            pii: None,
            values_suppressed: false,
        };
        let f: ekos_migrate::ColumnProfileFact = (&c).into();
        assert_eq!(f.min.as_deref(), Some("1.00"));
        assert_eq!(f.tier, "p0");
        assert_eq!(f.numeric_scale_used, Some(2));
    }
}
