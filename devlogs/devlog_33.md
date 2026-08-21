# Devlog 33 — Vision docs, an honest Claude Code + EKOS benchmark deck, and a GitHub-wide Pages/Actions outage

**Date:** 2026-08-06
**PRs:** none — direct commits to `main` this session (no feature-branch/PR workflow used)
**Branch:** `main` → `main`

---

## Summary

Three pieces of documentation work landed today: a non-promissory token-utility roadmap
(`VISION.md`), a new presentation deck demonstrating how Claude Code uses EKOS's MCP server for
code search/analysis with a *measured* with-vs-without token/round-trip comparison (not
estimated), and a stale-content fix to an existing deck. Separately, roughly two hours were spent
diagnosing what looked like a repo-specific GitHub Pages publishing failure — it turned out to be
a genuine GitHub-wide outage affecting both Actions and Pages, confirmed externally via
StatusGator after GitHub's own status page reported no incident.

---

## Commit — `6f2403c` — VISION.md + token-utility roadmap deck

### Problem / motivation
User supplied Russian-language notes on token-utility framing, explicitly asking to avoid
promissory language ("EKOS coin will be valuable") in favor of consequence-of-adoption framing
("token gets new utility as the platform develops"). No existing EKOS doc articulated a phased
token-utility roadmap in English.

### What was built
- `VISION.md` — 9-phase ecosystem roadmap (Community → Knowledge Network → Plugin Marketplace →
  Enterprise AI Platform → Agent Marketplace → Knowledge Marketplace → Governance → Reputation →
  Enterprise Marketplace), a "Proof of Knowledge" framing, and a 2026/2027/2028 roadmap table.
- Linked from `TOKENOMICS.md`, `README.md`, and `docs/index.html`'s token box + community grid.
- New deck: `docs/presentations/vision-and-token-utility.html`.

### Decisions
Kept `TOKENOMICS.md` scoped strictly to supply/allocation/vesting facts and moved all
utility/purpose framing into the new `VISION.md`, rather than growing `TOKENOMICS.md` further —
matches the existing site convention of one concern per document.

---

## Commit — `175cc58` — "Claude Code + EKOS" presentation deck

### Problem / motivation
User asked for a demonstration of how Claude Code uses EKOS's MCP server for code search and
analysis, with a with-vs-without token comparison. The project's existing decks
(`recovery-gaps-closed.html` etc.) set a hard precedent: every number shown must be real and
reproducible — no illustrative/estimated figures.

### What was built (10-slide deck, `docs/presentations/claude-code-with-ekos.html`)

| Slide | Content | Source of the numbers |
|---|---|---|
| §01–02 | grep-vs-`ekos_search` for "who implements `Observer`" | Live `grep`/`wc -c` against this repo |
| §03 | Measured comparison table (bytes, round-trips) | Same live commands |
| §04 | Evidence/provenance (`ekos_neighborhood` excerpt vs. full file) | Live MCP call, 600 B excerpt vs. 8,490 B file |
| §05 | Cross-project reach | Live `ekos_status` (88,637 entries / 22,032 objects / 10,335 relationships) |
| §06 | `/context` token footprint of the EKOS MCP tools in-session | Real `/context` command output (595 tokens / 0.1% of a 967k window) |
| §07 | Real 24-hour usage logs | Grepped `~/.claude/projects/<project>/*.jsonl` session transcripts for `tool_use` blocks matching `mcp__ekos__ekos_*` and `Skill` invocations of `ekos-knowledge` |
| §08 | Honest limits | `ekos_search("PassManager")` misses `pass.rs` (the actual definition) — search is file/excerpt-ranked, not symbol-aware |

### Implementation details worth remembering
- **Session transcripts are a legitimate, queryable log source.** Claude Code stores every
  session as JSONL under `~/.claude/projects/<project-slug>/*.jsonl`. Each line is a JSON object;
  `tool_use` blocks live at `message.content[].type == "tool_use"`, with `name` and `input`, and
  carry an ISO timestamp. This made "show me real EKOS usage in the last 24h" answerable with a
  plain Python/`jq` grep — no separate telemetry system needed.
