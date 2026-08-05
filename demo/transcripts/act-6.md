# Act 6 transcript

**Prompt:** What changed across my workspace in the last week?

```
Here's what happened in the EKOS workspace over the last 7 days, based on `git log` (the ledger itself is stale — 0 new objects/relationships since 2026-07-29, so it needs a rebuild if you want EKOS-native queries).

**Major features**
- **RFC 0031 — pluggable SQL dialect parsers** (`8df7939`, `d3e5a35`): new `sql-dialect-sdk` crate + `sql-dialect-mysql`/`sql-dialect-postgres` plugins, dialect registry, MySQL hash-comment test fixtures, benchmark updated for the new `parse_ddl_structural` signature.
- **Cross-system identity fix** (`8b15427`): SQL↔Pentaho exact table-name matches were being missed by identity resolution; also added two new pitch/recovery presentation HTML docs and MCP/runtime tweaks.
- **StreamLookup Pentaho + SQL statement-separator fix** (`4c41b53`): new `statement_repair.rs` module, fixes in `pentaho_analyzer.rs`, `sql_analyzer.rs`, `sql_transform_analyzer.rs`.
- **Marketing agent polish** (`54b20f7`): auto-loads `marketing/.env` for `ekos marketing publish`.

**Docs / non-code**
- New `PIONEER_PROGRAM.md` (created, then extended twice with signup link, consent section, payment SLA, and a link to GitHub Discussion #4).
- New `TOKENOMICS.md`, README updates (Windows/macOS install instructions, X account, contract address, Founder Vesting Wallet section).
- `devlog_31.md` and `devlog_32.md` added, recording the identity-resolution fix and RFC 0031 respectively.

**Uncommitted right now** (per `git status`): edits to `README.md`, `demo/DEMO.md`, `demo/headless.sh`, all `demo/transcripts/act-*.md`, `ekos.toml`, and marketing crate files (`prompt.rs`, `tweet.rs`, `commands/marketing.rs`), plus two untracked files: `docs/presentations/recovery-gaps-closed.html` and `ekos-marketing-agent-plan.md` — this looks like in-progress work on a marketing-agent plan and demo refresh, not yet committed.

Want me to rebuild the EKOS ledger (`ekos build && ekos recover && ekos compile && ekos commit`) so future knowledge-based queries reflect this week's work?
```
