# Devlog 22 — RFC 0020: GitHub Issues/PRs connector (proof-of-concept)

**Date:** 2026-07-24
**PRs:** worked on `main` (single session)
**Branch:** main

---

## Summary

Closes the last item of the "extend EKOS with deeper planning and dependency reasoning" roadmap:
one new connector, proving that source breadth beyond git/SQL/files follows the same established
Observer + recovery-pass pattern without inventing a new architectural mechanism. `ekos-plugin-github`
observes issues and PRs for one `owner/repo` via the GitHub REST API; `GitHubAnalyzerPass` maps them
to KIR — one object per issue/PR, `References` edges from a PR to the files its diff touches, and
`References` edges from an item to whatever it closes via GitHub's own documented auto-close
keywords (`Fixes #N`, `Closes #N`, etc.). Both plugin and pass are mock/fixture-tested only — no
live GitHub credential available in this environment, the same honest scoping RFC 0012 used for the
Salesforce/SAP connectors.

This is the fourth and final phase of the roadmap kicked off several sessions ago (Phase 0 — Ollama,
RFC 0021; Phase 1 — multi-hop impact reasoning, RFC 0018; Phase 2 — dependency-fact extraction +
symbol harvesting, RFC 0019; Phase 3 — this one). The Planner/Expert Agent demo layer (`impact-analyst`,
`demo/DEMO.md` Act 8) was wired in an earlier session once RFC 0018 landed.

---

## RFC 0020 — GitHub Issues/PRs Connector

### Problem / motivation

