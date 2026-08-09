# extract_aggregates (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← select_to_graph (`cb80b4b3-9be9-5aae-828e-f0d4f3ec4336`)
- → as_aggregate_function (`a2dbec2a-9c4a-517a-9cc0-1107daa41b02`)

### Contains

- ← ekos/crates/recovery/src/sql_transform_analyzer.rs (`987e3fbb-31ec-576d-b794-7e801336e8c8`)

## Diagram

```mermaid
graph TD
    n4f3d3e2a8b6d57b9a2f26c57d683e86f["extract_aggregates"]
    n987e3fbb31ec576db7947e801336e8c8["ekos/crates/recovery/src/sql_transform_analyzer.rs"]
    n987e3fbb31ec576db7947e801336e8c8 -->|Contains| n4f3d3e2a8b6d57b9a2f26c57d683e86f
    ncb80b4b39be95aae828ef0d4f3ec4336["select_to_graph"]
    ncb80b4b39be95aae828ef0d4f3ec4336 -->|Calls| n4f3d3e2a8b6d57b9a2f26c57d683e86f
    na2dbec2a9c4a517a9cc01107daa41b02["as_aggregate_function"]
    n4f3d3e2a8b6d57b9a2f26c57d683e86f -->|Calls| na2dbec2a9c4a517a9cc01107daa41b02
```

## Evidence

_No evidence cited._
