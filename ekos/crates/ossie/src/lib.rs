//! RFC 0245 — [Apache Ossie](https://ossie.apache.org) semantic models for EKOS.
//!
//! Pure: no store, no files. The CLI (`ekos export ossie`, `ekos import ossie`), the MCP tool
//! `ekos_ossie_export` and `recovery`'s `OssieAnalyzerPass` load and write; this crate maps.
//!
//! - [`model`] — the document types (writes spec `0.2.0.dev0`, reads `0.1.1` too);
//! - [`read`] — parse and sniff documents;
//! - [`build`] — the ledger as one model;
//! - [`check`] — the structural checks of Ossie's own validator;
//! - [`datatype`] — SQL types → Ossie data types.

pub mod build;
pub mod check;
pub mod datatype;
pub mod fk;
pub mod kinds;
pub mod model;
pub mod read;

pub use build::{BuildOptions, LedgerView, from_ledger};
pub use check::{Problem, check};
pub use model::*;
pub use read::{ReadError, parse, sniff};

/// The official JSON Schema this crate writes against (apache/ossie@8437d59).
pub const SCHEMA_JSON: &str = include_str!("../schema/ossie-schema.json");
