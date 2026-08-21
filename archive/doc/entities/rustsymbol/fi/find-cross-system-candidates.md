# find_cross_system_candidates (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- → normalize_cross_system (`70e3e344-be1f-53b0-b84d-ed93f29ff4e4`)
- → type_compat_score (`20571641-04cb-5dd0-846a-7cb921b2e2be`)
- → matchable_name (`890f9527-fef5-5556-bf1c-41613f51627e`)
- → column_overlap_score (`443f6bee-3576-5436-9013-bb79b5867fd2`)
- → combine_signals (`6522c3f2-5c8a-5d32-81c9-b0e4f61a4750`)

### Contains

- ← ekos/crates/identity/src/cross_system.rs (`44d130a8-ca02-506d-bc1a-21b037fb492c`)

## Diagram

```mermaid
graph TD
    n9dd46d310b3f54f29bc5f4deee8ca431["find_cross_system_candidates"]
    n44d130a8ca02506dbc1a21b037fb492c["ekos/crates/identity/src/cross_system.rs"]
    n44d130a8ca02506dbc1a21b037fb492c -->|Contains| n9dd46d310b3f54f29bc5f4deee8ca431
    n70e3e344be1f53b0b84ded93f29ff4e4["normalize_cross_system"]
    n9dd46d310b3f54f29bc5f4deee8ca431 -->|Calls| n70e3e344be1f53b0b84ded93f29ff4e4
    n2057164104cb5dd0846a7cb921b2e2be["type_compat_score"]
    n9dd46d310b3f54f29bc5f4deee8ca431 -->|Calls| n2057164104cb5dd0846a7cb921b2e2be
    n890f9527fef55556bf1c41613f51627e["matchable_name"]
    n9dd46d310b3f54f29bc5f4deee8ca431 -->|Calls| n890f9527fef55556bf1c41613f51627e
    n443f6bee357654369013bb79b5867fd2["column_overlap_score"]
    n9dd46d310b3f54f29bc5f4deee8ca431 -->|Calls| n443f6bee357654369013bb79b5867fd2
    n6522c3f25c8a5d3281c9b0e4f61a4750["combine_signals"]
    n9dd46d310b3f54f29bc5f4deee8ca431 -->|Calls| n6522c3f25c8a5d3281c9b0e4f61a4750
```

## Evidence

_No evidence cited._
