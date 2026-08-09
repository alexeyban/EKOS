# flatten (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← decompose (`a37fe3f9-4d7a-596d-b18f-e9d9d4931c36`)
- → AttributeRegistry::is_empty (`9cdd0396-a4a6-5ba7-b142-40240858e67e`)
- → flatten (`bf6d717e-62f8-5503-b991-dc1cf3358b97`)
- → AttributeRegistry::intern (`1f29bac1-ffa9-5110-bbe2-6742ce05b657`)

### Contains

- ← ekos/crates/ledger/src/fact.rs (`7837f9fa-7178-5151-a068-e75361336c37`)

## Diagram

```mermaid
graph TD
    nbf6d717e62f85503b991dc1cf3358b97["flatten"]
    n7837f9fa71785151a068e75361336c37["ekos/crates/ledger/src/fact.rs"]
    n7837f9fa71785151a068e75361336c37 -->|Contains| nbf6d717e62f85503b991dc1cf3358b97
    na37fe3f94d7a596db18fe9d9d4931c36["decompose"]
    na37fe3f94d7a596db18fe9d9d4931c36 -->|Calls| nbf6d717e62f85503b991dc1cf3358b97
    n9cdd0396a4a65ba7b14240240858e67e["AttributeRegistry::is_empty"]
    nbf6d717e62f85503b991dc1cf3358b97 -->|Calls| n9cdd0396a4a65ba7b14240240858e67e
    nbf6d717e62f85503b991dc1cf3358b97 -->|Calls| nbf6d717e62f85503b991dc1cf3358b97
    n1f29bac1ffa95110bbe26742ce05b657["AttributeRegistry::intern"]
    nbf6d717e62f85503b991dc1cf3358b97 -->|Calls| n1f29bac1ffa95110bbe26742ce05b657
```

## Evidence

_No evidence cited._
