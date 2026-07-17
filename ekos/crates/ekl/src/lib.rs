//! Enterprise Knowledge Language (EKL) — a deterministic, composable query
//! language over the EKOS knowledge graph (RFC 0010).
//!
//! EKL compiles to `Runtime` calls only — it never touches the ledger
//! directly, upholding the same consumer-facing boundary as `ekos ask`
//! (RFC 0009) and the Runtime itself (RFC 0005).

pub mod interpreter;
pub mod parser;

pub use interpreter::{EklError, EklInterpreter, EklResult};
pub use parser::{EklAst, Entity, Literal, Op, Order, ParseError, Predicate, ekl_parse};
