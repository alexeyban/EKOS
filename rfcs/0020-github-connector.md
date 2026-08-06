# RFC 0020 — GitHub Issues/PRs Connector (Proof-of-Concept)

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-07-24
**Gating:** none (additive; follows RFC 0012's connector pattern, RFC 0017's
Observer+recovery-pass integration shape)

---

## Motivation

The original "Sources → Knowledge Graph → Planner → Expert Agent" vision named
GitHub issues/PRs, Confluence, Jira, Stack Overflow, and blogs as ingestion
sources alongside the repo itself. The roadmap this RFC belongs to
(prioritizing the reasoning engine first, per the user's explicit choice)
deferred all new connectors except **one proof-of-concept**, to demonstrate
that source breadth beyond git/SQL/files follows the same established pattern
without a new architectural mechanism. GitHub was chosen because it is the
only one of those sources where "what does this reference" is already a
structured, well-known relationship (a PR's changed files; a closing keyword
in an issue/PR body) rather than free text needing an LLM to interpret.

## Design

### `ekos-plugin-github` — Observer

Follows RFC 0012's `SalesforceClient`/`SalesforceObserver` shape exactly
(constructor-injected client trait, real + mock implementations,
`Observer::scan` maps client output to `ObservationArtifact`s):

```rust
pub struct GitHubItem {
    pub number: u64,
    pub title: String,
    pub body: String,
    pub state: String,           // "open" | "closed"
    pub is_pull_request: bool,
    pub files_changed: Vec<String>, // populated for PRs only
}

#[async_trait]
pub trait GitHubClient: Send + Sync {
    async fn list_items(&self, owner: &str, repo: &str) -> Result<Vec<GitHubItem>, GitHubClientError>;
}
```

- `GitHubApiClient` — written to the documented GitHub REST v3 shape
  (`GET /repos/{owner}/{repo}/issues?state=all`, distinguishing PRs by the
  presence of a `pull_request` key per GitHub's own API convention; `GET
  /repos/{owner}/{repo}/pulls/{number}/files` for each PR's changed paths).
  **Never exercised against the live GitHub API** — no token/repo available
  in this environment, the same honest scoping RFC 0012 used for the
  Salesforce/SAP sandboxes.
- `MockGitHubClient` — fixed in-memory data, exercises the real mapping logic
  (`GitHubObserver::scan`, and the recovery pass below) with zero network
  dependency. This is the test bar for this phase, matching RFC 0012's.
- `GitHubObserver` emits one `ObservationArtifact` per issue/PR, target
  `"{owner}/{repo}#{number}"`.
- Config via `[connectors.github]` in `ekos.toml` (owner, repo, optional
  token env var name), same shape as other plugins' config sections.

### `GitHubAnalyzerPass` — recovery pass (RFC 0017's integration shape)

Per the crypto connector's established decision ("Observer + recovery-pass
pattern, not the `build.rs` inline shortcut `FileObserver` uses" —
`build.rs`'s inline promotion only ever constructs bare objects; a real
Object+Relationship+Evidence mapping needs a `CompilerPass`, same as
`CryptoAnalyzerPass`/`GitAnalyzerPass`/`DependencyAnalyzerPass`):

- One `KirObject` per issue/PR: `ObjectKind::Custom("Issue")` or
  `Custom("PullRequest")`, named `"{owner}/{repo}#{number}: {title}"`,
  properties `number`/`state`/`body` excerpt, deterministic id
  (`Uuid::new_v5` on `"github:{owner}/{repo}#{number}"` — stable across
  repeated `ekos recover` runs, same scheme every other pass in this
  codebase uses).
- **`References` edges, PR → file**: for each path in `files_changed`, a
  `RelationshipKind::References` edge from the PR object to the file object
  id — reusing `build.rs`'s exact deterministic file-id scheme
  (`Uuid::new_v5(NAMESPACE_URL, rel_path)`, the same reuse `DependencyAnalyzerPass`
  already established in RFC 0019) so the edge lands on the same object
  `ekos_search`/`ekos_impact` resolve, *if* that file has been observed by
  `ekos build`. If it hasn't (a GitHub-only workspace with no local
  checkout), the edge still records real provenance — a relationship whose
  target object doesn't exist yet is a pre-existing, accepted shape in this
  codebase (nothing at the ledger layer requires an edge's endpoints to
  already be objects).
- **`References` edges, closes-keyword → issue/PR**: body text is scanned
  for GitHub's own documented auto-close keywords (`close`, `closes`,
  `closed`, `fix`, `fixes`, `fixed`, `resolve`, `resolves`, `resolved`,
  case-insensitive) immediately followed by `#<number>` — the same
  plain-substring-matching philosophy RFC 0019 uses for dependency patterns,
  not a full GitHub-flavored-markdown parser. Emits a `References` edge from
  the referencing item to the referenced item (same repo, resolved by
  number → deterministic id).
- Evidence for every edge: the actual PR file-change entry, or the actual
  body sentence containing the closing keyword.

## Alternatives Considered

- **Confluence/Jira/Stack Overflow/blogs as the proof-of-concept instead of
  GitHub** — rejected for this phase; none of those sources have a
  structured "references X" relationship as directly available as a PR's
  file list or a closing keyword — they'd need LLM-based relationship
  extraction (a different, larger mechanism) to produce anything beyond
  free-text objects. GitHub proves the connector-breadth story with the
  cheapest possible relationship extraction.
- **A full GitHub GraphQL client for richer data (reviews, linked PRs via
  the API's native `closedByPullRequestsReferences` field) instead of
  REST + text-pattern matching** — rejected for a v1 proof-of-concept; the
  REST + keyword-pattern approach is simpler, has no new query language to
  learn, and is sufficient to prove the pattern generalizes. A future RFC
  can upgrade to GraphQL if body-text keyword matching proves too lossy in
  practice (it will miss non-standard phrasing, e.g. "this addresses #12").
- **Promoting inline in `build.rs`, like `FileObserver`** — rejected,
  consistent with RFC 0017's decision: `build.rs`'s inline path only
  constructs bare objects (no relationships), and this connector's entire
  value is in the `References` edges.

## Testing

- `MockGitHubClient`-driven observer tests: one artifact per issue/PR;
  distinguishes issues from PRs; empty repo produces no artifacts; same
  input produces the same artifact ids (idempotency).
- `GitHubAnalyzerPass` tests (mirroring `dependency_analyzer.rs`'s style):
  a PR with `files_changed` emits `References` edges to those file ids; a
  body containing `"Fixes #7"` emits a `References` edge to issue #7; an
  unrecognized closing phrase emits no edge; the same item across two
  passes resolves to the same object id (idempotent re-run).
- No live API integration test — same honest scoping as RFC 0012's
  Salesforce/SAP plugins (no sandbox credential available here).

## Acceptance Criteria

- [ ] `GitHubObserver` + `MockGitHubClient` pass the mock-driven test suite.
- [ ] `GitHubAnalyzerPass` emits `References` edges for PR→file and
      closes-keyword→issue/PR, each with evidence.
- [ ] Config wired via `[connectors.github]`, following the existing plugin
      config pattern.
- [ ] Zero new crate dependencies beyond the workspace's existing `reqwest`.
