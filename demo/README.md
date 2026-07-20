# EKOS Demo

A live, rehearsable demo of EKOS's Claude Code integration: two skills
(`ekos-knowledge`, `memory`) and four custom subagents, run against a real
compiled estate. Full script, prompts, and expected outputs: **[DEMO.md](DEMO.md)**.

This is demo material, not compiler code — no RFC applies (see the note at
the top of DEMO.md).

## Install

```bash
cp demo/agents/*.md ~/.claude/agents/
```

Then in Claude Code, run `/agents` and confirm `estate-scout`,
`impact-analyst`, `memory-keeper`, and `estate-architect` all appear.

## Run

**Live** (primary mode): open Claude Code from the workspace root
(the directory containing `ekos.toml`) and follow the prompts in
[DEMO.md](DEMO.md) act by act.

**Headless** (rehearsal, transcripts, fallback):

```bash
sh demo/headless.sh          # all seven acts
sh demo/headless.sh 2 7      # just acts 2 and 7
```

Transcripts land in `demo/transcripts/act-N.md`.

## Before presenting

Run through **Act 0** in [DEMO.md](DEMO.md) — ledger freshness, a fresh MCP
connection, agent installation, and a headless smoke pass.
