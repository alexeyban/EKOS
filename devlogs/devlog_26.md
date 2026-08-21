# Devlog 26 — Prompt-injection sanitization for the local document connector

**Date:** 2026-07-30
**PRs:** worked on `main` (single session, follow-up to devlog 24/25 — RFC 0023)
**Branch:** main

---

## Summary

Added a sanitization pass to `ekos-plugin-localdocs` that strips zero-width Unicode
(`U+200B/200C/200D/2060/FEFF`) and the Unicode tag block (`U+E0000`–`U+E007F`) from every string
the connector extracts — prose excerpt, table cells, OCR output — before any of it is written to
an `ObservationArtifact`. This closes a real gap identified while comparing EKOS's document
connector against `book-to-skill` (a similar document-to-agent-knowledge tool, 13.6k★): that
project deliberately hardens against exactly this class of document-borne prompt injection, and
RFC 0023's connector had no equivalent defense — any PDF/DOCX with hidden instructions embedded via
invisible Unicode would have flowed straight into the ledger and, from there, into an agent's
context via `ekos_search`/`ekos_state`.

## Design

- New module `plugins/localdocs/src/sanitize.rs`: `sanitize_text(&str) -> Sanitized { text,
  removed }`, a single filter pass with no new dependency (stdlib `char` iteration).
- Wired in at the one point in `LocalDocsObserver::scan` where every extracted string already
  passes through on its way into the artifact `data` JSON — the prose excerpt, each table cell (a
  nested loop inside the existing `tables_json` construction), and OCR output as each image is
  recognized. One injection point, three call sites, no duplicated logic.
- `sanitized_chars_removed` rides on the artifact only when nonzero (the same "absent means
  normal" convention `ocr_text` already uses) — a document search on this field alone would
  surface every book/PDF in a workspace that ever contained a hidden-Unicode payload, which is a
  useful signal on its own.
- A nonzero count also gets a `tracing::warn!` at scan time, so it's visible in `ekos build` output
  immediately, not just discoverable later by querying the ledger.

## Decisions

- **Scope matched to `book-to-skill`'s, not a general Unicode sanitizer** — stripping every
  non-printable/control character risks silently corrupting legitimate technical content (math
  notation, non-Latin scripts use plenty of combining marks and joiners that are *not* the
  zero-width/tag-block attack surface). The two character classes targeted are specifically the
  ones with no legitimate rendering purpose in prose that could still carry ASCII-mapped payloads.
- **Did not add "reject the document if sanitization removes 100% of its visible text"** —
  `book-to-skill` does this; RFC 0023's addendum explicitly scoped it out for now. A single
  scanned-cover-page OCR result being *entirely* invisible characters is a plausible malicious
  signal, but dropping the whole document loses real structural facts (page count, tables) that
  are independently useful and were never in the injected payload. Logged as a candidate follow-up,
  not implemented.
- **Did not add a post-hoc scan of committed KIR objects** for instruction-override phrasing (the
  advisory step `book-to-skill`'s Step 9.5 does) — that's a defense at a different layer (after
  compilation, across the whole ledger) than this addendum's scope (sanitize at extraction time,
  per-connector). Worth revisiting as an estate-wide diagnostic pass someday, not per-connector.

## Testing

- `sanitize.rs`: strips both character classes, leaves ordinary Unicode (em dash, section sign,
  accented characters) untouched, no-ops on empty input.
- `lib.rs` end-to-end: a synthetic hidden payload (tag-block-encoded "hidden") planted in prose, a
  table cell, and OCR output all get stripped by the time they reach the artifact, with
  `sanitized_chars_removed` reported; a clean document carries no such field at all.
- Full workspace: `cargo test --workspace` (all crates green), `cargo clippy --workspace
  --all-targets` (zero new warnings), `cargo fmt --check` clean.

## Knowledge Captured

- The Unicode tag block (`U+E0000`–`U+E007F`) exists specifically because each tag codepoint is
  offset from a printable ASCII character by a fixed amount — `char::from_u32(0xE0000 +
  ascii_byte as u32)` round-trips. That's what makes it the standard "invisible ASCII smuggling"
  vector: a whole hidden sentence can be tag-encoded and will render as nothing in any normal
  viewer while a model reading raw text sees it plainly. Test fixtures used real tag-encoded text
  (`\u{E0068}\u{E0069}...` = tag-h, tag-i, ... = "hi") rather than an arbitrary placeholder, so the
  regression test actually exercises the real attack shape.
- Comparing a project against an established, well-audited peer (`book-to-skill`'s README documents
  its own security hardening in detail: CodeQL/Bandit/Zizmor in CI, XXE/billion-laughs guards,
  argument-injection guards) is a fast way to find real gaps — this sanitization step exists because
  the comparison surfaced a concrete, specific threat class the original RFC 0023 never considered,
  not because of abstract "add security" pressure.

## Files Changed

| File | Change summary |
|---|---|
| `docs/rfcs/0023-local-document-connector.md` | New Security addendum section + acceptance criterion |
| `ekos/plugins/localdocs/src/sanitize.rs` | New: zero-width + Unicode-tag-block stripping |
| `ekos/plugins/localdocs/src/lib.rs` | Wired sanitization into excerpt/table-cell/OCR-text capture; two new end-to-end tests |
