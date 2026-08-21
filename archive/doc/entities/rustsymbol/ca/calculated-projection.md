# calculated_projection (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← select_to_graph (`cb80b4b3-9be9-5aae-828e-f0d4f3ec4336`)
- → as_aggregate_function (`a2dbec2a-9c4a-517a-9cc0-1107daa41b02`)
- → is_plain_column (`54075c3d-812b-5d83-9313-d1efae10429f`)

### Contains

- ← ekos/crates/recovery/src/sql_transform_analyzer.rs (`987e3fbb-31ec-576d-b794-7e801336e8c8`)

## Diagram

```mermaid
graph TD
    n6b551d77f70b58678c24e9593a282f1d["calculated_projection"]
    n987e3fbb31ec576db7947e801336e8c8["ekos/crates/recovery/src/sql_transform_analyzer.rs"]
    n987e3fbb31ec576db7947e801336e8c8 -->|Contains| n6b551d77f70b58678c24e9593a282f1d
    ncb80b4b39be95aae828ef0d4f3ec4336["select_to_graph"]
    ncb80b4b39be95aae828ef0d4f3ec4336 -->|Calls| n6b551d77f70b58678c24e9593a282f1d
    na2dbec2a9c4a517a9cc01107daa41b02["as_aggregate_function"]
    n6b551d77f70b58678c24e9593a282f1d -->|Calls| na2dbec2a9c4a517a9cc01107daa41b02
    n54075c3d812b5d839313d1efae10429f["is_plain_column"]
    n6b551d77f70b58678c24e9593a282f1d -->|Calls| n54075c3d812b5d839313d1efae10429f
```

## Evidence

_No evidence cited._
