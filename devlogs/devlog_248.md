# Devlog 248 — Disputed claims in answers (RFC 0173)

**Date:** 2026-10-11
**PRs:** none — committed to main, local gates green, `[skip ci]`
**Branch:** main

---

## Summary

RFC 0172 records where sources disagree, but the two tools an agent asks questions with still reported the side EKOS kept as a plain fact. `ekos_query` and `ekos_retrieve` now mark a claim `disputed: true`, with the conflict ids, when an open conflict is about the attribute the claim states. The answer also carries one `RSN008` warning per entity that has open conflicts. Only open conflicts count, so a decision by a person makes the claim settled again.

---

## RFC 0173 — `disputed: true` on query claims

### Problem / motivation

On LedgerSMB, `ekos_query "user_preference columns"` returned `user_preference.columns = [...]` and nothing else. The base schema and a 1.9 migration define that table differently and the conflict is open. An agent had to know to call `ekos_conflicts` separately. RFC 0172 listed this under "Not in this RFC".

### What was built

| Component | |
|---|---|
| `EvidenceItem.attr` | the fact a `Fact` claim states (`columns`, `label`, …); absent on search and graph claims |
| `EvidenceItem.disputed` / `.conflicts` | set when an open conflict is about that attribute; both absent from the JSON otherwise |
| `reason::OpenDispute`, `EvidenceSet::mark_disputed(lookup)` | pure; asks the lookup once per distinct entity; adds `RSN008` per disputed entity |
| `commands/conflicts.rs::flag_disputed` | the lookup, over the entity's incoming `Disputes` links, sharing `open_for`'s rule through `open_items_for` |
| `commands/mcp.rs` | both query tools call it when `[conflicts] enabled`; tool descriptions say what the flag means |

### Implementation details worth remembering

- **Matching is by attribute path.** A conflict's attribute disputes a claim when it equals the claim's `attr` or is nested under it: `columns.language.data_type` disputes the `columns` fact. `schema_owner` does not dispute `schema`; a test pins that.
- **The Runtime takes a lookup; it does not look up.** Which conflicts are current is recorded in `.ekos/conflicts/current.json` and `migrate.json`, beside the ledger. The Runtime only sees the ledger, where a conflict that has since disappeared still exists. So the CLI supplies the lookup, and reads the manifests once per answer.
- **A plan that reads one fact and then all facts** (`Compose[Fact columns, Fact *]`, the shape "X columns" compiles to) returns the `columns` claim twice. Both are flagged.

### Decisions

- **Flag the claim about the attribute, not every claim near the entity.** "`order_items` depends on `user_preference`" is not what the sources disagree on. A flag on everything near a conflict would be ignored. The `RSN008` note covers the entity-level case.
- **`ekos ask` is not changed.** Marking the rendered evidence changes the prompt, and that needs an RFC 0138 eval run before it ships. It is a TODO item.
- **`resolved` and `dismissed` are not flagged.** A person decided; reporting those as disputed would make the review pointless.

### Verified on LedgerSMB

`ekos_query "user_preference columns"` over `demo/ledgersmb-linkml/work/LedgerSMB`: 8 claims, the 2 `columns` claims flagged with conflict `cc57a3dc-…`, the other 6 (`name`, `kind`, `check_constraints`, `description`, `seed_rows`, `sql_comment`) not flagged, and one `RSN008`.

---

## Knowledge Captured

- **A `label_mismatch` disputes its `EnumMeaning`, not the table.** A question about `oe` does not surface the `oe_class_id` label conflicts unless the meaning object is in the evidence set. Linking a meaning's conflict to its column's table is a separate change.
- **`commands/mcp.rs` must not contain `Resolution::`** (the RFC 0172 source-scan guard). An MCP test that needs a decided conflict writes a new version with `status` set directly.
- **Adding a field to `EvidenceItem` touches five struct literals**, two of them in `ai.rs` tests. It has no `Default`.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0173-disputed-claims-in-answers.md` | new RFC |
| `ekos/crates/runtime/src/reason.rs` | `attr`/`disputed`/`conflicts`, `OpenDispute`, `mark_disputed`, `RSN008`, 4 tests |
| `ekos/crates/runtime/src/ai.rs` | test literals |
| `ekos/crates/cli/src/commands/conflicts.rs` | `open_items_for`, `flag_disputed` |
| `ekos/crates/cli/src/commands/mcp.rs` | both query tools flag disputes; descriptions; 1 end-to-end test |
| `ekos/docs/rfcs/0172-conflicting-evidence.md` | points at RFC 0173 |
| `README.md`, `CHANGELOG.md`, `CLAUDE.md`, `TODO.md`, `docs/generated/ekos-self-documentation.html` | the flag documented; TODO item ticked |
