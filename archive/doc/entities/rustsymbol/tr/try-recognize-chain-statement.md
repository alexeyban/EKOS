# try_recognize_chain_statement (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← walk_top_level_statement (`36a795d0-3788-5433-a369-2571af84e340`)
- → calls_to_nodes (`73d9612b-4af4-5e0f-b10f-ddfc27015e27`)
- → linearize_chain (`9a16b56c-b862-5655-af97-28a49dc6c15c`)

### Contains

- ← ekos/crates/recovery/src/python_analyzer.rs (`196ca3fd-e85c-5ab9-a142-50f63dc586b9`)

## Diagram

```mermaid
graph TD
    n1c28e11791f6543a95284a070d7a4528["try_recognize_chain_statement"]
    n196ca3fde85c5ab9a14250f63dc586b9["ekos/crates/recovery/src/python_analyzer.rs"]
    n196ca3fde85c5ab9a14250f63dc586b9 -->|Contains| n1c28e11791f6543a95284a070d7a4528
    n36a795d037885433a3692571af84e340["walk_top_level_statement"]
    n36a795d037885433a3692571af84e340 -->|Calls| n1c28e11791f6543a95284a070d7a4528
    n73d9612b4af45e0fb10fddfc27015e27["calls_to_nodes"]
    n1c28e11791f6543a95284a070d7a4528 -->|Calls| n73d9612b4af45e0fb10fddfc27015e27
    n9a16b56cb8625655af9728a49dc6c15c["linearize_chain"]
    n1c28e11791f6543a95284a070d7a4528 -->|Calls| n9a16b56cb8625655af9728a49dc6c15c
```

## Evidence

_No evidence cited._
