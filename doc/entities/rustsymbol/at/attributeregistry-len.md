# AttributeRegistry::len (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- ← AttributeRegistry::intern (`1f29bac1-ffa9-5110-bbe2-6742ce05b657`)
- → AttributeRegistry::len (`c441faa0-58d8-5de0-9575-74cc0e087136`)
- ← escape_segment (`36efc953-06f8-55fd-8c9b-56ab9c679642`)
- ← reconstruct (`5afc145c-c6ce-5e30-9ac5-52b81dd3b22b`)

### Contains

- ← ekos/crates/ledger/src/fact.rs (`7837f9fa-7178-5151-a068-e75361336c37`)

## Diagram

```mermaid
graph TD
    nc441faa058d85de0957574cc0e087136["AttributeRegistry::len"]
    n7837f9fa71785151a068e75361336c37["ekos/crates/ledger/src/fact.rs"]
    n7837f9fa71785151a068e75361336c37 -->|Contains| nc441faa058d85de0957574cc0e087136
    n1f29bac1ffa95110bbe26742ce05b657["AttributeRegistry::intern"]
    n1f29bac1ffa95110bbe26742ce05b657 -->|Calls| nc441faa058d85de0957574cc0e087136
    nc441faa058d85de0957574cc0e087136 -->|Calls| nc441faa058d85de0957574cc0e087136
    n36efc95306f855fd8c9b56ab9c679642["escape_segment"]
    n36efc95306f855fd8c9b56ab9c679642 -->|Calls| nc441faa058d85de0957574cc0e087136
    n5afc145cc6ce5e309ac552b81dd3b22b["reconstruct"]
    n5afc145cc6ce5e309ac552b81dd3b22b -->|Calls| nc441faa058d85de0957574cc0e087136
```

## Evidence

_No evidence cited._
