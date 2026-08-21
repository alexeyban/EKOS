# add_import (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- → python_module_kir_id (`12d0dd12-20c5-57da-aca4-f87c55d80182`)
- ← walk_top_level_statement (`36a795d0-3788-5433-a369-2571af84e340`)

### Contains

- ← ekos/crates/recovery/src/python_analyzer.rs (`196ca3fd-e85c-5ab9-a142-50f63dc586b9`)

## Diagram

```mermaid
graph TD
    n89c6ca8d85385378962bdd78b293813c["add_import"]
    n196ca3fde85c5ab9a14250f63dc586b9["ekos/crates/recovery/src/python_analyzer.rs"]
    n196ca3fde85c5ab9a14250f63dc586b9 -->|Contains| n89c6ca8d85385378962bdd78b293813c
    n12d0dd1220c557daaca4f87c55d80182["python_module_kir_id"]
    n89c6ca8d85385378962bdd78b293813c -->|Calls| n12d0dd1220c557daaca4f87c55d80182
    n36a795d037885433a3692571af84e340["walk_top_level_statement"]
    n36a795d037885433a3692571af84e340 -->|Calls| n89c6ca8d85385378962bdd78b293813c
```

## Evidence

_No evidence cited._
