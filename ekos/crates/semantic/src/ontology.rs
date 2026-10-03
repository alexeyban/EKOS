//! RFC 0170 Phase 4 — ontology mapping **suggestions** (hypotheses only).
//!
//! The vocabulary is the user's: a file named by `[semantics] ontology`, listing terms with an id
//! (a CURIE or URI), a label and optional synonyms. EKOS ships no vocabulary and invents no URI.
//! A suggestion is made only on exact word equality (`words_key`): a label match is an *exact*
//! suggestion, a synonym match a *close* one. Suggestions never become LinkML `exact_mappings`
//! on their own — they are exported as `ekos_suggested_*` annotations for a human to adopt.

use crate::business_semantics::words_key;
use serde::{Deserialize, Serialize};

/// One vocabulary term.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OntologyTerm {
    /// `schema:Invoice` or a full URI.
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub synonyms: Vec<String>,
}

/// A vocabulary file: `terms:` (and optional `prefixes:`, kept for the export).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Vocabulary {
    #[serde(default)]
    pub prefixes: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub terms: Vec<OntologyTerm>,
}

/// One suggested mapping.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Suggestion {
    pub id: String,
    pub label: String,
    /// `exact` (label) or `close` (synonym).
    pub match_type: String,
}

impl Vocabulary {
    /// Suggestions for something named `name` (any spelling: `OpenOrder`, `open_order`, …).
    pub fn suggest(&self, name: &str) -> Vec<Suggestion> {
        let key = words_key(name);
        if key.is_empty() {
            return Vec::new();
        }
        let mut out: Vec<Suggestion> = Vec::new();
        for t in &self.terms {
            let match_type = if words_key(&t.label) == key {
                "exact"
            } else if t.synonyms.iter().any(|s| words_key(s) == key) {
                "close"
            } else {
                continue;
            };
            out.push(Suggestion {
                id: t.id.clone(),
                label: t.label.clone(),
                match_type: match_type.into(),
            });
        }
        out.sort_by(|a, b| (a.match_type.as_str(), &a.id).cmp(&(b.match_type.as_str(), &b.id)));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggestions_need_exact_words_and_say_how_they_matched() {
        let v = Vocabulary {
            prefixes: Default::default(),
            terms: vec![
                OntologyTerm {
                    id: "schema:Invoice".into(),
                    label: "Invoice".into(),
                    synonyms: vec!["bill".into()],
                },
                OntologyTerm {
                    id: "schema:Order".into(),
                    label: "Order".into(),
                    synonyms: vec![],
                },
            ],
        };
        assert_eq!(v.suggest("invoices")[0].match_type, "exact");
        assert_eq!(v.suggest("Bill")[0].id, "schema:Invoice");
        assert_eq!(v.suggest("Bill")[0].match_type, "close");
        assert!(v.suggest("order_line").is_empty(), "no partial matches");
        assert!(v.suggest("").is_empty());
    }
}
