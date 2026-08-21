# render_source (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← render_dbt_model (`95ef6862-37ab-54fa-99de-4e67f2dffd69`)
- → get_str (`92ef0b6b-150b-5f26-bfbd-07900bf340c8`)
- → get_str_vec (`3cb9bf32-bfc0-5c54-b647-de3f7ed2c4ae`)

### Contains

- ← ekos/crates/dbt-gen/src/lib.rs (`20d45ed0-c411-592d-8034-6486682f898c`)

## Diagram

```mermaid
graph TD
    n02d30b9221135cbb8b2627374d4a83c9["render_source"]
    n20d45ed0c411592d80346486682f898c["ekos/crates/dbt-gen/src/lib.rs"]
    n20d45ed0c411592d80346486682f898c -->|Contains| n02d30b9221135cbb8b2627374d4a83c9
    n95ef686237ab54fa99de4e67f2dffd69["render_dbt_model"]
    n95ef686237ab54fa99de4e67f2dffd69 -->|Calls| n02d30b9221135cbb8b2627374d4a83c9
    n92ef0b6b150b5f26bfbd07900bf340c8["get_str"]
    n02d30b9221135cbb8b2627374d4a83c9 -->|Calls| n92ef0b6b150b5f26bfbd07900bf340c8
    n3cb9bf32bfc05c54b647de3f7ed2c4ae["get_str_vec"]
    n02d30b9221135cbb8b2627374d4a83c9 -->|Calls| n3cb9bf32bfc05c54b647de3f7ed2c4ae
```

## Evidence

_No evidence cited._
