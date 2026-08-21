# type_compat_score (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- → column_types (`c13aa3d4-2f69-56d7-8b30-c643c78450ea`)
- ← find_cross_system_candidates (`9dd46d31-0b3f-54f2-9bc5-f4deee8ca431`)

### Contains

- ← ekos/crates/identity/src/cross_system.rs (`44d130a8-ca02-506d-bc1a-21b037fb492c`)

## Diagram

```mermaid
graph TD
    n2057164104cb5dd0846a7cb921b2e2be["type_compat_score"]
    n44d130a8ca02506dbc1a21b037fb492c["ekos/crates/identity/src/cross_system.rs"]
    n44d130a8ca02506dbc1a21b037fb492c -->|Contains| n2057164104cb5dd0846a7cb921b2e2be
    nc13aa3d42f6956d78b30c643c78450ea["column_types"]
    n2057164104cb5dd0846a7cb921b2e2be -->|Calls| nc13aa3d42f6956d78b30c643c78450ea
    n9dd46d310b3f54f29bc5f4deee8ca431["find_cross_system_candidates"]
    n9dd46d310b3f54f29bc5f4deee8ca431 -->|Calls| n2057164104cb5dd0846a7cb921b2e2be
```

## Evidence

_No evidence cited._
