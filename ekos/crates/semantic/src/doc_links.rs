//! RFC 0144 §3 — deterministic documentation links.
//!
//! Runs inside `SemanticCompilerPass::run()` on the fully resolved graph, right after RFC 0094's
//! `concentration_risks` and for the same reason: it needs every analyzer's output at once (the
//! `Section` objects from `LocalDocAnalyzerPass` *and* the code objects from the language
//! analyzers), and nothing that only exists after `commit`.
//!
//! Two link types, both `References` edges from a `Section`, both carrying the section's own
//! evidence:
//! - **`rfc`** — an `RFC NNNN` mention → the `Document` whose `rfc_number` matches.
//! - **`code`** — a backticked identifier → the *single* code object with exactly that name.
//!   An ambiguous name produces no edge (RFC 0060: no guessing).
//!
//! Pure and deterministic: same graph in, same edges (same ids, same order) out.

use ekos_kir::{KirGraph, KirId, KirObject, KirRelationship, ObjectKind, RelationshipKind};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Cap on edges emitted per section, so a reference table or an index page can't explode the graph.
pub const MAX_LINKS_PER_SECTION: usize = 50;

/// Backticked spans shorter than this are ignored — `id`, `new`, `run` are never informative.
const MIN_CODE_SPAN_CHARS: usize = 4;

/// Object kinds a backticked doc span may link to.
const CODE_KINDS: &[&str] = &[
    "RustSymbol",
    "RustModule",
    "PythonSymbol",
    "PythonModule",
    "JsModule",
    "JsSymbol",
    "ElixirModule",
    "ElixirSymbol",
    "PerlPackage",
    "PerlSymbol",
    "Crate",
];

/// Derive RFC→RFC and doc→code `References` edges for every `Section` in `graph`.
pub fn doc_links(graph: &KirGraph) -> Vec<KirRelationship> {
    let mut rfc_docs: HashMap<String, KirId> = HashMap::new();
    let mut code_by_name: HashMap<&str, Vec<KirId>> = HashMap::new();
    for obj in &graph.objects {
        match &obj.kind {
            ObjectKind::Custom(k) if k == "Document" => {
                if let Some(n) = str_prop(obj, "rfc_number") {
                    rfc_docs.insert(n.to_string(), obj.id);
                }
            }
            ObjectKind::Custom(k) if CODE_KINDS.contains(&k.as_str()) => {
                code_by_name
                    .entry(obj.name.as_str())
                    .or_default()
                    .push(obj.id);
            }
            _ => {}
        }
    }
    for ids in code_by_name.values_mut() {
        ids.sort_by_key(|id| id.0);
        ids.dedup();
    }

    let mut sections: Vec<&KirObject> = graph
        .objects
        .iter()
        .filter(|o| matches!(&o.kind, ObjectKind::Custom(k) if k == "Section"))
        .collect();
    sections.sort_by_key(|o| o.id.0);

    let mut out = Vec::new();
    for section in sections {
        let Some(text) = str_prop(section, "excerpt") else {
            continue;
        };
        let own_rfc = str_prop(section, "rfc_number");
        // BTreeMap keyed by target: de-duplicates and fixes emission order.
        let mut links: BTreeMap<(u128, &'static str), BTreeMap<&'static str, String>> =
            BTreeMap::new();

        for (number, relation) in rfc_mentions(text) {
            if own_rfc == Some(number.as_str()) {
                continue;
            }
            let Some(target) = rfc_docs.get(&number) else {
                continue;
            };
            let entry = links.entry((target.0.as_u128(), "rfc")).or_default();
            // A stronger relation found anywhere in the section wins over a bare mention.
            if entry.get("relation").is_none_or(|r| r == "mentions") {
                entry.insert("relation", relation.to_string());
            }
            entry.insert("rfc_number", number);
        }

        for span in code_spans(text) {
            let target = unique(&code_by_name, &span).or_else(|| {
                span.rsplit("::")
                    .next()
                    .filter(|last| *last != span && last.chars().count() >= MIN_CODE_SPAN_CHARS)
                    .and_then(|last| unique(&code_by_name, last))
            });
            if let Some(target) = target
                && target != section.id
            {
                links
                    .entry((target.0.as_u128(), "code"))
                    .or_default()
                    .insert("mention", span);
            }
        }

        for ((target, link_type), props) in links.into_iter().take(MAX_LINKS_PER_SECTION) {
            let to = KirId(uuid::Uuid::from_u128(target));
            let mut rel = KirRelationship::deterministic(
                RelationshipKind::References,
                section.id,
                to,
                link_type,
            );
            rel.properties
                .insert("link_type".into(), serde_json::json!(link_type));
            for (k, v) in props {
                rel.properties.insert(k.into(), serde_json::json!(v));
            }
            rel.evidence = section.evidence.clone();
            out.push(rel);
        }
    }
    out
}

fn str_prop<'a>(obj: &'a KirObject, key: &str) -> Option<&'a str> {
    obj.properties.get(key).and_then(|v| v.as_str())
}

