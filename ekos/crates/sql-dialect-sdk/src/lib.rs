//! `SqlDialectParser` trait (RFC 0031) — the contract a pluggable SQL dialect crate
//! implements so `ekos-recovery`'s `SqlAnalyzerPass`/`SqlTransformAnalyzerPass` can parse
//! dialect-specific SQL without either pass hardcoding a dialect.
//!
//! Deliberately as small as `ekos_observation_sdk::Observer` (two required methods, no
//! associated types): a dialect crate's only job is saying *which* `sqlparser` dialect to
//! parse with and any dialect-specific text preprocessing it needs before that — all
//! DDL/transform-graph construction stays in `ekos-recovery`, so this trait never needs to
//! change when the IR it feeds does.

pub mod guard;
pub mod lex;
pub mod probe;

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

    /// How the engine behaves where generated SQL most often goes wrong (RFC 0242): checks
    /// consult these instead of hard-coding a dialect. Default: nothing is claimed, so no
    /// dialect-dependent check fires.
    fn facts(&self) -> DialectFacts {
        DialectFacts::default()
    }

    /// Short rules for writing correct SQL for this engine (RFC 0243): row limits, division,
    /// concatenation, case-insensitive matching, dates, quoting, NULL ordering. Shown to whoever
    /// writes SQL — a model before it drafts, a person running `ekos sql check`.
    fn generation_notes(&self) -> &'static [&'static str] {
        &[]
    }
}

/// How a query limits its rows (RFC 0243).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RowLimit {
    /// Not claimed: no row-limit check fires.
    #[default]
    Unknown,
    /// `LIMIT n` only (SQLite, MySQL, ClickHouse, Databricks).
    Limit,
    /// `FETCH FIRST n ROWS ONLY` only (Oracle 12c+).
    FetchFirst,
    /// `LIMIT n` or `FETCH FIRST n ROWS ONLY` (PostgreSQL, Trino, Snowflake).
    LimitOrFetch,
    /// `TOP n` or `OFFSET … FETCH NEXT` (SQL Server).
    TopOrFetch,
}

/// Engine behaviour that decides whether a query means what it looks like (RFC 0242/0243). Each
/// field is `true` only when the engine is documented to behave that way; `false` means "not
/// claimed". Start a dialect's facts from [`DialectFacts::GENERIC`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DialectFacts {
    /// `7 / 2` is `3` when both operands are integers.
    pub integer_division: bool,
    /// NULL sorts before every value in ascending order (and after them descending).
    pub nulls_first_ascending: bool,
    /// `LIKE` ignores case (at least for ASCII).
    pub like_case_insensitive: bool,
    /// `ILIKE` exists.
    pub has_ilike: bool,
    /// Dates and times are stored and compared as text, so a literal must match the stored format.
    pub dates_are_text: bool,
    /// RFC 0243: the row-limit forms the engine accepts.
    pub row_limit: RowLimit,
    /// RFC 0243: the character that quotes an identifier (`"`, `` ` ``, or `[` for `[name]`);
    /// `None` when not claimed.
    pub identifier_quote: Option<char>,
    /// RFC 0243: `"text"` is a string literal, not a name (MySQL's default mode).
    pub double_quote_is_string: bool,
    /// RFC 0243: `` `name` `` is accepted as a quoted identifier.
    pub backtick_identifiers: bool,
}

impl DialectFacts {
    /// Nothing claimed.
    pub const GENERIC: DialectFacts = DialectFacts {
        integer_division: false,
        nulls_first_ascending: false,
        like_case_insensitive: false,
        has_ilike: false,
        dates_are_text: false,
        row_limit: RowLimit::Unknown,
        identifier_quote: None,
        double_quote_is_string: false,
        backtick_identifiers: false,
    };
}

impl Default for DialectFacts {
    fn default() -> Self {
        DialectFacts::GENERIC
    }
}
