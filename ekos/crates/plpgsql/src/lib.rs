//! RFC 0163 — a deterministic, in-process PL/pgSQL parser producing a procedural IR.
//!
//! **No LLM anywhere in this crate**, and that ordering is the entire point. RFC 0164 wants to
//! verify that generated logic invents nothing by requiring every generated predicate to map to a
//! source IR node — and against `Unmapped`, *everything* maps. The check passes on invented logic
//! and the system reports green, which is worse than having no check at all.
//!
//! RFC 0148 hit the same wall for compiled binaries and RFC 0150 broke through it by writing a real
//! in-process CIL decoder before letting a model near the output. This is that decoder for
//! PL/pgSQL.
//!
//! # Deviation from RFC 0163
//!
//! The RFC specified `ProcStmt::Sql { graph: TransformGraph }`, embedding the existing dataflow IR
//! directly. This crate carries the statement's **text and span** instead, and leaves lowering to
//! `TransformGraph` to the consumer (RFC 0164).
//!
//! The reason is dependency direction: a parser that depends on `ekos-semantic` cannot be tested
//! without it, and the dataflow lowering is a separate concern that changes for separate reasons.
//! The procedural layer still owns order and condition, and the dataflow layer still owns what each
//! statement reads and writes — the seam just sits one step later than the RFC drew it.
pub mod ir;
pub mod lex;
pub mod parse;
pub mod source;

pub use ir::{CursorOp, Fidelity, LoopKind, ProcSignature, ProcStmt, ProcedureIr, Span, VarDecl};
pub use parse::{Origin, parse_body, parse_function};
pub use source::{RoutineSource, line_of, routines};
