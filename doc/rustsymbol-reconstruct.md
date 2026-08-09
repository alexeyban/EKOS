# reconstruct (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- → AttributeRegistry::len (`c441faa0-58d8-5de0-9575-74cc0e087136`)
- → insert_path (`3e14b1d6-e8f1-5a15-b5f9-d69f821b67cb`)
- → AttributeRegistry::name (`6f8b73f3-5dc9-536f-a72a-eaa0fd944f51`)
- → value_to_json (`cec10e58-166c-56a5-bb1f-471d24727e87`)
- → split_path (`dc099dd2-992b-5e15-9906-cd2ebe704310`)

### Contains

- ← ekos/crates/ledger/src/fact.rs (`7837f9fa-7178-5151-a068-e75361336c37`)

## Diagram

```mermaid
graph TD
    n5afc145cc6ce5e309ac552b81dd3b22b["reconstruct"]
    n7837f9fa71785151a068e75361336c37["ekos/crates/ledger/src/fact.rs"]
    n7837f9fa71785151a068e75361336c37 -->|Contains| n5afc145cc6ce5e309ac552b81dd3b22b
    nc441faa058d85de0957574cc0e087136["AttributeRegistry::len"]
    n5afc145cc6ce5e309ac552b81dd3b22b -->|Calls| nc441faa058d85de0957574cc0e087136
    n3e14b1d6e8f15a15b5f9d69f821b67cb["insert_path"]
    n5afc145cc6ce5e309ac552b81dd3b22b -->|Calls| n3e14b1d6e8f15a15b5f9d69f821b67cb
    n6f8b73f35dc9536fa72aeaa0fd944f51["AttributeRegistry::name"]
    n5afc145cc6ce5e309ac552b81dd3b22b -->|Calls| n6f8b73f35dc9536fa72aeaa0fd944f51
    ncec10e58166c56a5bb1f471d24727e87["value_to_json"]
    n5afc145cc6ce5e309ac552b81dd3b22b -->|Calls| ncec10e58166c56a5bb1f471d24727e87
    ndc099dd2992b5e159906cd2ebe704310["split_path"]
    n5afc145cc6ce5e309ac552b81dd3b22b -->|Calls| ndc099dd2992b5e159906cd2ebe704310
```

## Evidence

_No evidence cited._
