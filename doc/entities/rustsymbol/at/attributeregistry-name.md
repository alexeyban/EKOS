# AttributeRegistry::name (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → AttributeRegistry::get (`fbad72cf-cfbb-5d3a-88c6-3462f9a7860b`)
- ← reconstruct (`5afc145c-c6ce-5e30-9ac5-52b81dd3b22b`)

### Contains

- ← ekos/crates/ledger/src/fact.rs (`7837f9fa-7178-5151-a068-e75361336c37`)

## Diagram

```mermaid
graph TD
    n6f8b73f35dc9536fa72aeaa0fd944f51["AttributeRegistry::name"]
    n7837f9fa71785151a068e75361336c37["ekos/crates/ledger/src/fact.rs"]
    n7837f9fa71785151a068e75361336c37 -->|Contains| n6f8b73f35dc9536fa72aeaa0fd944f51
    nfbad72cfcfbb5d3a88c63462f9a7860b["AttributeRegistry::get"]
    n6f8b73f35dc9536fa72aeaa0fd944f51 -->|Calls| nfbad72cfcfbb5d3a88c63462f9a7860b
    n5afc145cc6ce5e309ac552b81dd3b22b["reconstruct"]
    n5afc145cc6ce5e309ac552b81dd3b22b -->|Calls| n6f8b73f35dc9536fa72aeaa0fd944f51
```

## Evidence

_No evidence cited._
