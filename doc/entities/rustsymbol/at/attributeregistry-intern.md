# AttributeRegistry::intern (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → AttributeRegistry::len (`c441faa0-58d8-5de0-9575-74cc0e087136`)
- → AttributeRegistry::get (`fbad72cf-cfbb-5d3a-88c6-3462f9a7860b`)
- ← decompose (`a37fe3f9-4d7a-596d-b18f-e9d9d4931c36`)
- ← flatten (`bf6d717e-62f8-5503-b991-dc1cf3358b97`)

### Contains

- ← ekos/crates/ledger/src/fact.rs (`7837f9fa-7178-5151-a068-e75361336c37`)

## Diagram

```mermaid
graph TD
    n1f29bac1ffa95110bbe26742ce05b657["AttributeRegistry::intern"]
    n7837f9fa71785151a068e75361336c37["ekos/crates/ledger/src/fact.rs"]
    n7837f9fa71785151a068e75361336c37 -->|Contains| n1f29bac1ffa95110bbe26742ce05b657
    nc441faa058d85de0957574cc0e087136["AttributeRegistry::len"]
    n1f29bac1ffa95110bbe26742ce05b657 -->|Calls| nc441faa058d85de0957574cc0e087136
    nfbad72cfcfbb5d3a88c63462f9a7860b["AttributeRegistry::get"]
    n1f29bac1ffa95110bbe26742ce05b657 -->|Calls| nfbad72cfcfbb5d3a88c63462f9a7860b
    na37fe3f94d7a596db18fe9d9d4931c36["decompose"]
    na37fe3f94d7a596db18fe9d9d4931c36 -->|Calls| n1f29bac1ffa95110bbe26742ce05b657
    nbf6d717e62f85503b991dc1cf3358b97["flatten"]
    nbf6d717e62f85503b991dc1cf3358b97 -->|Calls| n1f29bac1ffa95110bbe26742ce05b657
```

## Evidence

_No evidence cited._
