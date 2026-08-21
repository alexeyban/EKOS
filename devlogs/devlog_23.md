# Devlog 23 — RFC 0022: Confluence connector (proof-of-concept)

**Date:** 2026-07-25
**PRs:** worked on `main` (single session)
**Branch:** main

---

## Summary

Adds a second new-source connector on top of RFC 0020's GitHub proof-of-concept, at the user's
explicit request. `ekos-plugin-confluence` observes pages in one Confluence space via the
documented REST API; `ConfluenceAnalyzerPass` maps them to KIR — one object per page, `Contains`
edges for the real page hierarchy (Confluence's own `ancestors` field), and `References` edges for
in-page links detected via Confluence's own `content-title` link markup. Like every connector in
this codebase (Salesforce, SAP, crypto, GitHub), this ships mock/fixture-tested only — no live
Confluence credential available in this environment.

The key design decision: Confluence was originally scoped out of RFC 0020 with the reasoning that
it "would need LLM-based relationship extraction... a different, larger mechanism." This RFC avoids
that by using two relationships Confluence's own API/content format already expose structurally —
page hierarchy and page-link markup — keeping the connector at the same LLM-free, pattern-matching
tier as GitHub's file-changes/closes-keywords, rather than introducing a new enrichment mechanism.

---

## RFC 0022 — Confluence Connector

### What was built

| Component | File | Detail |
|---|---|---|
| `ConfluencePage`, `ConfluenceClient` trait | `plugins/confluence/src/lib.rs` | Mirrors `GitHubClient`'s constructor-injection shape exactly |
| `ConfluenceApiClient` | same | Real client against Confluence Cloud REST API v1 content endpoint (`spaceKey`, `expand=body.storage,ancestors`); written to the documented shape, never run live |
| `MockConfluenceClient` | same | Fixed in-memory pages — the test bar for this phase |
| `ConfluenceObserver` | same | One `ObservationArtifact` per page, target `"{space_key}:{page_id}"` |
| `ConfluenceAnalyzerPass` | `recovery/src/confluence_analyzer.rs` | One `KirObject` per page (`Custom("Page")`), `Contains` edges parent→child, `References` edges for intra-batch `content-title` links |
| `docs/rfcs/0022-confluence-connector.md` | new | Full RFC written first |
| `build.rs`/`recover.rs` wiring | `cli/src/commands/{build,recover}.rs` | `EKOS_CONFLUENCE_BASE_URL`/`EKOS_CONFLUENCE_SPACE` (required) + `EKOS_CONFLUENCE_TOKEN` (optional), same soft-skip pattern as crypto/GitHub |

### Implementation details worth remembering

- **The page-link detector only resolves titles within the same fetch batch** — Confluence's
  `content-title="..."` markup names a page by title, not by a globally resolvable id, so a link to
  a page outside the current space/batch produces no edge rather than a guessed one. This mirrors
  GitHub's closes-keyword scanner's honesty (it computes a deterministic id even for
  not-yet-fetched issues, because GitHub numbers are globally unique within a repo; Confluence
  titles are not globally unique across spaces, so the same trick doesn't apply here) — a real,
  documented v1 limitation, not an oversight.
- **`Contains` edges reuse a real API field (`ancestors`), not an inference** — Confluence's REST
  API already tells you a page's parent chain; this is the same "use what's already structured"
  principle RFC 0019/0020 established for dependency imports and closing keywords.
- **This is now the fourth connector following the exact same constructor-injected-client + real +
  mock shape** (Salesforce/SAP from RFC 0012, crypto from RFC 0017, GitHub from RFC 0020, now
  Confluence) — strong evidence the pattern generalizes cleanly across genuinely different source
  shapes without needing per-connector architectural changes.

### Decisions (alternatives considered, why this choice)

- **LLM-based topic/concept extraction from page bodies** — rejected again for this
  proof-of-concept, for the same reason RFC 0020 originally deferred Confluence entirely: a real
  capability worth having eventually, but a different, larger mechanism than proving connector
  breadth cheaply and structurally.
- **Confluence REST API v2 (space-id-based)** — the v1 content API's `spaceKey` + `expand` shape is
  simpler and sufficient for a client that will never run against a live site in this environment;
  a real credential would be needed to validate a v2 migration properly.

---

## Knowledge Captured

- **Not every "needs an LLM" scoping decision holds up under closer inspection** — RFC 0020 assumed
  Confluence needed LLM-based extraction to produce any relationship at all; on closer look, its own
  REST API (`ancestors`) and its own content format (`content-title` link markup) already expose two
  real, structural relationships for free. Worth re-examining similar "this needs an LLM" assumptions
  for Jira and Stack Overflow before assuming they're actually harder than GitHub/Confluence turned
  out to be.
- **Four connectors now share the identical constructor-injected-client shape from RFC 0012** — this
  is a stable, proven pattern in this codebase; a fifth connector should follow it without
  relitigating the design.

---

## Files Changed

| File | Change summary |
|---|---|
| `docs/rfcs/0022-confluence-connector.md` | New RFC |
| `ekos/plugins/confluence/Cargo.toml`, `ekos/plugins/confluence/src/lib.rs` | New `ekos-plugin-confluence` crate: `ConfluenceClient`/`ConfluenceApiClient`/`MockConfluenceClient`/`ConfluenceObserver` + tests |
| `ekos/crates/recovery/src/confluence_analyzer.rs` | New `ConfluenceAnalyzerPass` + tests |
| `ekos/crates/recovery/src/lib.rs` | Export `confluence_analyzer` module |
| `ekos/Cargo.toml` | New workspace member `plugins/confluence` |
| `ekos/crates/cli/Cargo.toml` | New dependency `ekos-plugin-confluence` |
| `ekos/crates/cli/src/commands/build.rs` | `EKOS_CONFLUENCE_BASE_URL`/`EKOS_CONFLUENCE_SPACE`/`EKOS_CONFLUENCE_TOKEN`-gated observer registration |
| `ekos/crates/cli/src/commands/recover.rs` | `collect_confluence_artifact_ids` + pass registration + summary line |
