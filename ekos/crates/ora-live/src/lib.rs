//! RFC 0211 — the live Oracle connector.
//!
//! The Oracle twin of `pg-live`: connect safely, read the catalog completely, and never let a
//! secret or a row value escape. The driver (rust-oracle over ODPI-C) is behind the feature
//! `oracle`; everything that maps catalog rows to facts is driver-independent and tested over
//! recorded rows ([`catalog::CatalogSource`]).

pub mod catalog;
pub mod profile;
pub mod session;

pub use catalog::{
    CatalogObject, CatalogSnapshot, CatalogSource, Census, ObjectKind, ViewFamily, introspect,
    reconcile,
};
pub use profile::{ProfileStats, ProfileTier, ScanSource};
pub use session::{OraConnection, OraSource, SessionPolicy, WriteCheck};

#[derive(Debug, thiserror::Error)]
pub enum OraError {
    #[error(
        "this build has no Oracle driver. Rebuild with the `oracle` feature; Oracle Instant Client \
         must also be installed, because the driver loads it at run time"
    )]
    DriverNotBuilt,
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
    #[error("refusing to use {0:?} as an Oracle identifier")]
    UnsafeIdentifier(String),
    #[error(
        "the dictionary lists object type {0:?}, which this connector neither reads nor counts. \
         Add it to catalog::ObjectKind or catalog::COUNTED_ONLY rather than skipping it"
    )]
    UnknownObjectType(String),
    #[error(
        "catalog introspection found {got} {kind} object(s) but the dictionary lists {expected}. \
         A short catalog silently shrinks every coverage number computed from it, so this is an \
         error rather than a smaller list. With ALL_* views this usually means missing privileges: \
         grant SELECT_CATALOG_ROLE so the DBA_* views can be read."
    )]
    CatalogIncomplete {
        kind: &'static str,
        expected: usize,
        got: usize,
    },
}
