//! RFC 0170 Phase 3 — constrained LLM definition text for business concepts.
//!
//! A concept's definition *is* its predicate (`parts.obsolete IS FALSE`); this step may add one or
//! two plain-language sentences for a reader, under the plan's rules:
//!
//! - the model sees only the concept's own evidence, numbered `E1…En`, and the predicate;
//! - every sentence must cite at least one evidence item it was given (`["E2"]`);
//! - a sentence with no valid citation is **dropped, not shown**, and counted;
//! - the text never changes the concept's status, confidence or signature: it is a reading aid
//!   on a hypothesis (`llm_definition`), and an expert's description always wins over it.
//!
//! Opt-in (`[semantics] llm-definitions = true`) because it is a model call per concept; with a
//! cached provider a re-run asks nothing and writes nothing new.

use crate::llm::{LlmProvider, LlmRequest};
use crate::llm_json::json_body;
use ekos_kir::{KirEvidence, KirObject};
use serde::Deserialize;
use serde_json::json;

const PROMPT_VERSION: &str = "semantics-definition/2";

const SYSTEM: &str = "You explain a business rule recovered from database code to a business reader. \
You are given the rule as a SQL predicate and numbered evidence lines (E1, E2, …) showing where the \
code uses it. Write at most two short sentences saying what rows the rule selects, in business terms. \
Use ONLY the evidence given; where evidence states what a code means, use that meaning. Do not \
guess or speculate: a sentence with \"likely\", \"probably\" or \"may\" is discarded. Every sentence MUST \
cite the evidence it rests on by number. Answer with JSON only: \
{\"sentences\": [{\"text\": \"…\", \"cites\": [\"E1\"]}]}";

/// What one run did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DefinitionStats {
    pub asked: usize,
    pub described: usize,
    /// Sentences dropped for citing nothing valid.
    pub sentences_dropped: usize,
    pub errors: usize,
    pub skipped_documented: usize,
}

#[derive(Deserialize)]
struct Reply {
    #[serde(default)]
    sentences: Vec<Sentence>,
}

#[derive(Deserialize)]
struct Sentence {
    text: String,
    #[serde(default)]
    cites: Vec<String>,
}

/// A sentence that hedges is a guess, whatever it cites.
fn hedges(text: &str) -> bool {
    let t = format!(" {} ", text.to_lowercase());
    [
        " likely ",
        " probably ",
        " possibly ",
        " presumably ",
        " perhaps ",
        " may be ",
        " might ",
        " could be ",
        " seems ",
        " appears to ",
        " likely,",
        " probably,",
    ]
    .iter()
    .any(|h| t.contains(h))
}

/// A kept sentence and the 1-based evidence numbers it cites.
pub type CitedSentence = (String, Vec<usize>);

/// The kept sentences (each with its valid citations) and how many were dropped.
pub fn keep_cited(reply: &str, evidence_count: usize) -> Option<(Vec<CitedSentence>, usize)> {
    let parsed: Reply = serde_json::from_str(json_body(reply)).ok()?;
    let mut kept = Vec::new();
    let mut dropped = 0;
    for s in parsed.sentences.into_iter().take(8) {
        let cites: Vec<usize> = s
            .cites
            .iter()
            .filter_map(|c| {
                c.trim()
                    .trim_start_matches(['E', 'e'])
                    .parse::<usize>()
                    .ok()
            })
            .filter(|&n| n >= 1 && n <= evidence_count)
            .collect();
        let text = s.text.trim().to_string();
        if cites.is_empty() || text.is_empty() || text.len() > 400 || hedges(&text) {
            dropped += 1;
        } else {
            kept.push((text, cites));
        }
    }
    Some((kept.into_iter().take(2).collect(), dropped))
}

