//! `SqlDialectParser` trait (RFC 0031) — the contract a pluggable SQL dialect crate
//! implements so `ekos-recovery`'s `SqlAnalyzerPass`/`SqlTransformAnalyzerPass` can parse
//! dialect-specific SQL without either pass hardcoding a dialect.
//!
//! Deliberately as small as `ekos_observation_sdk::Observer` (two required methods, no
//! associated types): a dialect crate's only job is saying *which* `sqlparser` dialect to
//! parse with and any dialect-specific text preprocessing it needs before that — all
//! DDL/transform-graph construction stays in `ekos-recovery`, so this trait never needs to
//! change when the IR it feeds does.

use sqlparser::dialect::Dialect;

/// Implemented once per SQL dialect (generic/ANSI, MySQL, PostgreSQL, ...) and registered by
/// name in `ekos-recovery`'s dialect registry — see RFC 0031's "connect a new one by
/// configuration" design. Registering a new dialect means: write a crate implementing this
/// trait, add it as a workspace member + dependency, add one line to the registry — the same
/// compile-time pattern already used for every `Observer` plugin in this codebase.
pub trait SqlDialectParser: Send + Sync {
    /// The registry/config key this parser answers to — e.g. `"mysql"`, `"postgres"`,
    /// `"generic"`. Must match the `dialect` value used in `ekos.toml`'s
    /// `[recover.sql]`/`[[recover.sql.dialect-rules]]`.
    fn name(&self) -> &str;

    /// Which `sqlparser` dialect to parse with. Returns a fresh `Box` each call since
    /// `sqlparser`'s dialect types aren't `Clone`. Bounded `+ Send + Sync` so callers (like
    /// `ekos-recovery`'s `CompilerPass` impls, which must themselves be `Send + Sync`) can
    /// store the result in a field.
    fn sqlparser_dialect(&self) -> Box<dyn Dialect + Send + Sync>;

    /// Dialect-specific text preprocessing applied before handing `sql` to `sqlparser` — e.g.
    /// stripping MySQL's `DELIMITER $$ ... $$` client-tool convention, which is not part of
    /// SQL grammar and no `sqlparser` dialect understands it. Default: no preprocessing.
    fn preprocess(&self, sql: &str) -> String {
        sql.to_string()
    }
}
