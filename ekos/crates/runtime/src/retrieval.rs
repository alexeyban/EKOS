//! RFC 0121 (Phase 2 of RFC 0118) — query understanding.
//!
//! Turns a raw question into a [`QueryUnderstanding`]: the entity mentions it names (resolved to
//! real objects), the significant keywords, and the *shape* of answer it wants ([`QueryType`]).
//! Fully offline — hand-rolled mention extraction + `ekos_identity` fuzzy string match + intent
//! rules, no LLM. This is the input the Query Planner (RFC 0123) will consume; nothing routes
//! through it yet.

use crate::ai::{QUESTION_STOPWORDS, extract_search_terms};
use crate::{RetrievalRequest, Runtime, RuntimeError};
use ekos_identity::similarity::{jaro_winkler, normalize};
use ekos_kir::{KirId, ObjectKind};
use serde::Serialize;

/// A resolved-name confidence at or above this counts as "the query names this entity".
pub const RESOLVE_THRESHOLD: f32 = 0.82;
/// How many BM25 hits per mention to score for resolution.
const RESOLVE_CANDIDATES: usize = 8;

/// The shape of answer a question wants — the Query Planner's routing key (RFC 0123).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum QueryType {
    /// A bare id or a single exact entity name — fetch its state, don't search.
    Lookup,
    /// Keywords — BM25 is the primary arm.
    Lexical,
    /// A natural-language question with no dominant entity — semantic + BM25.
    Conceptual,
    /// "what depends on X", "callers of Y", "what breaks if…" — traverse the graph.
    Structural,
    /// "how many", "list all … by …" — hand back to EKL `COUNT` / `GROUP BY`.
    Aggregate,
}

/// A named graph operation a `Structural` query resolves to (RFC 0122 exposes these as a surface).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum StructuralOp {
    Dependents,
    Dependencies,
    Callers,
    Neighborhood,
    Impact,
}

/// An entity mention in the query, resolved to a real object.
#[derive(Debug, Clone)]
pub struct ResolvedEntity {
    /// The substring the user actually typed.
    pub mention: String,
    pub id: KirId,
    pub name: String,
    pub kind: Option<ObjectKind>,
    /// `1.0` for an exact case-insensitive name match, else
    /// `jaro_winkler(normalize(mention), normalize(name))`.
    pub confidence: f32,
}

/// The result of [`understand`].
#[derive(Debug, Clone)]
pub struct QueryUnderstanding {
    pub raw: String,
    pub query_type: QueryType,
    pub keywords: Vec<String>,
    /// Best match first.
    pub resolved_entities: Vec<ResolvedEntity>,
    /// Set iff `query_type == Structural`.
    pub structural_op: Option<StructuralOp>,
}

impl QueryUnderstanding {
    /// The single most-confident resolved entity, if any cleared the threshold.
    pub fn primary_entity(&self) -> Option<&ResolvedEntity> {
        self.resolved_entities.first()
    }
}

/// Understand `raw` against the ledger the `runtime` wraps. Never fails on a bad question — an
/// unclassifiable query is `Lexical` with whatever keywords survive.
pub fn understand(raw: &str, runtime: &Runtime) -> Result<QueryUnderstanding, RuntimeError> {
    let keywords = extract_search_terms(raw);
    // Structured mentions (quoted / dotted / CamelCase / snake) are the strong signal; fall back
    // to the significant keywords as weaker candidates so a bare-word entity ("the orders table")
    // still resolves. Resolution's confidence threshold drops the non-entity words.
    let mut candidates = extract_mentions(raw);
    for kw in &keywords {
        if kw.len() >= 3 && !candidates.iter().any(|c| c.eq_ignore_ascii_case(kw)) {
            candidates.push(kw.clone());
        }
    }
    candidates.truncate(8);
    let resolved = resolve_entities(&candidates, runtime)?;
    let (query_type, structural_op) = classify_intent(raw, &resolved);
    Ok(QueryUnderstanding {
        raw: raw.to_string(),
        query_type,
        keywords,
        resolved_entities: resolved,
        structural_op,
    })
}

// ── mention extraction ─────────────────────────────────────────────────────

