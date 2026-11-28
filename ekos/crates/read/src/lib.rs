//! The read tools of the knowledge ledger as library functions (RFC 0269 phase 1).
//!
//! One implementation behind two front ends: the MCP server (`ekos mcp serve`) and the Python
//! package's native engine. Every function reads through [`ekos_runtime::Runtime`] or a
//! `&dyn KnowledgeStore` it is handed — the caller decides the store, and so the read scope
//! (RFC 0181). Nothing here writes, opens a store, or makes a network call.

pub mod args;
pub mod conflicts;
pub mod kinds;
pub mod tools;

/// Why a read failed, by who can fix it.
#[derive(Debug, thiserror::Error)]
pub enum ReadError {
    /// The caller's argument is wrong: a missing or malformed value, a query that does not parse.
    #[error("{0}")]
    Argument(String),
    /// The argument is well-formed and names nothing in the ledger.
    #[error("{0}")]
    NotFound(String),
    /// The store could not be read.
    #[error("{0}")]
    Store(String),
}

impl From<ekos_ledger::LedgerError> for ReadError {
    fn from(e: ekos_ledger::LedgerError) -> Self {
        Self::Store(e.to_string())
    }
}

impl From<ekos_runtime::RuntimeError> for ReadError {
    fn from(e: ekos_runtime::RuntimeError) -> Self {
        Self::Store(e.to_string())
    }
}

impl From<serde_json::Error> for ReadError {
    fn from(e: serde_json::Error) -> Self {
        Self::Store(e.to_string())
    }
}

/// A read's result.
pub type Result<T> = std::result::Result<T, ReadError>;
