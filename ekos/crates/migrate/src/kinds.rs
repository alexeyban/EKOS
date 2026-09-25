//! RFC 0154 — the `ObjectKind::Custom(_)` names EKOS Migrate owns.
//!
//! Every name carries a `Migration` prefix on purpose. `ekos_identity`'s `normalize()` lowercases,
//! so bare names like `Disposition`, `Divergence` or `ValidationRun` are collision bait against a
//! future analyzer kind — the failure RFC 0147 hit with `PerlPackage`/`PerlSymbol`.
//!
//! All of them are **structurally keyed**: an instance is identified by
//! `(project, unit key)` / `(unit, tier)` / `(project, alias)`, so no two distinct instances can
//! ever be the same real-world entity. Each has a `structurally_keyed: true` row in
//! [`ekos_kir::custom_kinds::REGISTRY`], which is what keeps `DefaultResolver` from collapsing a
//! whole migration into one canonical object through its same-kind `structural_score` fallback.

/// The migration project — one per (source, target) pair being migrated.
pub const PROJECT_KIND: &str = "MigrationProject";
/// A table, view, function, or group of them migrated together.
pub const UNIT_KIND: &str = "MigrationUnit";
/// A named connection: kind, host alias and database. **Never a credential.**
pub const CONNECTION_KIND: &str = "MigrationConnectionRef";
/// Table-level profile (RFC 0157). Aggregates only — never a row value.
pub const TABLE_PROFILE_KIND: &str = "MigrationTableProfile";
/// Column-level profile (RFC 0157). Bounds appear only for non-PII, non-text columns.
pub const COLUMN_PROFILE_KIND: &str = "MigrationColumnProfile";
/// A difference between the live catalog and the repository's own DDL (RFC 0157).
pub const DRIFT_KIND: &str = "MigrationDrift";

/// Every object kind this crate writes. The CLI guard test in `ekos-identity` asserts each has a
/// registry row; this constant is what makes that list reviewable in one place.
pub const ALL_KINDS: [&str; 6] = [
    PROJECT_KIND,
    UNIT_KIND,
    CONNECTION_KIND,
    TABLE_PROFILE_KIND,
    COLUMN_PROFILE_KIND,
    DRIFT_KIND,
];

/// Emitted on every state transition (RFC 0154 — the state machine's second write).
pub const TRANSITION_EVENT: &str = "MigrationTransition";
/// Emitted when an object is superseded. Nothing is ever deleted.
pub const STATUS_CHANGED_EVENT: &str = "MigrationStatusChanged";

/// `project → unit` containment, and `unit → superseding unit`.
pub const HAS_UNIT: &str = "HasMigrationUnit";
/// `unit → its profile`, and `table profile → column profile`.
pub const HAS_PROFILE: &str = "HasMigrationProfile";
pub const SUPERSEDES: &str = "Supersedes";
