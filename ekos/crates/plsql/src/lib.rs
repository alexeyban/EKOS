//! RFC 0210 — Oracle scripts and PL/SQL, deterministically and in process.
//!
//! [`script`] splits an Oracle `.sql` file the way SQL\*Plus reads it: statements, PL/SQL units
//! ending at a `/` line, and SQL\*Plus commands. The `oracle` SQL dialect and the PL/SQL front end
//! both read files through it, so the two never disagree about where a unit ends.
//!
//! [`parse`] reads each PL/SQL unit — functions, procedures, triggers, packages — into RFC 0163's
//! `ProcedureIr`, handing statement parsing to `ekos-plpgsql` under its PL/SQL flavor. **No LLM
//! anywhere in this crate**, for the reason `ekos-plpgsql` gives: fidelity is computed from the IR,
//! and an IR a model filled in cannot be checked against.
pub mod parse;
pub mod script;

pub use parse::{Routine, ScriptRoutines, TriggerSpec, read_script};
pub use script::{Tok, TokKind, Unit, UnitKind, neutralize_quotes, tokens, units};
