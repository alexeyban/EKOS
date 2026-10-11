//! The fidelity gate: which routines may be handed to a reconstruction step at all.
//!
//! A model given a partial recovery fills the gaps, and the anti-invention check cannot see an
//! invention that fills a hole it has no atoms for. So the gate is on the recovery, before any
//! candidate exists: only a routine whose every statement was recovered, with no statement built at
//! run time, is eligible. Everything else is a finding for a person, never a proposal (RFC 0164).

use ekos_plpgsql::{Fidelity, ProcedureIr};
use serde::{Deserialize, Serialize};

/// Why a routine may not be reconstructed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "reason")]
pub enum Ineligible {
    /// The body was not parsed (another language, or no body).
    SignatureOnly,
    /// Parsed with gaps.
    Partial {
        recovered: usize,
        unrecovered: usize,
    },
    /// Statements are constructed at run time; their text is not statically known.
    DynamicSql { sites: usize },
}

impl Ineligible {
    pub fn describe(&self) -> String {
        match self {
            Self::SignatureOnly => {
                "only the signature was recovered; the body was not parsed".into()
            }
            Self::Partial {
                recovered,
                unrecovered,
            } => format!(
                "{unrecovered} of {} statements were not recovered",
                recovered + unrecovered
            ),
            Self::DynamicSql { sites } => {
                format!("{sites} statement(s) are built at run time (EXECUTE)")
            }
        }
    }
}

/// The gate over a parsed routine.
pub fn routine_gate(ir: &ProcedureIr) -> Result<(), Ineligible> {
    match ir.fidelity() {
        Fidelity::Signature => return Err(Ineligible::SignatureOnly),
        Fidelity::Partial {
            recovered,
            unrecovered,
        } => {
            return Err(Ineligible::Partial {
                recovered,
                unrecovered,
            });
        }
        Fidelity::Statements => {}
    }
    match ir.dynamic_sites().len() {
        0 => Ok(()),
        sites => Err(Ineligible::DynamicSql { sites }),
    }
}

/// The same gate over what `ekos recover` recorded on a `Procedure` object, for a caller that has
/// the ledger and not the source. Anything missing or unreadable is ineligible: the label is what
/// lets a routine through, and an absent label is not one.
pub fn recorded_gate(
    properties: &serde_json::Map<String, serde_json::Value>,
) -> Result<(), Ineligible> {
    let count = |key: &str| properties.get(key).and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    let fidelity = properties
        .get("fidelity")
        .map(|v| match v {
            serde_json::Value::String(s) => s.clone(),
            other => other
                .get("fidelity")
                .and_then(|f| f.as_str())
                .unwrap_or_default()
                .to_string(),
        })
        .unwrap_or_default();
    match fidelity.as_str() {
        "statements" => {}
        "partial" => {
            return Err(Ineligible::Partial {
                recovered: count("statements_recovered"),
                unrecovered: count("statements_unrecovered"),
            });
        }
        _ => return Err(Ineligible::SignatureOnly),
    }
    if count("statements_unrecovered") > 0 {
        return Err(Ineligible::Partial {
            recovered: count("statements_recovered"),
            unrecovered: count("statements_unrecovered"),
        });
    }
    match count("dynamic_sql_sites") {
        0 => Ok(()),
        sites => Err(Ineligible::DynamicSql { sites }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_plpgsql::parse_function;
    use serde_json::json;

    fn gate(body: &str, language: &str) -> Result<(), Ineligible> {
        let src = format!("CREATE FUNCTION f() RETURNS int AS $$ {body} $$ LANGUAGE {language};");
        routine_gate(&parse_function(&src).unwrap())
    }

    #[test]
    fn only_a_fully_recovered_static_routine_passes() {
        assert_eq!(gate("BEGIN RETURN 1; END;", "plpgsql"), Ok(()));
        assert_eq!(
            gate("import os\nreturn 1", "plpython3u"),
            Err(Ineligible::SignatureOnly)
        );
        assert_eq!(
            gate(
                "BEGIN EXECUTE 'DELETE FROM ' || t; RETURN 1; END;",
                "plpgsql"
            ),
            Err(Ineligible::DynamicSql { sites: 1 })
        );
    }

    #[test]
    fn a_partial_recovery_is_refused_with_its_counts() {
        let ir = ekos_plpgsql::ProcedureIr::new(
            ekos_plpgsql::ProcSignature {
                name: "f".into(),
                arguments: vec![],
                returns: None,
                language: "plpgsql".into(),
            },
            vec![],
            vec![
                ekos_plpgsql::ProcStmt::Return {
                    value: Some("1".into()),
                    query: None,
                    next: false,
                    span: ekos_plpgsql::Span { start: 0, end: 1 },
                },
                ekos_plpgsql::ProcStmt::Unrecovered {
                    raw: "???".into(),
                    reason: "test".into(),
                    span: ekos_plpgsql::Span { start: 1, end: 2 },
                },
            ],
        );
        let err = routine_gate(&ir).unwrap_err();
        assert_eq!(
            err,
            Ineligible::Partial {
                recovered: 1,
                unrecovered: 1
            }
        );
        assert_eq!(err.describe(), "1 of 2 statements were not recovered");
    }

    #[test]
    fn the_recorded_label_is_gated_the_same_way_and_a_missing_one_fails() {
        let props = |v: serde_json::Value| v.as_object().unwrap().clone();
        assert_eq!(
            recorded_gate(&props(
                json!({"fidelity": "statements", "dynamic_sql_sites": 0})
            )),
            Ok(())
        );
        assert_eq!(
            recorded_gate(&props(
                json!({"fidelity": "statements", "dynamic_sql_sites": 2})
            )),
            Err(Ineligible::DynamicSql { sites: 2 })
        );
        assert_eq!(
            recorded_gate(&props(json!({
                "fidelity": "partial", "statements_recovered": 41, "statements_unrecovered": 3
            }))),
            Err(Ineligible::Partial {
                recovered: 41,
                unrecovered: 3
            })
        );
        // A label that says `statements` beside a non-zero gap count is not trusted.
        assert!(
            recorded_gate(&props(json!({
                "fidelity": "statements", "statements_unrecovered": 1
            })))
            .is_err()
        );
        assert_eq!(
            recorded_gate(&props(json!({}))),
            Err(Ineligible::SignatureOnly)
        );
        assert_eq!(
            recorded_gate(&props(json!({"fidelity": "signature"}))),
            Err(Ineligible::SignatureOnly)
        );
    }
}
