# insert_path (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← reconstruct (`5afc145c-c6ce-5e30-9ac5-52b81dd3b22b`)
- → AttributeRegistry::is_empty (`9cdd0396-a4a6-5ba7-b142-40240858e67e`)
- → insert_path (`3e14b1d6-e8f1-5a15-b5f9-d69f821b67cb`)

### Contains

- ← ekos/crates/ledger/src/fact.rs (`7837f9fa-7178-5151-a068-e75361336c37`)

## Diagram

```mermaid
graph TD
    n3e14b1d6e8f15a15b5f9d69f821b67cb["insert_path"]
    n7837f9fa71785151a068e75361336c37["ekos/crates/ledger/src/fact.rs"]
    n7837f9fa71785151a068e75361336c37 -->|Contains| n3e14b1d6e8f15a15b5f9d69f821b67cb
    n5afc145cc6ce5e309ac552b81dd3b22b["reconstruct"]
    n5afc145cc6ce5e309ac552b81dd3b22b -->|Calls| n3e14b1d6e8f15a15b5f9d69f821b67cb
    n9cdd0396a4a65ba7b14240240858e67e["AttributeRegistry::is_empty"]
    n3e14b1d6e8f15a15b5f9d69f821b67cb -->|Calls| n9cdd0396a4a65ba7b14240240858e67e
    n3e14b1d6e8f15a15b5f9d69f821b67cb -->|Calls| n3e14b1d6e8f15a15b5f9d69f821b67cb
```

## Evidence

_No evidence cited._
