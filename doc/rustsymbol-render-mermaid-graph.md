# render_mermaid_graph (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← build_object_page_model (`0b9a022b-a905-5064-bb70-9f4e04f76875`)
- → mermaid_arrow (`4a306dc2-d546-5c1c-a4d6-8cf1053971f7`)
- → mermaid_node_id (`6da0d0ba-0c8b-5a4e-bb6f-a414381854d9`)

### Contains

- ← ekos/crates/docs-gen/src/lib.rs (`6ca4fba0-18e5-59b7-9876-7dae72e2ce0e`)

## Diagram

```mermaid
graph TD
    n35d5fb7792f2506eba730ffa1f0149a3["render_mermaid_graph"]
    n6ca4fba018e559b798767dae72e2ce0e["ekos/crates/docs-gen/src/lib.rs"]
    n6ca4fba018e559b798767dae72e2ce0e -->|Contains| n35d5fb7792f2506eba730ffa1f0149a3
    n0b9a022ba9055064bb709f4e04f76875["build_object_page_model"]
    n0b9a022ba9055064bb709f4e04f76875 -->|Calls| n35d5fb7792f2506eba730ffa1f0149a3
    n4a306dc2d5465c1ca4d68cf1053971f7["mermaid_arrow"]
    n35d5fb7792f2506eba730ffa1f0149a3 -->|Calls| n4a306dc2d5465c1ca4d68cf1053971f7
    n6da0d0ba0c8b5a4ebb6fa414381854d9["mermaid_node_id"]
    n35d5fb7792f2506eba730ffa1f0149a3 -->|Calls| n6da0d0ba0c8b5a4ebb6fa414381854d9
```

## Evidence

_No evidence cited._
