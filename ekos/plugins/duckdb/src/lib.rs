//! RFC 0259 — DuckDB files, read-only.
//!
//! DuckDB serves the lakehouse's gold marts from one file. EKOS reads such a file and never
//! writes it (RFC 0259 §4): [`probe`] runs the text-to-SQL probes (`ekos sql check --duckdb`)
//! through a read-only, locked session with a deadline, and [`guard`] refuses anything that
//! would reach a file, the network or a setting before it is sent.
//!
//! The engine is the bundled `duckdb` crate behind the `duckdb` feature, off in the default build
//! like `oracle` (its C++ build takes several minutes). Without it the guard still runs and every
//! attempt to open a file says the feature is missing.

pub mod guard;
pub mod probe;
#[cfg(feature = "duckdb")]
pub mod session;

/// Why nothing was opened in a build without the engine.
pub const NOT_BUILT: &str = "this ekos was built without the duckdb feature, so DuckDB files \
     cannot be opened (rebuild with `--features duckdb`)";

/// Whether this build can open DuckDB files.
pub const fn available() -> bool {
    cfg!(feature = "duckdb")
}
