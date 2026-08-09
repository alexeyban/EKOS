# linearize_chain (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← try_recognize_chain_statement (`1c28e117-91f6-543a-9528-4a070d7a4528`)
- → linearize_chain (`9a16b56c-b862-5655-af97-28a49dc6c15c`)

### Contains

- ← ekos/crates/recovery/src/python_analyzer.rs (`196ca3fd-e85c-5ab9-a142-50f63dc586b9`)

## Diagram

```mermaid
graph TD
    n9a16b56cb8625655af9728a49dc6c15c["linearize_chain"]
    n196ca3fde85c5ab9a14250f63dc586b9["ekos/crates/recovery/src/python_analyzer.rs"]
    n196ca3fde85c5ab9a14250f63dc586b9 -->|Contains| n9a16b56cb8625655af9728a49dc6c15c
    n1c28e11791f6543a95284a070d7a4528["try_recognize_chain_statement"]
    n1c28e11791f6543a95284a070d7a4528 -->|Calls| n9a16b56cb8625655af9728a49dc6c15c
    n9a16b56cb8625655af9728a49dc6c15c -->|Calls| n9a16b56cb8625655af9728a49dc6c15c
```

## Evidence

_No evidence cited._
