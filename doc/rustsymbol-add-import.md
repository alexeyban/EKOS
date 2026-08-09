# add_import (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← parse_rust_file (`4cb8c941-1252-5ba3-be3c-b7d55f1a595d`)
- → rust_module_kir_id (`22873165-bc32-5c38-a219-22586895465f`)

### Contains

- ← ekos/crates/recovery/src/rust_analyzer.rs (`50cde56d-1f82-53a6-bc71-b5b2f7c711bc`)

## Diagram

```mermaid
graph TD
    n40de02d00c8a590797bf6f15ecb5b18e["add_import"]
    n50cde56d1f8253a6bc71b5b2f7c711bc["ekos/crates/recovery/src/rust_analyzer.rs"]
    n50cde56d1f8253a6bc71b5b2f7c711bc -->|Contains| n40de02d00c8a590797bf6f15ecb5b18e
    n4cb8c94112525ba3be3cb7d55f1a595d["parse_rust_file"]
    n4cb8c94112525ba3be3cb7d55f1a595d -->|Calls| n40de02d00c8a590797bf6f15ecb5b18e
    n22873165bc325c38a21922586895465f["rust_module_kir_id"]
    n40de02d00c8a590797bf6f15ecb5b18e -->|Calls| n22873165bc325c38a21922586895465f
```

## Evidence

_No evidence cited._
