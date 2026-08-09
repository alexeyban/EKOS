# render_architecture (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- → count_by_kind (`dd7cd492-c065-5cee-9bd2-2b5fe1a70310`)
- → render_er_diagram (`24aebc21-f28c-5ad9-8802-3013358450bc`)
- → render_relationship_kind_graph (`dd03b8f9-0881-534a-be1c-53991fa0faba`)
- → is_feeds_into (`41dc0e3d-c14a-5c12-a09f-6f9c44fbff80`)
- → unique_page_file_names (`136d1c6d-e6f6-5cda-97d8-3454bbc0d5e7`)
- → components_cross_reference (`a62d90d2-84d3-53ed-9f41-48432d1a2f15`)

### Contains

- ← ekos/crates/docs-gen/src/lib.rs (`6ca4fba0-18e5-59b7-9876-7dae72e2ce0e`)

## Diagram

```mermaid
graph TD
    nf75f757ec6435e43b04b48d13123d04b["render_architecture"]
    n6ca4fba018e559b798767dae72e2ce0e["ekos/crates/docs-gen/src/lib.rs"]
    n6ca4fba018e559b798767dae72e2ce0e -->|Contains| nf75f757ec6435e43b04b48d13123d04b
    ndd7cd492c0655cee9bd22b5fe1a70310["count_by_kind"]
    nf75f757ec6435e43b04b48d13123d04b -->|Calls| ndd7cd492c0655cee9bd22b5fe1a70310
    n24aebc21f28c5ad988023013358450bc["render_er_diagram"]
    nf75f757ec6435e43b04b48d13123d04b -->|Calls| n24aebc21f28c5ad988023013358450bc
    ndd03b8f90881534abe1c53991fa0faba["render_relationship_kind_graph"]
    nf75f757ec6435e43b04b48d13123d04b -->|Calls| ndd03b8f90881534abe1c53991fa0faba
    n41dc0e3dc14a5c12a09f6f9c44fbff80["is_feeds_into"]
    nf75f757ec6435e43b04b48d13123d04b -->|Calls| n41dc0e3dc14a5c12a09f6f9c44fbff80
    n136d1c6de6f65cda97d83454bbc0d5e7["unique_page_file_names"]
    nf75f757ec6435e43b04b48d13123d04b -->|Calls| n136d1c6de6f65cda97d83454bbc0d5e7
    na62d90d284d353ed9f4148432d1a2f15["components_cross_reference"]
    nf75f757ec6435e43b04b48d13123d04b -->|Calls| na62d90d284d353ed9f4148432d1a2f15
```

## Evidence

_No evidence cited._