/// Candidate entity mentions in `text`, most-specific first, de-duplicated (case-insensitively).
/// Heuristic, not NER: quoted / backticked spans, dotted paths, CamelCase, snake/kebab
/// identifiers. A missed mention just falls back to keyword search downstream.
pub fn extract_mentions(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |m: &str| {
        let m = m
            .trim()
            .trim_matches(|c: char| matches!(c, '(' | ')' | ',' | '.' | '?' | '!' | ':'));
        if m.len() < 2 {
            return;
        }
        if QUESTION_STOPWORDS.contains(&m.to_lowercase().as_str()) {
            return;
        }
        if !out.iter().any(|e| e.eq_ignore_ascii_case(m)) {
            out.push(m.to_string());
        }
    };

    // 0. RFC 0144: `RFC 13` / `RFC-0013` / `rfc 0013` → the canonical `RFC 0013`, resolved against
    // a Document's `rfc_number` in `resolve_entities` rather than by fuzzy name similarity.
    for number in rfc_numbers(text) {
        push(&format!("RFC {number}"));
    }

    // 1. quoted / backticked spans
    for (open, close) in [('\'', '\''), ('"', '"'), ('`', '`')] {
        let mut rest = text;
        while let Some(i) = rest.find(open) {
            let after = &rest[i + open.len_utf8()..];
            if let Some(j) = after.find(close) {
                push(&after[..j]);
                rest = &after[j + close.len_utf8()..];
            } else {
                break;
            }
        }
    }

    // 2. whitespace tokens: dotted paths, CamelCase, snake/kebab
    for tok in text.split_whitespace() {
        let t =
            tok.trim_matches(|c: char| !c.is_alphanumeric() && c != '.' && c != '_' && c != '-');
        if t.len() < 2 || !rfc_numbers(t).is_empty() {
            continue;
        }
        let has_dot = t.contains('.') && t.split('.').all(|p| !p.is_empty());
        let is_camel = {
            let mut chars = t.chars();
            let first_upper = chars.next().is_some_and(|c| c.is_uppercase());
            // A lowercase letter is required too: an acronym ("AI", "MCP", "SQL") is not an
            // identifier, and "AI" once resolved exactly (conf 1.0) to a minified JS symbol `aI`.
            first_upper
                && t.chars().skip(1).any(|c| c.is_uppercase())
                && t.chars().any(|c| c.is_lowercase())
                && t.chars().all(|c| c.is_alphanumeric())
        };
        let is_ident = (t.contains('_') || t.contains('-'))
            && t.chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == '-');
        if has_dot || is_camel || is_ident {
            push(t);
        }
    }

    out
}

/// RFC 0144: every `RFC NNN(N)` number in `text` (case-insensitive, space/`-` separated), zero-padded
/// to 4 digits — the same normalization `LocalDocAnalyzerPass` writes into `rfc_number`.
fn rfc_numbers(text: &str) -> Vec<String> {
    let lower = text.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let mut out: Vec<String> = Vec::new();
    let mut from = 0;
    while let Some(pos) = lower[from..].find("rfc") {
        let start = from + pos;
        from = start + 3;
        if start > 0 && bytes[start - 1].is_ascii_alphanumeric() {
            continue;
        }
        let mut j = from;
        while j < bytes.len() && matches!(bytes[j], b' ' | b'-') {
            j += 1;
        }
        let digits_start = j;
        while j < bytes.len() && bytes[j].is_ascii_digit() {
            j += 1;
        }
        let digits = &lower[digits_start..j];
        if (1..=4).contains(&digits.len())
            && (j >= bytes.len() || !bytes[j].is_ascii_alphanumeric())
            && let Ok(n) = digits.parse::<u32>()
        {
            let number = format!("{n:04}");
            if !out.contains(&number) {
                out.push(number);
            }
        }
    }
    out
}

/// Confidence given to an RFC mention matched by `rfc_number` — a structural, not fuzzy, match,
/// but kept below 1.0 so a question merely *containing* "RFC 0013" is never classified as a bare
/// `Lookup` of the document.
const RFC_MATCH_CONFIDENCE: f32 = 0.95;

// ── entity resolution ──────────────────────────────────────────────────────