- **`ekos_search` is file/excerpt-ranked, not symbol-aware**, confirmed two ways in this session:
  searching `"PassManager"` surfaced `compiler-core/src/lib.rs` (a re-export) and an RFC, but not
  `pass.rs` (the actual `struct PassManager` definition) — `grep` found it immediately, 14 hits.
  This is real product feedback, not just a deck talking point — logged here so a future session
  doesn't need to rediscover it.
- **`ekos_neighborhood`'s excerpt is capped, not proportional to file size**: an 8,490-byte file
  and (in an earlier check) much larger files both returned similarly-sized excerpts (~600 chars),
  giving a large compression ratio on big files and almost none on files already small enough to
  fit in one excerpt — worth knowing before citing an "Nx smaller" number without checking file
  size first.

### Decisions
Chose **measured production data over synthetic benchmarks** — every byte count and round-trip
number in the deck traces back to a command actually run in that session, matching the site's
"Proven, not promised" convention rather than the more common practice (see the repowise
comparison research earlier the same day) of citing benchmark-suite numbers.

---

## Commit — `fb5cd64` — fixed stale content in `github-repo-to-mcp-server.html`

### Problem / motivation
Re-reading this older deck (predates this session) turned up two factual errors: it claimed
"Seven tools" when `ekos mcp serve` now exposes eleven (`ekos_impact`,
`ekos_transformation_explain`, `ekos_transformation_diff`, and `ekos_identity_review` were added
by later RFCs and never backfilled into this deck), and its CTA footer still said "github.com —
coming soon" despite the repo and site having been public for a while.

