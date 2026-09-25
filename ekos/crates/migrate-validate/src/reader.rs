//! RFC 0156 — the read seam the tiers run over.
//!
//! Deliberately tiny and deliberately read-only. Two reasons it is a trait rather than a concrete
//! client:
//!
//! 1. **The independent-oracle rule.** The validator must not read a target through the thing that
//!    wrote it — a Spark job that mis-serialized a decimal on write mis-serializes it identically on
//!    read, and the comparison passes. A trait makes "which path reads this" an explicit choice at
//!    the call site instead of an accident.
//! 2. **It keeps the driver decision where it belongs.** Choosing `tokio-postgres` vs `sqlx` is
//!    RFC 0157's call, and it is entangled with the open non-`Sync` `KnowledgeStore` question. The
//!    tiers do not need to know.

use crate::Dialect;

#[derive(Debug, thiserror::Error)]
pub enum ReadError {
    #[error("{engine} query failed: {message}")]
    Query { engine: String, message: String },
    #[error("expected {expected} column(s), got {got}: {sql}")]
    Shape {
        expected: usize,
        got: usize,
        sql: String,
    },
    #[error("cannot parse {value:?} as {as_type} from column {column}")]
    Parse {
        value: String,
        as_type: &'static str,
        column: usize,
    },
}

/// A read-only query path to one engine.
pub trait EngineReader {
    fn dialect(&self) -> Dialect;

    /// A short label for diagnostics and for the `MigrationValidationRun` fact — it records *which
    /// path* read the data, which is what makes the independent-oracle rule auditable rather than
    /// aspirational.
    fn label(&self) -> &str;

    /// Execute a read-only query and return its rows as column strings.
    fn query(&self, sql: &str) -> Result<Vec<Vec<String>>, ReadError>;

    fn one_row(&self, sql: &str) -> Result<Vec<String>, ReadError> {
        let rows = self.query(sql)?;
        match rows.len() {
            1 => Ok(rows.into_iter().next().expect("checked len")),
            got => Err(ReadError::Shape {
                expected: 1,
                got,
                sql: sql.to_string(),
            }),
        }
    }

    fn scalar_u64(&self, sql: &str) -> Result<u64, ReadError> {
        let row = self.one_row(sql)?;
        let v = row.first().ok_or_else(|| ReadError::Shape {
            expected: 1,
            got: 0,
            sql: sql.to_string(),
        })?;
        v.trim().parse().map_err(|_| ReadError::Parse {
            value: v.clone(),
            as_type: "u64",
            column: 0,
        })
    }
}

/// An in-memory reader over pre-computed answers, for testing tier and bisect logic without an
/// engine.
///
/// It answers by exact SQL match, which is blunt on purpose: a test that silently matched a
/// *similar* query would stop testing the query the tiers actually build.
pub struct MockReader {
    pub dialect: Dialect,
    pub label: String,
    pub answers: std::collections::HashMap<String, Vec<Vec<String>>>,
}

impl MockReader {
    pub fn new(dialect: Dialect, label: impl Into<String>) -> Self {
        Self {
            dialect,
            label: label.into(),
            answers: Default::default(),
        }
    }

    pub fn with(mut self, sql: impl Into<String>, rows: Vec<Vec<String>>) -> Self {
        self.answers.insert(sql.into(), rows);
        self
    }
}

impl EngineReader for MockReader {
    fn dialect(&self) -> Dialect {
        self.dialect
    }
    fn label(&self) -> &str {
        &self.label
    }
    fn query(&self, sql: &str) -> Result<Vec<Vec<String>>, ReadError> {
        self.answers
            .get(sql)
            .cloned()
            .ok_or_else(|| ReadError::Query {
                engine: self.label.clone(),
                message: format!("no mocked answer for: {sql}"),
            })
    }
}
