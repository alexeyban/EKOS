//! Constrained reconstruction: judging a model's proposed target implementation of a routine.
//!
//! This module calls no model. It builds the prompt, and it judges the answer — the two parts that
//! decide what a model can get away with. The call itself is the caller's, behind whatever provider
//! and spending rule it has.
//!
//! Three fences, in order (RFC 0164):
//!
//! 1. **The gate comes before the prompt.** A routine that is not fully recovered, or that builds
//!    SQL at run time, is never sent ([`prompt`] returns the reason instead).
//! 2. **The candidate must parse and invent nothing.** Every table, column, literal, function and
//!    comparison in it must be stated by the source's procedural IR. One unmapped atom rejects it.
//! 3. **What survives is a proposal.** Nothing here marks it accepted, approved or executable.
//!
//! What this cannot establish is that the candidate *does* what the routine does. A dropped branch
//! invents nothing and passes. That is V5's question, and V5 for routines is not built, so a
//! proposal from here says so.

use crate::gate::{Ineligible, routine_gate};
use crate::invention::{InventionReport, check_candidate, routine_atoms};
use ekos_plpgsql::ProcedureIr;
use serde::{Deserialize, Serialize};

/// Bumped when the prompt text changes: it is part of what a recorded proposal cites.
pub const PROMPT_VERSION: &str = "migrate-reconstruct-v1";

pub const SYSTEM: &str = "You translate one PostgreSQL PL/pgSQL routine into ClickHouse SQL. \
You answer with ClickHouse SQL statements only, inside one ```sql fence, and nothing else. \
ClickHouse has no procedural language: no variables, IF, loops or exceptions. \
Use only tables, columns, constants, functions and comparisons that appear in the routine. \
Never add a filter, a constant, a table or a function the routine does not contain. \
If the routine cannot be expressed as plain ClickHouse SQL statements, answer with the single \
word CANNOT.";

/// What a model said it could not do.
pub const CANNOT: &str = "CANNOT";

/// The user prompt for one routine, or why it may not be sent.
pub fn prompt(source: &str, ir: &ProcedureIr) -> Result<String, Ineligible> {
    routine_gate(ir)?;
    Ok(format!(
        "Routine `{}` ({} argument(s), returns {}):\n\n```sql\n{}\n```\n",
        ir.signature.name,
        ir.signature.arguments.len(),
        ir.signature.returns.as_deref().unwrap_or("nothing"),
        source.trim()
    ))
}

/// The SQL in a model's answer: the first fenced block, or the whole answer when it has no fence.
pub fn extract_sql(answer: &str) -> Option<String> {
    let body = match answer.find("```") {
        Some(start) => {
            let after = &answer[start + 3..];
            let after = after
                .strip_prefix("sql")
                .or_else(|| after.strip_prefix("SQL"))
                .unwrap_or(after);
            &after[..after.find("```").unwrap_or(after.len())]
        }
        None => answer,
    };
    let body = body.trim();
    (!body.is_empty()).then(|| body.to_string())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "outcome")]
pub enum Reconstruction {
    /// The routine never reached a model.
    Ineligible { why: String },
    /// The model said it could not, or said nothing usable.
    NoCandidate { why: String },
    /// The candidate does not parse, or states something the routine does not.
    Rejected {
        candidate: String,
        report: InventionReport,
    },
    /// The candidate parses and invents nothing. Still unexecuted.
    Proposed {
        candidate: String,
        /// Atoms of the candidate, all mapped to the source.
        checked: usize,
        /// Source statements or conditions that did not parse and so vouch for nothing.
        source_fragments_unparsed: usize,
    },
}

