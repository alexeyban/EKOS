# decompose (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- → AttributeRegistry::intern (`1f29bac1-ffa9-5110-bbe2-6742ce05b657`)
- → type_name (`94c95a8b-e28c-584d-ba0f-e8212edd1910`)
- → AttributeRegistry::is_empty (`9cdd0396-a4a6-5ba7-b142-40240858e67e`)
- → escape_segment (`36efc953-06f8-55fd-8c9b-56ab9c679642`)
- → flatten (`bf6d717e-62f8-5503-b991-dc1cf3358b97`)

### Contains

- ← ekos/crates/ledger/src/fact.rs (`7837f9fa-7178-5151-a068-e75361336c37`)

## Diagram

```mermaid
graph TD
    na37fe3f94d7a596db18fe9d9d4931c36["decompose"]
    n7837f9fa71785151a068e75361336c37["ekos/crates/ledger/src/fact.rs"]
    n7837f9fa71785151a068e75361336c37 -->|Contains| na37fe3f94d7a596db18fe9d9d4931c36
    n1f29bac1ffa95110bbe26742ce05b657["AttributeRegistry::intern"]
    na37fe3f94d7a596db18fe9d9d4931c36 -->|Calls| n1f29bac1ffa95110bbe26742ce05b657
    n94c95a8be28c584dba0fe8212edd1910["type_name"]
    na37fe3f94d7a596db18fe9d9d4931c36 -->|Calls| n94c95a8be28c584dba0fe8212edd1910
    n9cdd0396a4a65ba7b14240240858e67e["AttributeRegistry::is_empty"]
    na37fe3f94d7a596db18fe9d9d4931c36 -->|Calls| n9cdd0396a4a65ba7b14240240858e67e
    n36efc95306f855fd8c9b56ab9c679642["escape_segment"]
    na37fe3f94d7a596db18fe9d9d4931c36 -->|Calls| n36efc95306f855fd8c9b56ab9c679642
    nbf6d717e62f85503b991dc1cf3358b97["flatten"]
    na37fe3f94d7a596db18fe9d9d4931c36 -->|Calls| nbf6d717e62f85503b991dc1cf3358b97
```

## Evidence

_No evidence cited._
