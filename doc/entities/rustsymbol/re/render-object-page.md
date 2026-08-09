# render_object_page (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- → build_object_page_model (`0b9a022b-a905-5064-bb70-9f4e04f76875`)
- → render_markdown_object_page (`b7354e77-e209-5c90-af84-82da91505df4`)

### Contains

- ← ekos/crates/docs-gen/src/lib.rs (`6ca4fba0-18e5-59b7-9876-7dae72e2ce0e`)

## Diagram

```mermaid
graph TD
    ncdc18413eaf95b539989839fc52293c1["render_object_page"]
    n6ca4fba018e559b798767dae72e2ce0e["ekos/crates/docs-gen/src/lib.rs"]
    n6ca4fba018e559b798767dae72e2ce0e -->|Contains| ncdc18413eaf95b539989839fc52293c1
    n0b9a022ba9055064bb709f4e04f76875["build_object_page_model"]
    ncdc18413eaf95b539989839fc52293c1 -->|Calls| n0b9a022ba9055064bb709f4e04f76875
    nb7354e77e2095c90af8482da91505df4["render_markdown_object_page"]
    ncdc18413eaf95b539989839fc52293c1 -->|Calls| nb7354e77e2095c90af8482da91505df4
```

## Evidence

_No evidence cited._
