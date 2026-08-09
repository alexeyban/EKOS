# join_node (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← select_to_graph (`cb80b4b3-9be9-5aae-828e-f0d4f3ec4336`)
- → push (`cc223cbc-9afd-5086-9f88-8c6d484895d1`)

### Contains

- ← ekos/crates/recovery/src/sql_transform_analyzer.rs (`987e3fbb-31ec-576d-b794-7e801336e8c8`)

## Diagram

```mermaid
graph TD
    n7eaa67f65bf654009b69a65cf91f7e66["join_node"]
    n987e3fbb31ec576db7947e801336e8c8["ekos/crates/recovery/src/sql_transform_analyzer.rs"]
    n987e3fbb31ec576db7947e801336e8c8 -->|Contains| n7eaa67f65bf654009b69a65cf91f7e66
    ncb80b4b39be95aae828ef0d4f3ec4336["select_to_graph"]
    ncb80b4b39be95aae828ef0d4f3ec4336 -->|Calls| n7eaa67f65bf654009b69a65cf91f7e66
    ncc223cbc9afd50869f888c6d484895d1["push"]
    n7eaa67f65bf654009b69a65cf91f7e66 -->|Calls| ncc223cbc9afd50869f888c6d484895d1
```

## Evidence

_No evidence cited._