fn unique(index: &HashMap<&str, Vec<KirId>>, name: &str) -> Option<KirId> {
    match index.get(name).map(Vec::as_slice) {
        Some([only]) => Some(*only),
        _ => None,
    }
}

/// Every `RFC NNNN` (also `RFC-NNNN`, `RFC NNN`) mention, zero-padded to 4 digits, with the
/// relation implied by its line: `depends_on` for "builds on"/"depends on"/"requires",
/// `supersedes` for "supersedes"/"replaces", else `mentions`.
fn rfc_mentions(text: &str) -> Vec<(String, &'static str)> {
    let mut found = Vec::new();
    for line in text.lines() {
        let lower = line.to_ascii_lowercase();
        let relation = if ["builds on", "depends on", "requires", "built on"]
            .iter()
            .any(|p| lower.contains(p))
        {
            "depends_on"
        } else if ["supersedes", "replaces"].iter().any(|p| lower.contains(p)) {
            "supersedes"
        } else {
            "mentions"
        };
        let bytes = line.as_bytes();
        let mut i = 0;
        while let Some(pos) = line[i..].find("RFC") {
            let start = i + pos;
            i = start + 3;
            // Word boundary before "RFC" (so "XRFC" doesn't match).
            if start > 0 && bytes[start - 1].is_ascii_alphanumeric() {
                continue;
            }
            let mut j = i;
            while j < bytes.len() && matches!(bytes[j], b' ' | b'-' | b'\t') {
                j += 1;
            }
            let digits_start = j;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                j += 1;
            }
            let digits = &line[digits_start..j];
            let boundary_after = j >= bytes.len() || !bytes[j].is_ascii_alphanumeric();
            if (3..=4).contains(&digits.len())
                && boundary_after
                && let Ok(n) = digits.parse::<u32>()
            {
                found.push((format!("{n:04}"), relation));
            }
        }
    }
    found
}

