# AttributeRegistry::get (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- ← AttributeRegistry::intern (`1f29bac1-ffa9-5110-bbe2-6742ce05b657`)
- → AttributeRegistry::get (`fbad72cf-cfbb-5d3a-88c6-3462f9a7860b`)
- ← AttributeRegistry::name (`6f8b73f3-5dc9-536f-a72a-eaa0fd944f51`)
- ← diff (`326570a1-c9d8-552b-8e4d-43921bf22e90`)

### Contains

- ← ekos/crates/ledger/src/fact.rs (`7837f9fa-7178-5151-a068-e75361336c37`)

## Diagram

```mermaid
graph TD
    nfbad72cfcfbb5d3a88c63462f9a7860b["AttributeRegistry::get"]
    n7837f9fa71785151a068e75361336c37["ekos/crates/ledger/src/fact.rs"]
    n7837f9fa71785151a068e75361336c37 -->|Contains| nfbad72cfcfbb5d3a88c63462f9a7860b
    n1f29bac1ffa95110bbe26742ce05b657["AttributeRegistry::intern"]
    n1f29bac1ffa95110bbe26742ce05b657 -->|Calls| nfbad72cfcfbb5d3a88c63462f9a7860b
    nfbad72cfcfbb5d3a88c63462f9a7860b -->|Calls| nfbad72cfcfbb5d3a88c63462f9a7860b
    n6f8b73f35dc9536fa72aeaa0fd944f51["AttributeRegistry::name"]
    n6f8b73f35dc9536fa72aeaa0fd944f51 -->|Calls| nfbad72cfcfbb5d3a88c63462f9a7860b
    n326570a1c9d8552b8e4d43921bf22e90["diff"]
    n326570a1c9d8552b8e4d43921bf22e90 -->|Calls| nfbad72cfcfbb5d3a88c63462f9a7860b
```

## Evidence

_No evidence cited._
