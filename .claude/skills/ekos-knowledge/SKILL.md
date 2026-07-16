---
name: ekos-knowledge
description: >-
  Query the EKOS enterprise knowledge ledger through the ekos MCP tools
  (ekos_search, ekos_ekl, ekos_neighborhood, ekos_state, ekos_dependents,
  ekos_diff, ekos_status). Use this whenever the user asks what exists across
  their projects, about database tables/schemas, what depends on something,
  what would break if something changed, what changed recently in the
  workspace, who contributes to what, or any question spanning multiple
  repositories — even if they never say "ekos" or "knowledge ledger". Also use
  it when the user mentions EKOS, the ledger, compiled knowledge, or asks for
  an evidence-backed answer about their codebase estate.
---

# Querying the EKOS knowledge ledger

EKOS is a compiler for enterprise knowledge: it scans projects (files, git
history, SQL schemas), compiles what it finds into evidence-backed objects and
relationships, and serves them read-only through MCP tools. Answers from these
tools come with provenance — prefer them over guessing whenever a question is
about *what exists* rather than *what a specific line of code does*.

## Picking the right tool

| You need | Tool | Notes |
|---|---|---|
| An entry point from free text | `ekos_search` | FTS over object names/kinds; trailing `*` = prefix search |
| A precise, filterable listing | `ekos_ekl` | EKL query language — see cheat sheet below |
| What's connected to an object | `ekos_neighborhood` | BFS `depth` hops from an id |
| Impact: "what breaks if X changes?" | `ekos_dependents` | Incoming edges = `dependents`, outgoing = `dependencies` |
| Full detail + proof for one object | `ekos_state` | Object + relationships + **evidence**; `at` (RFC 3339) reconstructs the past |
| "What changed since T?" | `ekos_diff` | `from` required, `to` defaults to now; names resolved |
| Is there data / how much / how fresh | `ekos_status` | Cheap first call when unsure the ledger has anything |

## The investigation pattern

Chain the tools — each answer feeds the next call:

1. **Locate** the concept: `ekos_ekl` when you can name a kind, `ekos_search`
   otherwise. Both return object **ids** — everything else keys off them.
2. **Expand**: `ekos_neighborhood` (what's around it) or `ekos_dependents`
   (directional impact) with the id from step 1.
3. **Prove**: `ekos_state` returns the evidence — the SQL fragment, commit, or
   file that justifies each conclusion. Cite it when the stakes warrant.
4. **Hand off**: evidence carries source paths. To read or edit the actual
   file, switch to your normal file tools (or a code-navigation MCP) with that
   path — EKOS tells you *where and why*, file tools do the rest.

Example — "can I safely rename the customers table?":
`ekos_ekl "FIND Object WHERE kind = 'Table' AND name = 'customers'"` → take id
→ `ekos_dependents` → report the FK holders as the blast radius, with
`ekos_state` evidence if the user wants proof.

## EKL cheat sheet

```
FIND <Object|Relationship>
  [WHERE <field> <op> <value> [AND ...]]
  [FROM '<object name>']
  [ORDER BY <field> [DESC]] [LIMIT <n>] [RETURN <col>, ...]
```

- Operators: `=  !=  <  >  <=  >=  CONTAINS`; string values in single quotes.
- Object kinds that exist in practice: `File`, `Table`, `Person`, `Entity`;
  the taxonomy also defines `Directory`, `Service`, `Api`, `BusinessRule`,
  `BusinessConcept`, `Dataset`, `Column`, `Pipeline`, `Dashboard`, `Model`,
  `Prompt`, `Agent`.
- Relationship kinds: `ForeignKey`, `OwnedBy` (contributor → commit),
  `CoupledWith` (files that change together), `DependsOn`, `Contains`,
  `Calls`, `References`, `Extends`.
- Useful shapes:
  - `FIND Object WHERE kind = 'Table' AND name CONTAINS 'order'`
  - `FIND Relationship WHERE kind = 'ForeignKey' FROM 'orders'`
  - `FIND Object WHERE kind = 'Person'` — who contributes to the workspace

## Honesty rules

- **Knowledge is a snapshot, not live.** Check `ekos_status` /`ekos_diff` when
  currency matters, and say "as of the last build" when reporting. To refresh:
  run `ekos build && ekos recover && ekos compile && ekos commit` in the
  workspace root (the directory containing `ekos.toml` and `.ekos/`).
- **Empty results are answers.** "The ledger has no X" is worth reporting —
  distinguish "not scanned" (check `ekos_status`) from "scanned, absent".
- **Don't use EKOS for symbol-level questions** ("where is this function
  defined / who calls it") — that's line-precise work for LSP-based tools on
  live code. EKOS wins on cross-project, semantic, historical, and
  evidence questions; use each for what it's for.
- Tool errors come back as readable text (`isError: true`) — bad EKL syntax
  or an unknown id is feedback to adjust the call, not a reason to abandon
  the tool.

## This machine's setup

The `ekos` MCP server is registered at user scope and serves the workspace at
`/home/legion/PycharmProjects` (44 projects; config in `ekos.toml` there,
ledger in `.ekos/ledger/ledger.db`). The binary lives at
`/home/legion/PycharmProjects/EKOS/ekos/target/release/ekos` — rebuild with
`cargo build --release -p ekos` from `/home/legion/PycharmProjects/EKOS/ekos`
after changing EKOS itself.
