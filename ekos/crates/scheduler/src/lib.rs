//! Scheduler — re-exports from compiler-core.
//!
//! The scheduler logic lives in `ekos-compiler-core`. This crate exists
//! as a separately versionable surface for future policy extensions.
pub use ekos_compiler_core::{ExecutionReport, FailureMode, PassOutcome, Scheduler};
