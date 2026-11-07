# Devlog 326 — Annotations (RFC 0247): record, CLI, MCP read tool, bulk resolve, consumers

**Date:** 2026-11-07
**PRs:** none (direct commits to main)
**Branch:** main

---

## Summary
RFC 0247 is implemented through its core: a person's comment, instruction or synonym on any ledger entity,
bound to the entity's digest; a CLI; a read-only MCP tool; deciding conflicts by rule; and two consumers
(retrieval resolves approved synonyms, generated docs show "Notes from people"). RFC 0248/0249 are accepted on
the maintainer's instruction but not implemented: 0248's engine lives in `ekos-lakehouse`, 0249 needs a
moderation backend and three pre-launch checks that code cannot settle.

---

## What was built
| Piece | Where |
|---|---|
| Record, closed `Rule` vocabulary, staleness, applicability, `data_block` | `kir/src/annotations.rs` (pure; `semantic` re-exports it) |
| `ekos annotate add/list/show/withdraw/approve/confirm/resolve` | `cli/src/commands/annotate.rs` |
| Deciding conflicts by rule | `semantic/src/bulk_resolve.rs` + `annotate resolve --rule` |
| MCP `ekos_annotations` (read-only, in `SCOPED_TOOLS` and the console policy) | `cli/src/commands/mcp.rs`, `web/api/app/mcp_policy.py` |
| Synonyms in `understand` | `runtime/src/retrieval.rs::annotation_synonym_entities` |
| "Notes from people" on object pages | `docs-gen` (`AnnotationRow`, `annotation_rows`) |

## Decisions
- **Validity is computed on read**, never stored: the annotation keeps the digest it was written against; if the
  entity's digest differs it is stale and applies nothing. Same rule as RFC 0213 sign-offs.
- **Closed rule vocabulary, values are data**: a rule value may not hold quotes, semicolons, comment markers or
  control characters, so annotations cannot reopen SQL injection. Free text reaches a model only in a delimited
  `<user_notes>` data block.
- **Bulk resolve is a dry run unless `--apply`**, needs a rationale and a matcher (no match-everything rule),
  and `pick_source` only picks when exactly one claim fits — ambiguity is left to a person with the reason.
  The rule is recorded once as a shared instruction on the project; each decision is an ordinary conflict
  decision whose note is the rationale.
- **The pure module moved from `semantic` to `kir`** because `runtime` and `docs-gen` depend on `kir` only; a
  dependency on `semantic` would have been a cycle or a heavy edge. `semantic` re-exports it.
- **Runtime only honours shared + approved synonyms**: it has no requester, so a personal or unapproved synonym
  names nothing there. A synonym is *named after its word* so the lexical index finds it.

## Knowledge Captured
- `is_significant` documents every `Custom` kind: any new object kind that is a note *about* another object must
  be excluded or it gets its own page.
- `KirObject` has no `PartialEq`; types holding one cannot derive it.
- `pytest` for `web/api` needs `uv run` (and `-o addopts=""` without pytest-cov).
- A one-shot shell that `rm -rf`s a variable path is denied by the permission classifier; write the check as an
  integration test instead (it is also the better artifact).

## Not done
RFC 0247: the console annotations panel and Review-page use; term resolution that applies `Rule`s (it belongs
to RFC 0248). RFC 0248, 0249: see TODO.md.

## Files Changed
See the commits 1110af9e and the one after it: `kir/src/annotations.rs`, `semantic/src/bulk_resolve.rs`,
`cli/src/commands/{annotate,conflicts,docs,mcp,review_guard,catalogue}.rs`, `cli/tests/annotate_e2e.rs`,
`docs-gen/src/lib.rs`, `runtime/src/retrieval.rs`, `web/api/app/mcp_policy.py`, `TODO.md`, `CLAUDE.md`.