/// Judge a model's answer for a routine.
pub fn judge(ir: &ProcedureIr, answer: &str) -> Reconstruction {
    if let Err(why) = routine_gate(ir) {
        return Reconstruction::Ineligible {
            why: why.describe(),
        };
    }
    if answer.trim().trim_matches('.').eq_ignore_ascii_case(CANNOT) {
        return Reconstruction::NoCandidate {
            why: "the model answered that the routine cannot be expressed as plain SQL".into(),
        };
    }
    let Some(candidate) = extract_sql(answer) else {
        return Reconstruction::NoCandidate {
            why: "the answer held no SQL".into(),
        };
    };
    let source = routine_atoms(ir);
    let report = check_candidate(&source.atoms, &candidate);
    if report.accepted() {
        Reconstruction::Proposed {
            candidate,
            checked: report.checked,
            source_fragments_unparsed: source.unparsed,
        }
    } else {
        Reconstruction::Rejected { candidate, report }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_plpgsql::parse_function;

    const ROUTINE: &str = "CREATE FUNCTION open_total(in_customer int) RETURNS numeric AS $$\n\
        DECLARE t numeric;\n\
        BEGIN\n\
          SELECT sum(amount) INTO t FROM invoice WHERE customer_id = in_customer AND status = 'open';\n\
          RETURN t;\n\
        END; $$ LANGUAGE plpgsql;";

    fn ir(src: &str) -> ProcedureIr {
        parse_function(src).unwrap()
    }

    #[test]
    fn a_faithful_candidate_is_a_proposal_and_an_inventing_one_is_rejected() {
        let ir = ir(ROUTINE);
        let faithful = "```sql\nSELECT sum(amount) FROM invoice WHERE customer_id = in_customer \
                        AND status = 'open'\n```";
        assert!(
            matches!(judge(&ir, faithful), Reconstruction::Proposed { checked, .. } if checked > 5),
            "{:#?}",
            judge(&ir, faithful)
        );
        // The same answer with a filter nobody wrote.
        let inventing = "```sql\nSELECT sum(amount) FROM invoice WHERE customer_id = in_customer \
                         AND status = 'open' AND deleted = 0\n```";
        let Reconstruction::Rejected { report, .. } = judge(&ir, inventing) else {
            panic!("an invented filter was not rejected");
        };
        assert!(report.unmapped.iter().any(|a| a.text == "deleted"));
        // A changed constant.
        let changed =
            "SELECT sum(amount) FROM invoice WHERE customer_id = in_customer AND status = 'paid'";
        assert!(matches!(
            judge(&ir, changed),
            Reconstruction::Rejected { .. }
        ));
    }

    #[test]
    fn an_answer_that_is_not_sql_is_rejected_or_no_candidate() {
        let ir = ir(ROUTINE);
        assert!(matches!(
            judge(&ir, "CANNOT"),
            Reconstruction::NoCandidate { .. }
        ));
        assert!(matches!(
            judge(&ir, "  \n"),
            Reconstruction::NoCandidate { .. }
        ));
        assert!(matches!(
            judge(&ir, "```sql\n```"),
            Reconstruction::NoCandidate { .. }
        ));
        let prose = "Sure! Here is how I would approach this migration.";
        let Reconstruction::Rejected { report, .. } = judge(&ir, prose) else {
            panic!("prose was not rejected");
        };
        assert!(report.parse_error.is_some());
    }

    #[test]
    fn an_ineligible_routine_gets_no_prompt_and_no_verdict_on_any_answer() {
        let dynamic = ir("CREATE FUNCTION f(t text) RETURNS int AS $$ BEGIN \
                          EXECUTE 'DELETE FROM ' || t; RETURN 1; END; $$ LANGUAGE plpgsql;");
        assert!(prompt("…", &dynamic).is_err());
        assert!(matches!(
            judge(&dynamic, "SELECT 1"),
            Reconstruction::Ineligible { why } if why.contains("EXECUTE")
        ));
        let other = ir("CREATE FUNCTION g() RETURNS int AS $$ return 1 $$ LANGUAGE plpython3u;");
        assert!(prompt("…", &other).is_err());
    }

    #[test]
    fn the_prompt_carries_the_routine_and_nothing_else_from_the_workspace() {
        let p = prompt(ROUTINE, &ir(ROUTINE)).unwrap();
        assert!(
            p.contains("Routine `open_total` (1 argument(s), returns numeric)"),
            "{p}"
        );
        assert!(p.contains("WHERE customer_id = in_customer"));
        assert!(SYSTEM.contains("Never add a filter"));
    }

    #[test]
    fn sql_is_taken_from_the_first_fence_or_the_whole_answer() {
        assert_eq!(
            extract_sql("x\n```sql\nSELECT 1\n```\ny").as_deref(),
            Some("SELECT 1")
        );
        assert_eq!(
            extract_sql("```\nSELECT 2\n```").as_deref(),
            Some("SELECT 2")
        );
        assert_eq!(extract_sql("SELECT 3").as_deref(), Some("SELECT 3"));
        assert_eq!(extract_sql("```sql\nSELECT 4").as_deref(), Some("SELECT 4"));
    }
}
