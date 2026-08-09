# escape_segment (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- → AttributeRegistry::len (`c441faa0-58d8-5de0-9575-74cc0e087136`)
- ← decompose (`a37fe3f9-4d7a-596d-b18f-e9d9d4931c36`)

### Contains

- ← ekos/crates/ledger/src/fact.rs (`7837f9fa-7178-5151-a068-e75361336c37`)

## Diagram

```mermaid
graph TD
    n36efc95306f855fd8c9b56ab9c679642["escape_segment"]
    n7837f9fa71785151a068e75361336c37["ekos/crates/ledger/src/fact.rs"]
    n7837f9fa71785151a068e75361336c37 -->|Contains| n36efc95306f855fd8c9b56ab9c679642
    nc441faa058d85de0957574cc0e087136["AttributeRegistry::len"]
    n36efc95306f855fd8c9b56ab9c679642 -->|Calls| nc441faa058d85de0957574cc0e087136
    na37fe3f94d7a596db18fe9d9d4931c36["decompose"]
    na37fe3f94d7a596db18fe9d9d4931c36 -->|Calls| n36efc95306f855fd8c9b56ab9c679642
```

## Evidence

_No evidence cited._
