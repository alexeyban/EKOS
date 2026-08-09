# walk_top_level_statement (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← parse_python_file (`72ff02a8-b7ba-5481-b85d-c5b408ab50e4`)
- → try_recognize_chain_statement (`1c28e117-91f6-543a-9528-4a070d7a4528`)
- → add_symbol (`458e9ef2-0f1a-57ae-8965-7f762081285d`)
- → add_import (`89c6ca8d-8538-5378-962b-dd78b293813c`)

### Contains

- ← ekos/crates/recovery/src/python_analyzer.rs (`196ca3fd-e85c-5ab9-a142-50f63dc586b9`)

## Diagram

```mermaid
graph TD
    n36a795d037885433a3692571af84e340["walk_top_level_statement"]
    n196ca3fde85c5ab9a14250f63dc586b9["ekos/crates/recovery/src/python_analyzer.rs"]
    n196ca3fde85c5ab9a14250f63dc586b9 -->|Contains| n36a795d037885433a3692571af84e340
    n72ff02a8b7ba5481b85dc5b408ab50e4["parse_python_file"]
    n72ff02a8b7ba5481b85dc5b408ab50e4 -->|Calls| n36a795d037885433a3692571af84e340
    n1c28e11791f6543a95284a070d7a4528["try_recognize_chain_statement"]
    n36a795d037885433a3692571af84e340 -->|Calls| n1c28e11791f6543a95284a070d7a4528
    n458e9ef20f1a57ae89657f762081285d["add_symbol"]
    n36a795d037885433a3692571af84e340 -->|Calls| n458e9ef20f1a57ae89657f762081285d
    n89c6ca8d85385378962bdd78b293813c["add_import"]
    n36a795d037885433a3692571af84e340 -->|Calls| n89c6ca8d85385378962bdd78b293813c
```

## Evidence

_No evidence cited._
