# select_to_graph (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← query_to_graph (`f816648e-a2ad-50bd-bd73-494fdbbbad49`)
- → calculated_projection (`6b551d77-f70b-5867-8c24-e9593a282f1d`)
- → extract_aggregates (`4f3d3e2a-8b6d-57b9-a2f2-6c57d683e86f`)
- → table_factor_node (`e24f2073-e41e-5300-bc61-d0729092e131`)
- → join_node (`7eaa67f6-5bf6-5400-9b69-a65cf91f7e66`)
- → push (`cc223cbc-9afd-5086-9f88-8c6d484895d1`)

### Contains

- ← ekos/crates/recovery/src/sql_transform_analyzer.rs (`987e3fbb-31ec-576d-b794-7e801336e8c8`)

## Diagram

```mermaid
graph TD
    ncb80b4b39be95aae828ef0d4f3ec4336["select_to_graph"]
    n987e3fbb31ec576db7947e801336e8c8["ekos/crates/recovery/src/sql_transform_analyzer.rs"]
    n987e3fbb31ec576db7947e801336e8c8 -->|Contains| ncb80b4b39be95aae828ef0d4f3ec4336
    nf816648ea2ad50bdbd73494fdbbbad49["query_to_graph"]
    nf816648ea2ad50bdbd73494fdbbbad49 -->|Calls| ncb80b4b39be95aae828ef0d4f3ec4336
    n6b551d77f70b58678c24e9593a282f1d["calculated_projection"]
    ncb80b4b39be95aae828ef0d4f3ec4336 -->|Calls| n6b551d77f70b58678c24e9593a282f1d
    n4f3d3e2a8b6d57b9a2f26c57d683e86f["extract_aggregates"]
    ncb80b4b39be95aae828ef0d4f3ec4336 -->|Calls| n4f3d3e2a8b6d57b9a2f26c57d683e86f
    ne24f2073e41e5300bc61d0729092e131["table_factor_node"]
    ncb80b4b39be95aae828ef0d4f3ec4336 -->|Calls| ne24f2073e41e5300bc61d0729092e131
    n7eaa67f65bf654009b69a65cf91f7e66["join_node"]
    ncb80b4b39be95aae828ef0d4f3ec4336 -->|Calls| n7eaa67f65bf654009b69a65cf91f7e66
    ncc223cbc9afd50869f888c6d484895d1["push"]
    ncb80b4b39be95aae828ef0d4f3ec4336 -->|Calls| ncc223cbc9afd50869f888c6d484895d1
```

## Evidence

_No evidence cited._
