# build_object_page_model (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- → render_mermaid_graph (`35d5fb77-92f2-506e-ba73-0ffa1f0149a3`)
- → format_value (`08249438-7eae-5338-8318-b073fd1f7d01`)
- ← render_object_page (`cdc18413-eaf9-5b53-9989-839fc52293c1`)

### Contains

- ← ekos/crates/docs-gen/src/lib.rs (`6ca4fba0-18e5-59b7-9876-7dae72e2ce0e`)

## Diagram

```mermaid
graph TD
    n0b9a022ba9055064bb709f4e04f76875["build_object_page_model"]
    n6ca4fba018e559b798767dae72e2ce0e["ekos/crates/docs-gen/src/lib.rs"]
    n6ca4fba018e559b798767dae72e2ce0e -->|Contains| n0b9a022ba9055064bb709f4e04f76875
    n35d5fb7792f2506eba730ffa1f0149a3["render_mermaid_graph"]
    n0b9a022ba9055064bb709f4e04f76875 -->|Calls| n35d5fb7792f2506eba730ffa1f0149a3
    n082494387eae53388318b073fd1f7d01["format_value"]
    n0b9a022ba9055064bb709f4e04f76875 -->|Calls| n082494387eae53388318b073fd1f7d01
    ncdc18413eaf95b539989839fc52293c1["render_object_page"]
    ncdc18413eaf95b539989839fc52293c1 -->|Calls| n0b9a022ba9055064bb709f4e04f76875
```

## Evidence

_No evidence cited._
