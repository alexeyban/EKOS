# Devlog 356 — A short README, and why comparable projects have the stars

**Date:** 2026-11-27
**PRs:** none (direct to main)
**Branch:** main

---

## Summary

The README had grown to 2,995 lines: every RFC added a section, and the first screen explained
EKOS in its own vocabulary. It is now 176 lines that say what EKOS does, how to start and what
was measured. The old text is kept whole as `docs/REFERENCE.md`. The ekos.dev homepage got the
same headline and lede (site repo, commit 5f4502d). The change follows a comparison with
projects in the same space that have tens of thousands of stars.

---

## The comparison

Star counts read from the GitHub API on 2026-11-27. EKOS: 28.

| Project | Stars | Created | First line |
|---|---|---|---|
| Graphify-Labs/graphify | 153,967 | 2026-04 | Turn any codebase … into a queryable knowledge graph |
| mem0ai/mem0 | 69,526 | 2023-06 | The Memory Layer for AI Agents |
| DeusData/codebase-memory-mcp | 49,234 | 2026-02 | Local code intelligence for developers and AI agents |
| abhigyanpatwari/GitNexus | 48,932 | 2025-08 | indexes a codebase into a knowledge graph |
| HKUDS/LightRAG | 40,896 | 2024-10 | Simple and Fast Retrieval-Augmented Generation |
| microsoft/graphrag | 36,762 | 2024-03 | graph-based RAG |
| topoteretes/cognee | 33,662 | 2023-08 | open-source AI memory platform for agents |
| getzep/graphiti | 33,131 | 2024-08 | Real-Time Knowledge Graphs for AI Agents |
| tirth8205/code-review-graph | 32,959 | 2026-02 | Local-first code intelligence graph for MCP and CLI |
| oraios/serena | 31,740 | 2025-03 | MCP toolkit for coding |
| vanna-ai/vanna | 23,784 | 2023-05 | Chat with your SQL database |
| cube-js/cube | 21,301 | 2018-09 | open-source semantic layer |
| Canner/WrenAI | 18,413 | 2024-03 | governed text-to-SQL through an open context layer |
| open-metadata/OpenMetadata | 15,891 | 2021-08 | Open Context Layer for Data and AI |
| datahub-project/datahub | 13,027 | 2015-11 | Context Platform for your Data and AI Stack |
| linkml/linkml | 709 | 2021-03 | Linked Open Data Modeling Language |

What the high-star READMEs share, and what EKOS's did not have:

1. **One sentence a stranger understands**, naming one user and one result. EKOS opened with
   "Enterprise Knowledge Operating System", "Canonical Knowledge Model" and "Semantic Knowledge
   Ledger".
2. **A picture or a GIF in the first screen.** EKOS had none.
3. **Value in one or two commands** (`npx gitnexus analyze`; `pip install` then `/graphify`).
   EKOS needs an installer and then five pipeline verbs.
4. **A package-manager install** (npm, PyPI). EKOS has a script and installers; it is not on
   crates.io, Homebrew or npm.
5. **A README of 200–900 lines** with details in `docs/`. EKOS: 2,995, headed by RFC numbers.
6. **A developer community link** (Discord), translations, a Trendshift badge.
7. **The audience is every developer with a coding agent.** The pure ontology project in the
   list, LinkML, has 709 stars: semantics alone does not draw stars; a daily-use developer tool
   does. EKOS's deepest work (Oracle, Talend, Pentaho, sign-off) serves a small, paying audience
   that rarely stars repositories.

---

## What was changed

| Item | Change |
|---|---|
| `README.md` | New: tagline, diagram, four differences, quick start, example questions, how it works, measured results, sources, comparison table, links |
| `docs/REFERENCE.md` | The previous README, moved; relative links rewritten; one link that was already broken fixed |
| `CLAUDE.md` | Devlog rule: feature text goes to `docs/REFERENCE.md`; the README stays short |
| ekos.dev `index.html` | Headline and lede in plain words, example questions, BIRD figure in the stat row, demo videos above the roadmap, nav no longer wraps |

### Decisions

- The reference was moved, not cut: every section in it is still linked from the README.
- The team section and the Product Hunt badge left the README; both stay on ekos.dev.
- No output sample in the README: the CLI's query output is rows of ids, which is not something
  to show a newcomer. A recorded terminal session is the open item.

---

## Knowledge Captured

- The devlog rule used to say "update README.md" for each feature. That rule is what produced a
  2,995-line README. New feature text goes to `docs/REFERENCE.md`.
- Not done, in order of expected effect: a one-command first run (`ekos build … commit` as one
  verb, needs an RFC); a 30-second recording for the README's first screen; publishing to
  crates.io / Homebrew (crates.io groundwork exists since RFC 0153); a Discord or other developer
  channel; a Show HN post once the first three exist.
- The site's top navigation carries "Token" and "Sentry" (a Solana token and a Telegram bot).
  None of the compared projects mixes a token with a developer tool. Left in place: it is the
  owners' decision.

---

## Files Changed

| File | Change summary |
|---|---|
| `README.md` | Rewritten, 176 lines |
| `docs/REFERENCE.md` | The former README |
| `docs/assets/readme-flow.svg` | Diagram for the README |
| `docs/deployment.md`, `VISION.md`, `CLAUDE.md` | Pointers to the reference |
