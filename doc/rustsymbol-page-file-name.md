# page_file_name (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← render_markdown_object_page (`b7354e77-e209-5c90-af84-82da91505df4`)
- ← render_html_object_page (`2c4b2f66-239c-55ad-bcda-4a963de9d84f`)
- ← unique_page_file_names (`136d1c6d-e6f6-5cda-97d8-3454bbc0d5e7`)

### Contains

- ← ekos/crates/docs-gen/src/lib.rs (`6ca4fba0-18e5-59b7-9876-7dae72e2ce0e`)

## Diagram

```mermaid
graph TD
    nd8b69d99022e532f988b43e4370b5981["page_file_name"]
    n6ca4fba018e559b798767dae72e2ce0e["ekos/crates/docs-gen/src/lib.rs"]
    n6ca4fba018e559b798767dae72e2ce0e -->|Contains| nd8b69d99022e532f988b43e4370b5981
    nb7354e77e2095c90af8482da91505df4["render_markdown_object_page"]
    nb7354e77e2095c90af8482da91505df4 -->|Calls| nd8b69d99022e532f988b43e4370b5981
    n2c4b2f66239c55adbcda4a963de9d84f["render_html_object_page"]
    n2c4b2f66239c55adbcda4a963de9d84f -->|Calls| nd8b69d99022e532f988b43e4370b5981
    n136d1c6de6f65cda97d83454bbc0d5e7["unique_page_file_names"]
    n136d1c6de6f65cda97d83454bbc0d5e7 -->|Calls| nd8b69d99022e532f988b43e4370b5981
```

## Evidence

_No evidence cited._