### What was built
Added the four missing tool rows to the MCP tools table (cross-checked against
`ekos/crates/cli/src/commands/mcp.rs`'s `tool_definitions()`) and replaced the placeholder CTA
link with the real repo URL.

### Knowledge captured
Older decks can silently drift out of sync with the MCP tool surface as new RFCs ship — there's no
automated check tying deck content to `tool_definitions()`. Worth a periodic pass, or a lightweight
test asserting deck tool-name mentions are a subset of the real tool list, if this keeps happening.

---

## The GitHub Pages/Actions outage (no code change — documented for the record)

### What happened
After pushing `175cc58`, GitHub Pages returned 404 for the new deck. Diagnosis proceeded on the
(reasonable, but ultimately wrong) assumption that this was repo-specific:

1. Legacy Jekyll builder started failing in ~2–3s per attempt (vs. ~5min for a real build) —
   looked like an instant validation rejection, not real Jekyll processing.
2. Added `docs/.nojekyll` (this site has never been an actual Jekyll site — no `_config.yml`, no
   front matter, no layouts) — didn't help.
3. Switched Pages source to GitHub Actions (`build_type: workflow`), added
   `.github/workflows/pages.yml` (checkout → `upload-pages-artifact` → `deploy-pages`). Three
   separate `workflow_dispatch` runs all got stuck: one timed out after 8+ min sitting in
   `deployment_queued`, one was auto-cancelled, one hung in `waiting` on a pending-deployment
   approval gate that had no reviewers configured and returned HTTP 502 on a direct cancel
   attempt via the API.
4. Deleted and let GitHub recreate the auto-managed `github-pages` environment (a known
   workaround for wedged pending deployments) — Pages silently reconciled `build_type` back to
   `workflow` afterward (the presence of `pages.yml` appears to make GitHub auto-prefer Actions
   mode), and the same run stayed stuck.
5. As a genuinely different code path (not just a retry), created an orphan `gh-pages` branch with
   `docs/`'s contents promoted to its root, pushed it, and pointed Pages at
   `gh-pages`/`/` instead of `main`/`/docs`. **Same symptom** — `building` status, fresh commit
   SHA, zero progress for 6+ minutes, no error ever raised.
6. Checked `githubstatus.com` early in the process — it reported Pages "Operational," which
   pointed the investigation toward "something wrong with this repo" rather than "something wrong
   with GitHub."
7. **User pointed at StatusGator** (`statusgator.com/services/github/actions`), which showed an
   active, acknowledged "Incident with Actions" (Actions **and** Pages both marked Down) — the
   real explanation the whole time.

### Knowledge captured
- **A stuck-but-not-erroring deployment (`building`/`waiting`/`queued` that never resolves and
  never times out with a clear error) across *every* configuration variant tried — different
  build type, different branch, different environment, freshly created — is a strong signal to
  check platform status *before* trying a fourth workaround, not after.**
- **GitHub's own status page can under-report or lag behind an active incident.**
  `githubstatus.com` showed "Pages: Operational" while a third-party aggregator (StatusGator,
  which aggregates community outage reports) showed an active, acknowledged incident affecting
  both Actions and Pages. For future incident triage, check both — don't treat the first-party
  status page as authoritative on its own.
- **`.nojekyll`, environment deletion, and switching build types are all legitimate fixes for
  their respective *specific* failure modes** (an actual Jekyll misconfiguration, a wedged
  pending-deployment approval, a builder-type mismatch) — they're just the wrong tool when the
  actual cause is an upstream outage. None of the changes made this session were wasted, though:
  `.nojekyll`, the Actions workflow, and the `gh-pages` branch are all now in place and correctly
  configured for whenever the outage clears.

### Current state (unresolved, blocked on GitHub, not on us)
- `main` has all content changes committed and pushed (`834d3d6`, `fb5cd64`) — no content is at
  risk.
- `gh-pages` branch exists (`219c1ec7`) with the same `docs/` content at its root, as an alternate
  publish target.
- Pages source is currently `gh-pages`/`/`, `build_type: legacy`.
- Live site (`alexeyban.github.io/EKOS`) is stale until GitHub's Actions/Pages incident resolves —
  no further action expected to help until then.

---

## Also this session (no file changes)

- Compared EKOS against `repowise-dev/repowise` (a code-health/agent-context MCP tool) at the
  user's request — pure research, reported in chat, nothing written to the repo.
  Conclusion: not really a competitor — repowise scores one repo's code health with published
  defect-prediction benchmarks; EKOS compiles a whole enterprise's systems (code + data + legacy
  ETL + docs) into an evidence-backed model. EKOS has no equivalent to repowise's defect-risk
  scoring, dead-code detection, PR bot, or IDE integration.
- Drafted (not posted) LinkedIn and X/Twitter announcement copy for the new deck and for the first
  3 Pioneer Program payouts. No posting tool/credentials are available in this environment
  (`TWITTER_API_KEY`/`API_SECRET`/`ACCESS_TOKEN`/`ACCESS_SECRET` all unset) — `ekos marketing
  publish` (RFC 0030) exists but is devlog-triggered, not built for ad-hoc announcements, so it
  wasn't a fit here either. Flagged that `PIONEER_PROGRAM.md` requires separate consent before
  naming individual pioneers publicly — kept the draft generic pending that confirmation.

---

## Files Changed

| File | Change summary |
|---|---|
| `VISION.md` | New — phased token-utility roadmap, non-promissory framing |
| `TOKENOMICS.md` | Linked to `VISION.md`; scope note added |
| `README.md` | Added Presentations section; linked `VISION.md`/token box |
| `docs/index.html` | Token box copy; Vision link; two new deck entries in Presentations list |
| `docs/presentations.html` | Two new deck entries |
| `docs/presentations/vision-and-token-utility.html` | New deck |
| `docs/presentations/claude-code-with-ekos.html` | New deck, 10 slides, all numbers measured live |
| `docs/presentations/github-repo-to-mcp-server.html` | Fixed stale "seven tools" → eleven; fixed placeholder CTA link |
| `docs/.nojekyll` | New — this site was never a Jekyll site |
| `.github/workflows/pages.yml` | New — Actions-based Pages deployment (currently blocked by the outage, not by its own config) |
| `gh-pages` branch | New — alternate Pages publish target, `docs/` content at root |
