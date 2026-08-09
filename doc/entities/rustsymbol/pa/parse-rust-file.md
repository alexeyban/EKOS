# parse_rust_file (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← RustAnalyzerPass::run (`8fb40c09-18b3-5b5a-bc8a-00f048756585`)
- → add_symbol (`18bb973a-8ef0-557c-961e-d46286fcab0f`)
- → type_name (`b2c88510-792a-5dc7-8c04-f5e81cf9e666`)
- → add_import (`40de02d0-0c8a-5907-97bf-6f15ecb5b18e`)
- → flatten_use_tree (`5a57188e-5640-5c40-8eb9-08eda1c1a388`)

### Contains

- ← ekos/crates/recovery/src/rust_analyzer.rs (`50cde56d-1f82-53a6-bc71-b5b2f7c711bc`)

## Diagram

```mermaid
graph TD
    n4cb8c94112525ba3be3cb7d55f1a595d["parse_rust_file"]
    n50cde56d1f8253a6bc71b5b2f7c711bc["ekos/crates/recovery/src/rust_analyzer.rs"]
    n50cde56d1f8253a6bc71b5b2f7c711bc -->|Contains| n4cb8c94112525ba3be3cb7d55f1a595d
    n8fb40c0918b35b5abc8a00f048756585["RustAnalyzerPass::run"]
    n8fb40c0918b35b5abc8a00f048756585 -->|Calls| n4cb8c94112525ba3be3cb7d55f1a595d
    n18bb973a8ef0557c961ed46286fcab0f["add_symbol"]
    n4cb8c94112525ba3be3cb7d55f1a595d -->|Calls| n18bb973a8ef0557c961ed46286fcab0f
    nb2c88510792a5dc78c04f5e81cf9e666["type_name"]
    n4cb8c94112525ba3be3cb7d55f1a595d -->|Calls| nb2c88510792a5dc78c04f5e81cf9e666
    n40de02d00c8a590797bf6f15ecb5b18e["add_import"]
    n4cb8c94112525ba3be3cb7d55f1a595d -->|Calls| n40de02d00c8a590797bf6f15ecb5b18e
    n5a57188e56405c408eb908eda1c1a388["flatten_use_tree"]
    n4cb8c94112525ba3be3cb7d55f1a595d -->|Calls| n5a57188e56405c408eb908eda1c1a388
```

## Evidence

_No evidence cited._