The original vision diagram named GitHub issues/PRs, Confluence, Jira, and Stack Overflow as
ingestion sources beyond the repo itself. The user explicitly scoped the roadmap to **one** new
connector as proof-of-concept (not all of them) — GitHub was the natural choice because its
"what does this reference" relationships (a PR's changed files; an issue-closing keyword) are
already structured data, not free text needing an LLM to interpret, unlike Confluence/Jira/blogs.

### What was built

| Component | File | Detail |
|---|---|---|
| `GitHubItem`, `GitHubClient` trait | `plugins/github/src/lib.rs` | Mirrors `SalesforceClient`'s constructor-injection shape exactly (RFC 0012) |
| `GitHubApiClient` | same | Real client against GitHub REST v3 (`/issues?state=all`, `/pulls/{n}/files`); written to the documented shape, never run against the live API (no token/repo in this environment) |
| `MockGitHubClient` | same | Fixed in-memory items — the actual test bar for this phase |
| `GitHubObserver` | same | One `ObservationArtifact` per issue/PR, target `"{owner}/{repo}#{number}"` |
| `GitHubAnalyzerPass` | `recovery/src/github_analyzer.rs` | New `CompilerPass`: one `KirObject` per item (`Custom("Issue")`/`Custom("PullRequest")`), `References` edges PR→file and closes-keyword→item, each with evidence |
| `docs/rfcs/0020-github-connector.md` | new | Full RFC written first, per the mandatory workflow |
| `build.rs` wiring | `cli/src/commands/build.rs` | `EKOS_GITHUB_OWNER`/`EKOS_GITHUB_REPO` (required) + `EKOS_GITHUB_TOKEN` (optional) env-var-gated, same soft-skip pattern as the crypto connector's `EKOS_CRYPTO_EXPORT_DIR` |
| `recover.rs` wiring | `cli/src/commands/recover.rs` | `collect_github_artifact_ids` (mirrors `collect_crypto_artifact_ids`) + pass registration + summary line |

### Implementation details worth remembering

- **The closing-keyword scanner is plain substring matching, not a GFM parser** — same tradeoff as
  RFC 0019's dependency-pattern matching. It correctly handles the tricky case of `"close"` being a
  literal prefix of `"closes"`/`"closed"` (verified by test: searching for `"close"` in `"closes
  #1"` finds the substring but the character immediately after isn't `#`, so it correctly doesn't
  double-fire — the `"closes"` keyword's own scan catches the real match). It will also produce a
  false positive on `"disclosed #1"` (contains `"closed"` as a substring) — an accepted, documented
  v1 limitation, not a bug.
- **PR→file `References` edges reuse `build.rs`'s exact file-id derivation
  (`Uuid::new_v5(NAMESPACE_URL, rel_path)`)** — the same reuse `DependencyAnalyzerPass` established
  in RFC 0019. This is now the *third* independent call site of that exact scheme
  (`build.rs`, `dependency_analyzer.rs`, `github_analyzer.rs`) — a good candidate for extracting into
  one shared function in `ekos-kir` if a fourth caller ever appears, flagged but not done here to
  avoid touching working code across three files for a v1 proof-of-concept.
- **A `References` edge can point at a file object that doesn't exist yet** (e.g. a GitHub-only
  workspace that hasn't run `ekos build` against the actual checkout, or a PR touching a file that
  was later deleted) — this is a pre-existing, accepted shape at the ledger layer (nothing requires
  an edge's endpoints to already be objects), not new to this RFC.
- Unlike `DependencyAnalyzerPass` (RFC 0019) which is unconditionally wired into every `ekos recover`
  run (scanning source files that are always present), the GitHub connector is fully opt-in via env
  vars — most workspaces have no GitHub repo configured, and unauthenticated requests against a
  real repo would hit GitHub's public rate limit during CI/tests if ever accidentally enabled.

### Decisions (alternatives considered, why this choice)

- **GitHub GraphQL (with its native `closedByPullRequestsReferences` field) instead of REST +
  keyword matching** — rejected for this proof-of-concept; REST + pattern matching needs no new
  query language and is sufficient to prove the connector-breadth pattern generalizes. A future RFC
  can upgrade if keyword matching's false-negative rate (missing phrasing like "this addresses #12")
  proves too lossy in practice.
- **Confluence/Jira/Stack Overflow as the proof-of-concept instead** — rejected; none of those
  sources have a structured "references X" relationship as cheaply available as GitHub's PR file
  list and closing keywords — they'd need LLM-based extraction, a different and larger mechanism.
- **Promoting inline in `build.rs` like `FileObserver`** — rejected, consistent with RFC 0017's
  decision: the inline path only ever constructs bare objects, and this connector's entire value is
  in the `References` edges, which need a real `CompilerPass`.

---

## Knowledge Captured

- **The `Uuid::new_v5(NAMESPACE_URL, rel_path)` file-id scheme is now depended on by three
  independent modules that must never diverge**: `build.rs` (the original definition), RFC 0019's
  `dependency_analyzer.rs`, and this RFC's `github_analyzer.rs`. None of them import a shared
  function — each redefines the same one-liner locally. If file-object identity ever needs to
  change (e.g. per-repo namespacing in a multi-repo estate), all three must be updated together;
  worth extracting to `ekos_kir` before a fourth caller appears.
- **GitHub's issues API returning both issues and PRs, distinguished only by the presence of a
  `pull_request` key**, is a real, documented GitHub API quirk (not an EKOS design choice) — the
  observer's `is_pull_request` field exists specifically to normalize that away for every downstream
  consumer, the same way `GitAnalyzerPass`'s devlog(14) noted normalizing a similarly quirky
  upstream shape.
- **Every connector this project has added so far (Salesforce, SAP, crypto, now GitHub) has shipped
  with zero live-credential integration testing** — this is a consistent, deliberate project pattern
  (RFC 0012's original scoping decision), not something specific to this session. Mock-client tests
  are the accepted bar; a live integration test is explicitly out of scope until a real credential
  becomes available.

---

## Files Changed

| File | Change summary |
|---|---|
| `docs/rfcs/0020-github-connector.md` | New RFC |
| `ekos/plugins/github/Cargo.toml`, `ekos/plugins/github/src/lib.rs` | New `ekos-plugin-github` crate: `GitHubClient`/`GitHubApiClient`/`MockGitHubClient`/`GitHubObserver` + tests |
| `ekos/crates/recovery/src/github_analyzer.rs` | New `GitHubAnalyzerPass` + tests |
| `ekos/crates/recovery/src/lib.rs` | Export `github_analyzer` module |
| `ekos/Cargo.toml` | New workspace member `plugins/github` |
| `ekos/crates/cli/Cargo.toml` | New dependency `ekos-plugin-github` |
| `ekos/crates/cli/src/commands/build.rs` | `EKOS_GITHUB_OWNER`/`EKOS_GITHUB_REPO`/`EKOS_GITHUB_TOKEN`-gated observer registration |
| `ekos/crates/cli/src/commands/recover.rs` | `collect_github_artifact_ids` + pass registration + summary line |
