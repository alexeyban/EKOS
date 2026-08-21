# column_types (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- → type_family (`49576809-e712-5bd4-91a2-edf1ee60a47d`)
- ← type_compat_score (`20571641-04cb-5dd0-846a-7cb921b2e2be`)

### Contains

- ← ekos/crates/identity/src/cross_system.rs (`44d130a8-ca02-506d-bc1a-21b037fb492c`)

## Diagram

```mermaid
graph TD
    nc13aa3d42f6956d78b30c643c78450ea["column_types"]
    n44d130a8ca02506dbc1a21b037fb492c["ekos/crates/identity/src/cross_system.rs"]
    n44d130a8ca02506dbc1a21b037fb492c -->|Contains| nc13aa3d42f6956d78b30c643c78450ea
    n49576809e7125bd491a2edf1ee60a47d["type_family"]
    nc13aa3d42f6956d78b30c643c78450ea -->|Calls| n49576809e7125bd491a2edf1ee60a47d
    n2057164104cb5dd0846a7cb921b2e2be["type_compat_score"]
    n2057164104cb5dd0846a7cb921b2e2be -->|Calls| nc13aa3d42f6956d78b30c643c78450ea
```

## Evidence

_No evidence cited._