/// Add `llm_definition` to every concept in `concepts` without an author's or expert's
/// description, at most `max` of them. `evidence_of` returns a concept's own evidence records.
pub async fn describe_concepts(
    provider: &dyn LlmProvider,
    concepts: &mut [KirObject],
    evidence_of: impl Fn(&KirObject) -> Vec<KirEvidence>,
    max: usize,
) -> DefinitionStats {
    let mut stats = DefinitionStats::default();
    for c in concepts.iter_mut() {
        let documented = ["expert_description", "description"].iter().any(|k| {
            c.properties
                .get(*k)
                .and_then(|v| v.as_str())
                .is_some_and(|s| !s.is_empty())
        });
        if documented {
            stats.skipped_documented += 1;
            continue;
        }
        if stats.asked >= max {
            break;
        }
        let evidence: Vec<KirEvidence> = evidence_of(c).into_iter().take(14).collect();
        if evidence.is_empty() {
            continue;
        }
        let definition = c
            .properties
            .get("definition")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let lines: Vec<String> = evidence
            .iter()
            .enumerate()
            .map(|(i, e)| {
                format!(
                    "E{}: {}{} — {}",
                    i + 1,
                    e.location.path,
                    e.location.line.map(|l| format!(":{l}")).unwrap_or_default(),
                    e.fragment
                )
            })
            .collect();
        let user = format!(
            "Rule: {definition}\nNamed (derived from the predicate, not by a person): {}\nEvidence:\n{}",
            c.name,
            lines.join("\n")
        );
        stats.asked += 1;
        let resp = match provider
            .complete(&LlmRequest {
                system: SYSTEM,
                user: &user,
                prompt_version: PROMPT_VERSION,
                max_tokens: 400,
                history: &[],
            })
            .await
        {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(concept = %c.name, "llm definition failed: {e}");
                stats.errors += 1;
                continue;
            }
        };
        let Some((kept, dropped)) = keep_cited(&resp.content, evidence.len()) else {
            stats.errors += 1;
            continue;
        };
        stats.sentences_dropped += dropped;
        if kept.is_empty() {
            continue;
        }
        let citations: Vec<serde_json::Value> = kept
            .iter()
            .map(|(text, cites)| {
                json!({
                    "text": text,
                    "evidence": cites.iter().map(|&n| {
                        let e = &evidence[n - 1];
                        match e.location.line {
                            Some(l) => format!("{}:{l}", e.location.path),
                            None => e.location.path.clone(),
                        }
                    }).collect::<Vec<_>>(),
                })
            })
            .collect();
        let text = kept
            .iter()
            .map(|(t, _)| t.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        c.properties.insert("llm_definition".into(), json!(text));
        c.properties
            .insert("llm_citations".into(), json!(citations));
        c.properties.insert("llm_model".into(), json!(resp.model));
        if dropped > 0 {
            c.properties
                .insert("llm_sentences_dropped".into(), json!(dropped));
        }
        stats.described += 1;
    }
    stats
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::MockLlmProvider;
    use ekos_kir::{ObjectKind, SourceLocation};

    #[test]
    fn uncited_or_miscited_sentences_are_dropped() {
        let reply = r#"{"sentences": [
            {"text": "Parts that are still sold.", "cites": ["E1"]},
            {"text": "Probably used for the catalogue.", "cites": []},
            {"text": "Cites evidence it was never given.", "cites": ["E9"]},
            {"text": "Also counted in stock reports.", "cites": ["e2", "E7"]},
            {"text": "These are likely Quality accounts.", "cites": ["E1"]}
        ]}"#;
        let (kept, dropped) = keep_cited(reply, 2).unwrap();
        assert_eq!(
            kept,
            vec![
                ("Parts that are still sold.".to_string(), vec![1]),
                ("Also counted in stock reports.".to_string(), vec![2]),
            ]
        );
        assert_eq!(dropped, 3, "uncited, miscited and hedged");
        assert!(keep_cited("not json", 2).is_none());
    }

    fn concept(described: bool) -> KirObject {
        let mut o = KirObject::new("PartsNotObsolete", ObjectKind::BusinessConcept);
        o.properties
            .insert("definition".into(), json!("parts.obsolete IS FALSE"));
        if described {
            o.properties
                .insert("expert_description".into(), json!("Sellable part."));
        }
        o
    }

    #[tokio::test]
    async fn only_undocumented_concepts_are_described_with_citations() {
        let provider = MockLlmProvider::new(
            r#"{"sentences": [{"text": "Parts not marked obsolete.", "cites": ["E1"]}]}"#,
        );
        let mut items = vec![concept(false), concept(true)];
        let ev = |_: &KirObject| {
            vec![KirEvidence::new(
                SourceLocation::at("sql/Parts.sql", 12),
                "parts.obsolete IS FALSE (parts__list, Where)",
            )]
        };
        let stats = describe_concepts(&provider, &mut items, ev, 10).await;
        assert_eq!(stats.described, 1);
        assert_eq!(stats.skipped_documented, 1);
        assert_eq!(
            items[0].properties["llm_definition"],
            json!("Parts not marked obsolete.")
        );
        assert_eq!(
            items[0].properties["llm_citations"][0]["evidence"],
            json!(["sql/Parts.sql:12"])
        );
        assert!(!items[1].properties.contains_key("llm_definition"));
    }
}