/// Backticked spans that look like an identifier or a `::`-path (`build_llm_provider`,
/// `ekos_common::redaction`, `ekos-ledger`). A trailing `()` is dropped. Spans with spaces,
/// file extensions, or other punctuation (`ekos build`, `transform_ir.rs`, `[llm]`) are skipped.
fn code_spans(text: &str) -> BTreeSet<String> {
    let mut spans = BTreeSet::new();
    for (i, piece) in text.split('`').enumerate() {
        if i % 2 == 0 {
            continue;
        }
        let span = piece.trim().trim_end_matches("()");
        let valid = span.chars().count() >= MIN_CODE_SPAN_CHARS
            && span
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && span
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | ':' | '-'))
            && !span.ends_with(':');
        if valid {
            spans.insert(span.to_string());
        }
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_kir::{KirEvidence, SourceLocation};

    fn obj(graph: &mut KirGraph, name: &str, kind: &str, props: &[(&str, &str)]) -> KirId {
        let mut o = KirObject::new(name, ObjectKind::Custom(kind.to_string()));
        o.id = KirId(uuid::Uuid::new_v5(
            &uuid::Uuid::NAMESPACE_URL,
            format!("test:{kind}:{name}").as_bytes(),
        ));
        for (k, v) in props {
            o.properties.insert((*k).into(), serde_json::json!(v));
        }
        if kind == "Section" {
            let ev = graph.add_evidence(KirEvidence::new(SourceLocation::at(name, 1), "section"));
            o.evidence.push(ev);
        }
        graph.add_object(o)
    }

    fn fixture() -> (KirGraph, KirId, KirId, KirId) {
        let mut g = KirGraph::new();
        let rfc15 = obj(
            &mut g,
            "docs/rfcs/0015-packs.md",
            "Document",
            &[("rfc_number", "0015")],
        );
        obj(
            &mut g,
            "docs/rfcs/0016-facts.md",
            "Document",
            &[("rfc_number", "0016")],
        );
        let provider = obj(&mut g, "build_llm_provider", "RustSymbol", &[]);
        obj(&mut g, "parse_config", "RustSymbol", &[]);
        obj(&mut g, "parse_config", "PythonSymbol", &[]);
        let section = obj(
            &mut g,
            "docs/rfcs/0016-facts.md § Motivation",
            "Section",
            &[
                ("rfc_number", "0016"),
                (
                    "excerpt",
                    "## Motivation\nThis builds on RFC 0015 for pack segments.\nSee RFC 0016 and RFC 9999.\nCalls `build_llm_provider()` and `parse_config`, not `ekos build`.",
                ),
            ],
        );
        (g, section, rfc15, provider)
    }

    #[test]
    fn links_rfc_mentions_with_relation_and_skips_self_and_unknown() {
        let (g, section, rfc15, _) = fixture();
        let rels = doc_links(&g);
        let rfc: Vec<_> = rels
            .iter()
            .filter(|r| r.properties["link_type"] == "rfc")
            .collect();
        assert_eq!(rfc.len(), 1, "self (0016) and unknown (9999) must not link");
        assert_eq!(rfc[0].from, section);
        assert_eq!(rfc[0].to, rfc15);
        assert_eq!(rfc[0].kind, RelationshipKind::References);
        assert_eq!(rfc[0].properties["relation"], "depends_on");
        assert!(!rfc[0].evidence.is_empty());
    }

    #[test]
    fn links_a_unique_backticked_symbol_but_not_an_ambiguous_one() {
        let (g, _, _, provider) = fixture();
        let code: Vec<_> = doc_links(&g)
            .into_iter()
            .filter(|r| r.properties["link_type"] == "code")
            .collect();
        assert_eq!(code.len(), 1, "{code:?}");
        assert_eq!(code[0].to, provider);
        assert_eq!(code[0].properties["mention"], "build_llm_provider");
    }

    #[test]
    fn a_qualified_path_falls_back_to_its_unique_last_segment() {
        let mut g = KirGraph::new();
        let target = obj(&mut g, "reason_with_history", "RustSymbol", &[]);
        obj(
            &mut g,
            "CLAUDE.md § Runtime",
            "Section",
            &[("excerpt", "see `AiRuntime::reason_with_history`")],
        );
        let rels = doc_links(&g);
        assert_eq!(rels.len(), 1);
        assert_eq!(rels[0].to, target);
    }

    #[test]
    fn is_deterministic_across_runs() {
        let (g, ..) = fixture();
        let a: Vec<_> = doc_links(&g).iter().map(|r| (r.id, r.to)).collect();
        let b: Vec<_> = doc_links(&g).iter().map(|r| (r.id, r.to)).collect();
        assert_eq!(a, b);
    }

    #[test]
    fn caps_links_per_section() {
        let mut g = KirGraph::new();
        let mut excerpt = String::new();
        for i in 0..(MAX_LINKS_PER_SECTION + 10) {
            let name = format!("symbol_{i:03}");
            obj(&mut g, &name, "RustSymbol", &[]);
            excerpt.push_str(&format!("`{name}` "));
        }
        obj(
            &mut g,
            "big.md § Index",
            "Section",
            &[("excerpt", &excerpt)],
        );
        assert_eq!(doc_links(&g).len(), MAX_LINKS_PER_SECTION);
    }

    #[test]
    fn rfc_mention_parsing() {
        let m = rfc_mentions("RFC-0013 and RFC 43, XRFC 0001, RFC 00150\nsupersedes RFC 0007");
        assert_eq!(
            m,
            vec![
                ("0013".to_string(), "mentions"),
                ("0007".to_string(), "supersedes")
            ]
        );
    }

    #[test]
    fn code_span_filtering() {
        let spans = code_spans(
            "`ekos build` `transform_ir.rs` `[llm]` `ekos_common::redaction` `ekos-ledger` `run`",
        );
        assert_eq!(
            spans.into_iter().collect::<Vec<_>>(),
            vec![
                "ekos-ledger".to_string(),
                "ekos_common::redaction".to_string()
            ]
        );
    }
}
