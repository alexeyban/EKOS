# RFC 0173 — Disputed claims in answers: `disputed: true` on `ekos_query` / `ekos_retrieve`

| | |
|---|---|
| **Status** | Implemented (2026-10-11), with the `ekos ask` and table-link amendments below |
| **Depends on** | RFC 0172 (ConflictingEvidence), RFC 0123/0124 (Query Plan IR, `EvidenceSet`, the QUERY tools) |

## Problem

RFC 0172 records where sources disagree, and shows it in `ekos_conflicts` and on `ekos_state`. The two tools an agent actually asks questions with do not show it.

`ekos_query` answers "what columns does `user_preference` have" with one claim, `user_preference.columns = [...]`. That value is the side EKOS kept. LedgerSMB's base schema and one of its migrations define that table differently, and the conflict is open. The agent is told one side as a fact, and nothing tells it to look at `ekos_conflicts`.

RFC 0172 listed this under "Not in this RFC".

## Design

**A claim about a disputed attribute says so.** An `EvidenceItem` gains three fields:

| Field | |
|---|---|
| `attr` | the fact the claim states (`columns`, `label`, …). Only `Fact` claims have one. Left out otherwise. |
| `disputed` | `true` when an open conflict is about that attribute. Left out when false. |
| `conflicts` | the ids of those conflicts, sorted. Left out when empty. |

A conflict's attribute disputes a claim when it equals the claim's `attr` or is nested under it. So `columns.language.data_type` and `columns` both dispute the `columns` claim, and `label` disputes `label`. A conflict about `check_constraints` does not dispute the `columns` claim.

**A claim that is not about the attribute is not flagged.** "`order_items` depends on `user_preference`" stays unflagged when the columns of `user_preference` are disputed: that claim is not what the sources disagree on. Flagging everything near a conflict would teach agents to ignore the flag.

**The set still says the entity has open conflicts.** For every entity in the evidence set that has open conflicts, the set carries one `RSN008` warning that names the entity, the disputed attributes and the conflict ids, and points at `ekos_conflicts`. So a search or graph answer about a disputed object is not silent either.

**Only open conflicts count.** A `resolved` or `dismissed` conflict was decided by a person, and the claim is then reported as settled. Only conflicts current in the latest commit or assessment count, by the same rule as `ekos_state`.

## Where it lives

- `ekos_runtime::reason`: `OpenDispute { conflict, subject, attribute }` and `EvidenceSet::mark_disputed(lookup)`. The Runtime does not know which conflicts are current, because that is recorded in `.ekos/conflicts/*.json` beside the ledger. So the caller passes a lookup from an entity id to its open disputes. The function is pure and asks once per distinct entity.
- `ekos` CLI, `commands/conflicts.rs::disputes_for`: the lookup, through the entity's incoming `Disputes` links, sharing `open_for`'s rule.
- `commands/mcp.rs`: `ekos_query` and `ekos_retrieve` call it when `[conflicts] enabled` (the default). It only reads; the human-only `resolve` path stays unreachable from MCP.

Cost is one `relationships_for` per distinct entity in the answer, the same as `ekos_state` pays for one object.

## Amendment (same day): `ekos ask` and table links

**`ekos ask`.** `AiRuntime::with_disputes(lookup)` runs the same marking inside `gather_evidence`. In the evidence the model reads, a disputed claim's line ends with a fixed marker (`reason::DISPUTED_MARKER`), and each `RSN008` is appended as a `Note:` line, because a search or graph claim states no attribute and is never marked itself. The system prompt is unchanged. So an answer that touches no open conflict gets a byte-identical prompt; a test pins that, and it is why no RFC 0138 eval run was needed. `ekos eval run` does not pass the lookup.

**Label conflicts reach their table.** A `label_mismatch` disputes its `EnumMeaning`, so a question about the table missed it. At commit, `conflicts::column_links` adds a second `Disputes` link from each label conflict to the table its meaning names, when exactly one `Table` has that name. The link carries `attribute = columns.<column>.meaning` and the table's name; the lookup uses the link's attribute when it has one. `ekos_state` on the table lists the conflict too.

## Not in this RFC

- **Console:** see RFC 0174.

## Tests

- A fact claim on a disputed attribute is flagged with the conflict id; a nested conflict attribute flags the parent fact.
- A fact claim on another attribute of the same entity, and a graph claim about it, are not flagged, and the set carries one `RSN008` for the entity.
- An entity with no disputes changes nothing, and the fields are absent from the JSON.
- The lookup is asked once per distinct entity.
- MCP: `ekos_query` on a table with an open conflict flags the `columns` claim; after the conflict is dismissed the flag is gone; with `[conflicts] enabled = false` nothing is flagged.
