# flatten_use_tree (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← parse_rust_file (`4cb8c941-1252-5ba3-be3c-b7d55f1a595d`)
- → flatten_use_tree (`5a57188e-5640-5c40-8eb9-08eda1c1a388`)

### Contains

- ← ekos/crates/recovery/src/rust_analyzer.rs (`50cde56d-1f82-53a6-bc71-b5b2f7c711bc`)

## Diagram

```mermaid
graph TD
    n5a57188e56405c408eb908eda1c1a388["flatten_use_tree"]
    n50cde56d1f8253a6bc71b5b2f7c711bc["ekos/crates/recovery/src/rust_analyzer.rs"]
    n50cde56d1f8253a6bc71b5b2f7c711bc -->|Contains| n5a57188e56405c408eb908eda1c1a388
    n4cb8c94112525ba3be3cb7d55f1a595d["parse_rust_file"]
    n4cb8c94112525ba3be3cb7d55f1a595d -->|Calls| n5a57188e56405c408eb908eda1c1a388
    n5a57188e56405c408eb908eda1c1a388 -->|Calls| n5a57188e56405c408eb908eda1c1a388
```

## Evidence

_No evidence cited._
