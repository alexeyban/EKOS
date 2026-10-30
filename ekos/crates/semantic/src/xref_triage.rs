//! RFC 0233 Phase 4 — optional LLM triage for findings the graph cannot settle.
//!
//! For an `AmbiguousReference` (which candidate?) or a `DynamicReferenceUnknown` (who calls it?),
//! a model may *suggest* an answer. Pure here: the prompt, and the judge that keeps an answer only
//! when it names one of the objects it was shown and cites them by id (cite-or-drop, the RFC 0170
//! Phase 3 rule). A suggestion never changes a finding's kind, severity or confidence, and nothing
//! here writes anything: the CLI prints it, labelled as a model's suggestion.

use crate::xref::{Diagnostic, DiagnosticKind};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Bump when the prompt or the judge changes.
pub const PROMPT_VERSION: &str = "xref-triage/v1";

pub const SYSTEM: &str = "You help triage one finding of a code-and-schema integrity check. You are \
shown the finding and a numbered list of compiled objects. Answer with ONE JSON object and nothing \
else: {\"suggestion\": \"<the exact name of one listed object, or empty>\", \"role\": \"target\" | \
\"entry_point\" | \"none\", \"rationale\": \"<one or two sentences>\", \"cites\": [\"<id of every \
listed object you rely on>\"]}. Only name an object from the list. If the list does not let you \
decide, answer {\"suggestion\": \"\", \"role\": \"none\", \"rationale\": \"<why>\", \"cites\": []}.";

/// One object the model may name: a candidate of an ambiguous reference, or a search hit for a
/// routine's name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextItem {
    pub id: String,
    pub kind: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
}

/// Which findings triage applies to.
pub fn eligible(d: &Diagnostic) -> bool {
    matches!(
        d.kind,
        DiagnosticKind::AmbiguousReference | DiagnosticKind::DynamicReferenceUnknown
    )
}

/// The user message for one finding.
pub fn prompt(d: &Diagnostic, context: &[ContextItem]) -> String {
    let question = match d.kind {
        DiagnosticKind::AmbiguousReference => format!(
            "`{}` ({} at {}) names {} compiled objects. Which one does it most likely mean? \
             role = \"target\".",
            d.target_spec.written,
            d.source.object_name,
            location(d),
            context.len()
        ),
        _ => format!(
            "No entry point reaches routine `{}` ({}). Which listed object most likely calls or \
             runs it? role = \"entry_point\".",
            d.source.object_name,
            location(d)
        ),
    };
    let mut out = format!(
        "{question}\nReason the check gave: {}\n\nObjects:\n",
        d.reason
    );
    for c in context {
        out.push_str(&format!(
            "- id={} kind={} name={}{}\n",
            c.id,
            c.kind,
            c.name,
            c.file
                .as_deref()
                .map(|f| format!(" file={f}"))
                .unwrap_or_default()
        ));
    }
    out
}

fn location(d: &Diagnostic) -> String {
    match (&d.source.file, d.source.line) {
        (Some(f), Some(l)) => format!("{f}:{l}"),
        (Some(f), None) => f.clone(),
        _ => d.source.object_kind.clone(),
    }
}

/// What the judge made of the model's answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum Triage {
    /// A suggestion that names a shown object and cites it. Never a fact.
    Suggested {
        suggestion: String,
        role: String,
        rationale: String,
        cites: Vec<ContextItem>,
    },
    /// The model said the list does not decide it.
    Undecided { rationale: String },
    /// The answer was not usable: not JSON, an unknown name, or uncited.
    Dropped { why: String },
}

#[derive(Deserialize)]
struct Answer {
    #[serde(default)]
    suggestion: String,
    #[serde(default)]
    role: String,
    #[serde(default)]
    rationale: String,
    #[serde(default)]
    cites: Vec<String>,
}

/// Keep the answer only if it names one shown object and cites shown objects only.
pub fn judge(context: &[ContextItem], answer: &str) -> Triage {
    let text = answer.trim();
    let json = text
        .find('{')
        .and_then(|a| text.rfind('}').map(|b| &text[a..=b]))
        .unwrap_or(text);
    let Ok(a) = serde_json::from_str::<Answer>(json) else {
        return Triage::Dropped {
            why: "the answer was not one JSON object".into(),
        };
    };
    if a.suggestion.trim().is_empty() || a.role == "none" {
        return Triage::Undecided {
            rationale: a.rationale,
        };
    }
    let by_id: BTreeMap<&str, &ContextItem> = context.iter().map(|c| (c.id.as_str(), c)).collect();
    let Some(cites) = a
        .cites
        .iter()
        .map(|id| by_id.get(id.as_str()).map(|c| (*c).clone()))
        .collect::<Option<Vec<_>>>()
    else {
        return Triage::Dropped {
            why: "it cited an object it was not shown".into(),
        };
    };
    let named = cites
        .iter()
        .any(|c| c.name.eq_ignore_ascii_case(a.suggestion.trim()));
    if !named {
        return Triage::Dropped {
            why: format!(
                "`{}` is not one of the objects it cited",
                a.suggestion.trim()
            ),
        };
    }
    if !matches!(a.role.as_str(), "target" | "entry_point") {
        return Triage::Dropped {
            why: format!("unknown role `{}`", a.role),
        };
    }
    Triage::Suggested {
        suggestion: a.suggestion.trim().to_string(),
        role: a.role,
        rationale: a.rationale,
        cites,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> Vec<ContextItem> {
        vec![
            ContextItem {
                id: "a".into(),
                kind: "Table".into(),
                name: "sales.customers".into(),
                file: Some("sql/sales.sql".into()),
            },
            ContextItem {
                id: "b".into(),
                kind: "Table".into(),
                name: "crm.customers".into(),
                file: None,
            },
        ]
    }

    #[test]
    fn a_cited_suggestion_from_the_list_is_kept() {
        let t = judge(
            &ctx(),
            r#"Sure: {"suggestion":"sales.customers","role":"target","rationale":"same file","cites":["a"]}"#,
        );
        let Triage::Suggested {
            suggestion, cites, ..
        } = t
        else {
            panic!("{t:?}")
        };
        assert_eq!(suggestion, "sales.customers");
        assert_eq!(cites[0].id, "a");
    }

    #[test]
    fn inventions_and_uncited_answers_are_dropped() {
        for answer in [
            r#"{"suggestion":"public.customers","role":"target","rationale":"x","cites":["a"]}"#,
            r#"{"suggestion":"sales.customers","role":"target","rationale":"x","cites":["zz"]}"#,
            r#"{"suggestion":"sales.customers","role":"target","rationale":"x","cites":[]}"#,
            r#"{"suggestion":"sales.customers","role":"owner","rationale":"x","cites":["a"]}"#,
            "it is probably the sales one",
        ] {
            assert!(
                matches!(judge(&ctx(), answer), Triage::Dropped { .. }),
                "{answer}"
            );
        }
    }

    #[test]
    fn saying_the_list_does_not_decide_is_kept_as_undecided() {
        let t = judge(
            &ctx(),
            r#"{"suggestion":"","role":"none","rationale":"both plausible","cites":[]}"#,
        );
        assert_eq!(
            t,
            Triage::Undecided {
                rationale: "both plausible".into()
            }
        );
    }
}
