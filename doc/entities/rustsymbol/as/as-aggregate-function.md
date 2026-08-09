# as_aggregate_function (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← extract_aggregates (`4f3d3e2a-8b6d-57b9-a2f2-6c57d683e86f`)
- ← calculated_projection (`6b551d77-f70b-5867-8c24-e9593a282f1d`)

### Contains

- ← ekos/crates/recovery/src/sql_transform_analyzer.rs (`987e3fbb-31ec-576d-b794-7e801336e8c8`)

## Diagram

```mermaid
graph TD
    na2dbec2a9c4a517a9cc01107daa41b02["as_aggregate_function"]
    n987e3fbb31ec576db7947e801336e8c8["ekos/crates/recovery/src/sql_transform_analyzer.rs"]
    n987e3fbb31ec576db7947e801336e8c8 -->|Contains| na2dbec2a9c4a517a9cc01107daa41b02
    n4f3d3e2a8b6d57b9a2f26c57d683e86f["extract_aggregates"]
    n4f3d3e2a8b6d57b9a2f26c57d683e86f -->|Calls| na2dbec2a9c4a517a9cc01107daa41b02
    n6b551d77f70b58678c24e9593a282f1d["calculated_projection"]
    n6b551d77f70b58678c24e9593a282f1d -->|Calls| na2dbec2a9c4a517a9cc01107daa41b02
```

## Evidence

_No evidence cited._
