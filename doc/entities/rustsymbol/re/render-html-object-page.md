# render_html_object_page (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- → html_document (`e33469ec-3771-5c0f-a921-6ee66db5430b`)
- → strip_mermaid_fence (`277a818f-2b3f-5dde-8f6a-ae8221017509`)
- → html_escape (`9e97d333-8249-538e-9727-6e1e970dcf37`)
- → page_file_name (`d8b69d99-022e-532f-988b-43e4370b5981`)

### Contains

- ← ekos/crates/docs-gen/src/lib.rs (`6ca4fba0-18e5-59b7-9876-7dae72e2ce0e`)

## Diagram

```mermaid
graph TD
    n2c4b2f66239c55adbcda4a963de9d84f["render_html_object_page"]
    n6ca4fba018e559b798767dae72e2ce0e["ekos/crates/docs-gen/src/lib.rs"]
    n6ca4fba018e559b798767dae72e2ce0e -->|Contains| n2c4b2f66239c55adbcda4a963de9d84f
    ne33469ec37715c0fa9216ee66db5430b["html_document"]
    n2c4b2f66239c55adbcda4a963de9d84f -->|Calls| ne33469ec37715c0fa9216ee66db5430b
    n277a818f2b3f5dde8f6aae8221017509["strip_mermaid_fence"]
    n2c4b2f66239c55adbcda4a963de9d84f -->|Calls| n277a818f2b3f5dde8f6aae8221017509
    n9e97d3338249538e97276e1e970dcf37["html_escape"]
    n2c4b2f66239c55adbcda4a963de9d84f -->|Calls| n9e97d3338249538e97276e1e970dcf37
    nd8b69d99022e532f988b43e4370b5981["page_file_name"]
    n2c4b2f66239c55adbcda4a963de9d84f -->|Calls| nd8b69d99022e532f988b43e4370b5981
```

## Evidence

_No evidence cited._
