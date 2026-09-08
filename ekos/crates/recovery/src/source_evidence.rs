//! RFC 0140 §1 — attach a real link back to source text for symbols that carry a span.
//!
//! RFC 0088 taught `rust_analyzer`, `elixir_analyzer` and `python_analyzer` to record a
//! `source_span` (`{start_line, end_line}`) per symbol. Nothing ever attached a *file* to it: those
//! analyzers emitted no `KirEvidence` at all (see `rust_analyzer.rs`'s own RFC 0079 note), so the
//! path survived only as an id-hash ingredient. A symbol therefore knew it lived at lines 200-322 —
//! of an unnamed file.
//!
//! Measured on the RFC 0138 suite before this existed: of 1,289 evidence claims rendered to the
//! model in a full run, **zero** carried a line number and only 26.4% carried any location. An
//! answer could cite a file and never a place in it, and no consumer could re-read the code behind
//! a claim.
//!
//! **Why persisting the fragment is safe.** The text comes from the analyzer's own `data.source`,
//! which reached it through the observation layer and has therefore already passed RFC 0043
//! redaction. That is the same reasoning that makes RFC 0140 §3 require query-time source reads to
//! come from the content-addressed artifact store rather than the live filesystem — reading a file
//! from disk later would be a new raw-content entry point that redaction never saw.

use ekos_kir::{KirEvidence, KirGraph, KirObject, SourceLocation};

/// Cap on a persisted fragment. The span itself still records the true range, so a consumer that
/// wants the whole body can go and read it — this only stops one enormous item from dominating the
/// ledger.
const MAX_FRAGMENT_LINES: u64 = 40;

/// The `(start, end)` lines RFC 0088's `source_span` recorded for this object, if any.
pub(crate) fn span_lines(obj: &KirObject) -> Option<(u64, u64)> {
    let v = obj.properties.get("source_span")?;
    Some((v.get("start_line")?.as_u64()?, v.get("end_line")?.as_u64()?))
}

/// The source text of a 1-indexed, inclusive line range, capped at [`MAX_FRAGMENT_LINES`].
pub(crate) fn slice_lines(source: &str, start: u64, end: u64) -> String {
    let take = (end.saturating_sub(start) + 1).min(MAX_FRAGMENT_LINES) as usize;
    source
        .lines()
        .skip(start.saturating_sub(1) as usize)
        .take(take)
        .collect::<Vec<_>>()
        .join("\n")
}

/// Give `obj` a `KirEvidence` naming the file and line its span starts at, with the span's own
/// source as the fragment. No-op for an object without a `source_span` — analyzers that don't
/// compute one (SQL, git, docs) keep their existing file-level evidence.
pub(crate) fn attach(obj: &mut KirObject, source: &str, path: &str, graph: &mut KirGraph) {
    let Some((start, end)) = span_lines(obj) else {
        return;
    };
    let ev = KirEvidence::new(
        SourceLocation::at(path, start as u32),
        slice_lines(source, start, end),
    );
    obj.evidence.push(ev.id);
    graph.add_evidence(ev);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_kir::ObjectKind;

    fn symbol_with_span(start: u64, end: u64) -> KirObject {
        let mut o = KirObject::new("sym", ObjectKind::Custom("RustSymbol".into()));
        o.properties.insert(
            "source_span".into(),
            serde_json::json!({"start_line": start, "end_line": end}),
        );
        o
    }

    #[test]
    fn attaching_names_the_file_and_the_starting_line() {
        let mut g = KirGraph::default();
        let mut obj = symbol_with_span(3, 4);
        attach(&mut obj, "a\nb\nc\nd\ne\n", "src/demo.rs", &mut g);

        assert_eq!(obj.evidence.len(), 1, "the symbol must gain evidence");
        let ev = g.evidence.first().expect("evidence recorded on the graph");
        assert_eq!(ev.location.path, "src/demo.rs");
        assert_eq!(
            ev.location.line,
            Some(3),
            "a span without a file is unusable — the line must be paired with the path"
        );
        assert_eq!(
            ev.fragment, "c\nd",
            "the fragment is the span's real source"
        );
    }

    #[test]
    fn an_object_without_a_span_is_left_alone() {
        // SQL/git/docs analyzers record no span; they must not gain a fabricated location.
        let mut g = KirGraph::default();
        let mut obj = KirObject::new("t", ObjectKind::Table);
        attach(&mut obj, "irrelevant", "schema.sql", &mut g);
        assert!(obj.evidence.is_empty());
        assert!(g.evidence.is_empty());
    }

    #[test]
    fn an_enormous_span_is_capped_but_still_starts_at_the_span() {
        let src: String = (1..=500).map(|i| format!("line{i}\n")).collect();
        let frag = slice_lines(&src, 10, 400);
        assert!(frag.starts_with("line10"));
        assert_eq!(frag.lines().count(), MAX_FRAGMENT_LINES as usize);
    }
}
