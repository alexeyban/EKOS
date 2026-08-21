# render_relationship_kind_graph (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← render_architecture (`f75f757e-c643-5e43-b04b-48d13123d04b`)
- → mermaid_arrow (`4a306dc2-d546-5c1c-a4d6-8cf1053971f7`)

### Contains

- ← ekos/crates/docs-gen/src/lib.rs (`6ca4fba0-18e5-59b7-9876-7dae72e2ce0e`)

## Diagram

```mermaid
graph TD
    ndd03b8f90881534abe1c53991fa0faba["render_relationship_kind_graph"]
    n6ca4fba018e559b798767dae72e2ce0e["ekos/crates/docs-gen/src/lib.rs"]
    n6ca4fba018e559b798767dae72e2ce0e -->|Contains| ndd03b8f90881534abe1c53991fa0faba
    nf75f757ec6435e43b04b48d13123d04b["render_architecture"]
    nf75f757ec6435e43b04b48d13123d04b -->|Calls| ndd03b8f90881534abe1c53991fa0faba
    n4a306dc2d5465c1ca4d68cf1053971f7["mermaid_arrow"]
    ndd03b8f90881534abe1c53991fa0faba -->|Calls| n4a306dc2d5465c1ca4d68cf1053971f7
```

## Evidence

_No evidence cited._