fn resolve_entities(
    mentions: &[String],
    runtime: &Runtime,
) -> Result<Vec<ResolvedEntity>, RuntimeError> {
    let mut resolved: Vec<ResolvedEntity> = Vec::new();
    for mention in mentions {
        let hits = runtime
            .retrieve(&RetrievalRequest::lexical(mention.clone()))?
            .hits;
        // RFC 0144: an `RFC NNNN` mention resolves only to the Document whose `rfc_number`
        // matches — never by name similarity, which can't tell `0013` from `0143`.
        if let Some(number) = mention.strip_prefix("RFC ") {
            // Search the bare number: an RFC's path (`docs/rfcs/0013-…`) tokenizes to `rfcs`, which
            // an unstemmed backend (SQLite FTS5) never matches against `RFC`.
            let number_hits = runtime
                .retrieve(&RetrievalRequest::lexical(number.to_string()))?
                .hits;
            for h in number_hits
                .into_iter()
                .chain(hits)
                .take(RESOLVE_CANDIDATES * 4)
            {
                let Some(obj) = runtime.load_object(&h.id)? else {
                    continue;
                };
                let is_match = matches!(&obj.kind, ObjectKind::Custom(k) if k == "Document")
                    && obj.properties.get("rfc_number").and_then(|v| v.as_str()) == Some(number);
                if is_match {
                    resolved.push(ResolvedEntity {
                        mention: mention.clone(),
                        id: h.id,
                        name: obj.name,
                        kind: Some(obj.kind),
                        confidence: RFC_MATCH_CONFIDENCE,
                    });
                    break;
                }
            }
            continue;
        }
        let mention_norm = normalize(mention);
        let mention_lc = mention.trim().to_lowercase();
        let best = hits
            .into_iter()
            .take(RESOLVE_CANDIDATES)
            .map(|h| {
                let conf = if h.name.trim().to_lowercase() == mention_lc {
                    1.0
                } else {
                    jaro_winkler(&mention_norm, &normalize(&h.name))
                };
                (h, conf)
            })
            .filter(|(_, c)| *c >= RESOLVE_THRESHOLD)
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        if let Some((h, confidence)) = best {
            resolved.push(ResolvedEntity {
                mention: mention.clone(),
                id: h.id,
                name: h.name,
                kind: h.kind,
                confidence,
            });
        }
    }
    resolved.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(resolved)
}

// ── intent classification ──────────────────────────────────────────────────

fn looks_like_uuid(s: &str) -> bool {
    let s = s.trim();
    s.len() == 36
        && s.chars().enumerate().all(|(i, c)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                c == '-'
            } else {
                c.is_ascii_hexdigit()
            }
        })
}

