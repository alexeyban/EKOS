# render_api (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- → is_symbol_kind (`c6edd30c-c121-5dad-95d2-155f5391c723`)
- → render_api_from_legacy_file_symbols (`7dbba924-067b-5b9c-9975-9e223d01dc4c`)
- → unique_page_file_names (`136d1c6d-e6f6-5cda-97d8-3454bbc0d5e7`)

### Contains

- ← ekos/crates/docs-gen/src/lib.rs (`6ca4fba0-18e5-59b7-9876-7dae72e2ce0e`)

## Diagram

```mermaid
graph TD
    n586deb177a7c5503a4df9f4430a3b19f["render_api"]
    n6ca4fba018e559b798767dae72e2ce0e["ekos/crates/docs-gen/src/lib.rs"]
    n6ca4fba018e559b798767dae72e2ce0e -->|Contains| n586deb177a7c5503a4df9f4430a3b19f
    nc6edd30cc1215dad95d2155f5391c723["is_symbol_kind"]
    n586deb177a7c5503a4df9f4430a3b19f -->|Calls| nc6edd30cc1215dad95d2155f5391c723
    n7dbba924067b5b9c99759e223d01dc4c["render_api_from_legacy_file_symbols"]
    n586deb177a7c5503a4df9f4430a3b19f -->|Calls| n7dbba924067b5b9c99759e223d01dc4c
    n136d1c6de6f65cda97d83454bbc0d5e7["unique_page_file_names"]
    n586deb177a7c5503a4df9f4430a3b19f -->|Calls| n136d1c6de6f65cda97d83454bbc0d5e7
```

## Evidence

_No evidence cited._
