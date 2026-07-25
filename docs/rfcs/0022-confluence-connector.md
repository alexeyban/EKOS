# RFC 0022 — Confluence Connector (Proof-of-Concept)

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-07-25
**Gating:** none (additive; follows RFC 0012's connector pattern, RFC 0020's
Observer + recovery-pass integration shape)

---

## Motivation

The original ingestion-source vision named Confluence alongside GitHub,
Jira, and Stack Overflow. RFC 0020 shipped GitHub as the roadmap's one
required proof-of-concept and explicitly deferred the rest, noting that
Confluence/Jira/blogs lack GitHub's cheaply-available structured
relationships (a PR's file list, a closing keyword) and would "need
LLM-based relationship extraction... a different, larger mechanism."

This RFC adds Confluence anyway, **without** introducing an LLM pass, by
using two relationships that Confluence's own API and content format
already expose structurally, no interpretation required:

1. **Page hierarchy** — Confluence pages carry an explicit parent page id
   (`ancestors`, in the REST API's own terms). This is a real `Contains`
   edge, not an inference.
2. **In-page links to other pages** — Confluence's storage format
   represents a link to another page as `<ri:page ri:content-title="Some
   Page" />` — a structured tag, not free prose. Detecting it is a plain
   substring scan for `content-title="..."`, the same pattern-matching
   philosophy RFC 0019/0020 already established, not markup parsing.

This keeps the connector at the same structural, LLM-free tier as GitHub,
while proving the *breadth* claim — a second, independently-shaped source
follows the identical Observer + recovery-pass integration with no new
architectural mechanism.

## Design

### `ekos-plugin-confluence` — Observer

Mirrors `SalesforceClient`/`GitHubClient`'s constructor-injection shape
exactly (RFC 0012, RFC 0020):

```rust
pub struct ConfluencePage {
    pub id: String,
    pub space_key: String,
    pub title: String,
    /// Storage-format body (Confluence's own XHTML-like markup) — read,
    /// not rendered; link detection scans this directly.
    pub body: String,
    pub parent_id: Option<String>,
}

#[async_trait]
pub trait ConfluenceClient: Send + Sync {
    async fn list_pages(&self, space_key: &str) -> Result<Vec<ConfluencePage>, ConfluenceClientError>;
}
```

- `ConfluenceApiClient` — written to the documented Confluence Cloud REST
  API shape (`GET /wiki/rest/api/content?spaceKey={key}&expand=body.storage,ancestors`).
  **Never exercised against a live Confluence site** — no credential
  available in this environment, the same honest scoping every connector
  in this codebase has used since RFC 0012 (Salesforce, SAP), through RFC
  0020 (GitHub).
- `MockConfluenceClient` — fixed in-memory pages; the actual test bar for
  this phase.
- `ConfluenceObserver` emits one `ObservationArtifact` per page, target
  `"{space_key}:{page_id}"`.
- Config via `EKOS_CONFLUENCE_BASE_URL`/`EKOS_CONFLUENCE_SPACE`/
  `EKOS_CONFLUENCE_TOKEN` env vars, same soft-skip-when-absent pattern
  `build.rs` already uses for the crypto and GitHub connectors.

### `ConfluenceAnalyzerPass` — recovery pass

Same shape as `GitHubAnalyzerPass` (RFC 0020): one `CompilerPass`, pure
structural mapping, no LLM in the loop.

- One `KirObject` per page: `ObjectKind::Custom("Page")`, named
  `"{space_key}: {title}"`, deterministic id (`Uuid::new_v5` on
  `"confluence:{space_key}:{page_id}"`), `excerpt` property from the page
  body (capped, same convention as every other connector's searchable
  text) so pages are findable via `ekos_search`.
- **`Contains` edge, parent → child**: when `parent_id` is present and
  resolves to another page in the same batch, a `RelationshipKind::Contains`
  edge from the parent to the child, evidence citing the parent-child
  relationship itself. (If the parent isn't in the current batch — e.g. a
  single-space fetch whose parent lives in another space — the edge target
  id is still computed and recorded; see RFC 0020's identical acceptance of
  edges whose target isn't yet a known object.)
- **`References` edge, page → linked page**: body text is scanned for
  `content-title="<title>"` occurrences (Confluence's own page-link markup).
  A match is resolved to a `KirId` **only when the title matches another
  page title in the same batch** — cross-batch/cross-space title resolution
  is out of scope for this proof-of-concept (a real limitation, not an
  oversight: unlike GitHub's numeric issue references, a title alone isn't
  guaranteed unique across spaces, so this RFC only trusts a match within
  the fetched set).

## Alternatives Considered

- **LLM-based extraction of "what topics/concepts does this page discuss
  and relate to"** — rejected for this proof-of-concept, exactly as RFC
  0020 reasoned for Confluence originally: a real capability worth having
  eventually, but a different, larger mechanism (`SqlAnalyzerPass`'s
  enrichment shape) than this RFC's goal of proving connector breadth
  cheaply and structurally.
- **Cross-space title resolution for `content-title` links** — rejected for
  v1; would require either fetching every space up front or a second pass
  after all spaces are known. Documented as a real limitation, not solved
  here.
- **Confluence Cloud REST API v2 (space-id-based) instead of the v1
  content API (space-key-based)** — the v1 API's `expand=body.storage,ancestors`
  shape is simpler and sufficiently documented for a client that will never
  be run against a live site in this environment; v2 migration is a
  reasonable future improvement once real credentials exist to validate
  against.

## Testing

- `MockConfluenceClient`-driven observer tests: one artifact per page;
  empty space produces no artifacts; same input produces the same artifact
  ids (idempotency).
- `ConfluenceAnalyzerPass` tests (mirroring `github_analyzer.rs`'s style):
  a child page with `parent_id` set emits a `Contains` edge from parent to
  child; a body containing `content-title="Other Page"` where "Other Page"
  is in the same batch emits a `References` edge; a `content-title`
  reference to a title *not* in the batch emits no edge; the same page
  across two passes resolves to the same object id (idempotent re-run).
- No live API integration test — same scoping as every other connector in
  this codebase.

## Acceptance Criteria

- [ ] `ConfluenceObserver` + `MockConfluenceClient` pass the mock-driven
      test suite.
- [ ] `ConfluenceAnalyzerPass` emits `Contains` edges for page hierarchy and
      `References` edges for intra-batch page links, each with evidence.
- [ ] Wired into `build.rs`/`recover.rs` following the GitHub connector's
      env-var-gated, soft-skip pattern.
- [ ] Zero new crate dependencies beyond the workspace's existing `reqwest`.
