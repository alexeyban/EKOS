# EKOS Demo

A live, rehearsable demo of EKOS's Claude Code integration: two skills
(`ekos-knowledge`, `memory`) and six custom subagents, run against a real
compiled estate, across twelve acts. Full script, prompts, and expected
outputs: **[DEMO.md](DEMO.md)**.

This is demo material, not compiler code — no RFC applies (see the note at
the top of DEMO.md).

## Install

```bash
cp demo/agents/*.md ~/.claude/agents/
```

Then in Claude Code, run `/agents` and confirm `estate-scout`,
`impact-analyst`, `memory-keeper`, `estate-architect`, `legacy-logic-recoverer`,
and `identity-reviewer` all appear.

## Run

**Live** (primary mode): open Claude Code from the workspace root
(the directory containing `ekos.toml`) and follow the prompts in
[DEMO.md](DEMO.md) act by act.

**Headless** (rehearsal, transcripts, fallback) — automates Acts 1–8 only;
Acts 9–12 (multi-agent chains, each needing its own scratch workspace built first) are
presented live:

```bash
sh demo/headless.sh          # acts 1-8
sh demo/headless.sh 2 7      # just acts 2 and 7
```

Transcripts land in `demo/transcripts/act-N.md`.

## Before presenting

Run through **Act 0** in [DEMO.md](DEMO.md) — ledger freshness, a fresh MCP
connection, agent installation, and a headless smoke pass.