fn classify_intent(raw: &str, resolved: &[ResolvedEntity]) -> (QueryType, Option<StructuralOp>) {
    let q = raw.trim().to_lowercase();

    // Lookup: a bare id, or the whole query IS one exact-resolved entity.
    if looks_like_uuid(&q) {
        return (QueryType::Lookup, None);
    }
    if let Some(e) = resolved.first()
        && e.confidence >= 0.999
        && e.mention.trim().to_lowercase() == q
    {
        return (QueryType::Lookup, None);
    }

    // Aggregate.
    for p in ["how many", "count ", "number of", "list all", "how much"] {
        if q.starts_with(p) || q.contains(&format!(" {p}")) {
            return (QueryType::Aggregate, None);
        }
    }

    // Structural — order matters (most specific verb phrases first).
    let structural: &[(&[&str], StructuralOp)] = &[
        (
            &["what breaks if", "impact of", "affected by", "blast radius"],
            StructuralOp::Impact,
        ),
        (
            &["callers of", "who calls", "what calls", "called by"],
            StructuralOp::Callers,
        ),
        (
            &["dependencies of", "what does", "depends on what"],
            StructuralOp::Dependencies,
        ),
        (
            &[
                "depends on",
                "dependents of",
                "what uses",
                "used by",
                "what depends",
            ],
            StructuralOp::Dependents,
        ),
        (
            &[
                "related to",
                "connected to",
                "neighbours of",
                "neighbors of",
                "near ",
            ],
            StructuralOp::Neighborhood,
        ),
    ];
    for (phrases, op) in structural {
        if phrases.iter().any(|p| q.contains(*p)) {
            return (QueryType::Structural, Some(*op));
        }
    }

    // Conceptual: a question with no single dominant entity.
    let is_question = q.ends_with('?')
        || [
            "how ", "why ", "what ", "when ", "where ", "who ", "explain", "describe",
        ]
        .iter()
        .any(|p| q.starts_with(p));
    let dominant_entity = resolved.first().is_some_and(|e| e.confidence >= 0.95);
    if is_question && !dominant_entity {
        return (QueryType::Conceptual, None);
    }

    (QueryType::Lexical, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_kir::{KirObject, ObjectKind};
    use tempfile::TempDir;

    fn temp_ledger() -> (ekos_ledger::Ledger, TempDir) {
        let dir = TempDir::new().unwrap();
        (
            ekos_ledger::Ledger::open(&dir.path().join("l.db")).unwrap(),
            dir,
        )
    }

    #[test]
    fn mention_extraction_table() {
        let cases: &[(&str, &[&str])] = &[
            ("what does `authenticate()` return", &["authenticate"]),
            ("\"orders\" table columns", &["orders"]),
            ("UserService dependencies", &["UserService"]),
            (
                "plausible.billing.subscription status",
                &["plausible.billing.subscription"],
            ),
            ("the order_items rows", &["order_items"]),
            ("how does the system work", &[]),
            // RFC 0144: acronyms are not CamelCase identifiers; RFC numbers are canonicalized.
            ("what does it expose to AI agents over MCP", &[]),
            (
                "What does RFC 13 expose, and does rfc-0015 build on it?",
                &["RFC 0013", "RFC 0015"],
            ),
        ];
        for (input, want) in cases {
            let got = extract_mentions(input);
            assert_eq!(got, *want, "input {input:?}");
        }
    }

    #[test]
    fn resolves_exact_and_fuzzy_names() {
        let (l, _d) = temp_ledger();
        for n in ["UserService", "OrderService", "billing_report"] {
            l.append_object(&KirObject::new(n, ObjectKind::Custom("Symbol".into())))
                .unwrap();
        }
        let rt = Runtime::new(&l);

        let exact = resolve_entities(&["UserService".into()], &rt).unwrap();
        assert_eq!(exact.len(), 1);
        assert_eq!(exact[0].name, "UserService");
        assert_eq!(exact[0].confidence, 1.0);

        let fuzzy = resolve_entities(&["userservice".into()], &rt).unwrap();
        assert_eq!(fuzzy[0].name, "UserService");
        assert_eq!(fuzzy[0].confidence, 1.0, "case-insensitive exact");

        let none = resolve_entities(&["totally_unrelated_xyz".into()], &rt).unwrap();
        assert!(none.is_empty());
    }

    #[test]
    fn an_rfc_mention_resolves_to_its_document_by_rfc_number_only() {
        let (l, _d) = temp_ledger();
        let rfc13 = KirObject::new(
            "docs/rfcs/0013-mcp-server.md",
            ObjectKind::Custom("Document".into()),
        )
        .with_property("rfc_number", serde_json::json!("0013"));
        let rfc143 = KirObject::new(
            "ekos/docs/rfcs/0143-mcp-http-transport.md",
            ObjectKind::Custom("Document".into()),
        )
        .with_property("rfc_number", serde_json::json!("0143"))
        .with_property("excerpt", serde_json::json!("builds on RFC 0013"));
        let file13 = KirObject::new("docs/rfcs/0013-mcp-server.md", ObjectKind::File);
        for o in [&rfc143, &file13, &rfc13] {
            l.append_object(o).unwrap();
        }
        let rt = Runtime::new(&l);

        let got = resolve_entities(&["RFC 0013".into()], &rt).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(
            got[0].id, rfc13.id,
            "the Document, not the File or RFC 0143"
        );
        assert_eq!(got[0].confidence, RFC_MATCH_CONFIDENCE);

        assert!(
            resolve_entities(&["RFC 0099".into()], &rt)
                .unwrap()
                .is_empty()
        );

        let u = understand("What does RFC 0013 expose to AI agents?", &rt).unwrap();
        assert_eq!(u.resolved_entities[0].id, rfc13.id);
        assert_ne!(u.query_type, QueryType::Lookup);
    }

    #[test]
    fn intent_table() {
        let cases: &[(&str, QueryType, Option<StructuralOp>)] = &[
            ("how many tables are there", QueryType::Aggregate, None),
            ("list all modules by language", QueryType::Aggregate, None),
            (
                "what depends on the orders table",
                QueryType::Structural,
                Some(StructuralOp::Dependents),
            ),
            (
                "callers of authenticate",
                QueryType::Structural,
                Some(StructuralOp::Callers),
            ),
            (
                "what breaks if we drop customers",
                QueryType::Structural,
                Some(StructuralOp::Impact),
            ),
            (
                "dependencies of the billing module",
                QueryType::Structural,
                Some(StructuralOp::Dependencies),
            ),
            (
                "what is related to the session store",
                QueryType::Structural,
                Some(StructuralOp::Neighborhood),
            ),
            ("how does authentication work", QueryType::Conceptual, None),
            ("why is the report slow?", QueryType::Conceptual, None),
            ("session timeout config", QueryType::Lexical, None),
            (
                "3f8a1c2d-0000-4000-8000-000000000000",
                QueryType::Lookup,
                None,
            ),
        ];
        for (q, want_type, want_op) in cases {
            let (t, op) = classify_intent(q, &[]);
            assert_eq!(t, *want_type, "query {q:?}");
            assert_eq!(op, *want_op, "query {q:?} op");
        }
    }

    #[test]
    fn understand_end_to_end() {
        let (l, _d) = temp_ledger();
        l.append_object(&KirObject::new("orders", ObjectKind::Table))
            .unwrap();
        let rt = Runtime::new(&l);
        let u = understand("what depends on the orders table", &rt).unwrap();
        assert_eq!(u.query_type, QueryType::Structural);
        assert_eq!(u.structural_op, Some(StructuralOp::Dependents));
        assert_eq!(u.primary_entity().map(|e| e.name.as_str()), Some("orders"));
        assert!(u.keywords.contains(&"orders".to_string()));
    }
}
