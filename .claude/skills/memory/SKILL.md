---
name: memory
description: Routing note for memory in this estate. Use ONLY when the user types /memory explicitly. This skill no longer stores or retrieves anything itself — it names which system owns each kind of memory. Do NOT use it to record or recall anything: for anchored per-codebase notes use the session-memory skill, for querying compiled knowledge use the ekos-knowledge skill.
---

# Memory — routing only

This skill used to keep estate-wide notes as Markdown files indexed through the EKOS ledger. It
was retired on 2026-09-21. Its store had not been written to since 2026-07-17, and it had four
defects that made it actively misleading — most importantly it resolved its own note directory to
the wrong path, so a session would report "no memory notes exist" while notes sat one directory
up.

## Who owns what

| You want to… | Use |
|---|---|
| record or recall a note about **this codebase**, anchored to a real object and flagged when that object changes | the **session-memory** skill (`ekos_session_*`) |
| ask what exists across the estate — tables, services, files, contributors — from compiled knowledge | the **ekos-knowledge** skill (`ekos_search`, `ekos_ekl`, `ekos_state`, …) |
| carry narrative, preferences and working agreements **across sessions** | Claude Code's own memory (`~/.claude/projects/<project>/memory/`, indexed by `MEMORY.md`) |

Those three do not overlap: the first is per-codebase and object-anchored, the second is read-only
over compiled facts, the third is free-text and cross-session.

## The old notes

Nine legacy notes remain on disk at `/home/legion/PycharmProjects/memory/`, untouched. Read them
directly if you need them; nothing indexes them any more. They follow a
`<scope>--<type>--<keywords>.md` convention.

## Why the retrieval instructions are gone

They told the agent to run `ekos_search "..."` as a shell command. There is no such binary and no
such CLI subcommand — `ekos_search` is an MCP tool (`mcp__ekos__ekos_search`). Use ekos-knowledge,
which gets this right.
