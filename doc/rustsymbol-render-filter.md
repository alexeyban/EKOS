# render_filter (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← render_dbt_model (`95ef6862-37ab-54fa-99de-4e67f2dffd69`)
- → no_upstream_placeholder (`0a8b417e-5dc9-5199-9efe-5d7b8d2b02b5`)
- → get_str (`92ef0b6b-150b-5f26-bfbd-07900bf340c8`)

### Contains

- ← ekos/crates/dbt-gen/src/lib.rs (`20d45ed0-c411-592d-8034-6486682f898c`)

## Diagram

```mermaid
graph TD
    n4976bdefe8ee5e029603fe82d738e9c7["render_filter"]
    n20d45ed0c411592d80346486682f898c["ekos/crates/dbt-gen/src/lib.rs"]
    n20d45ed0c411592d80346486682f898c -->|Contains| n4976bdefe8ee5e029603fe82d738e9c7
    n95ef686237ab54fa99de4e67f2dffd69["render_dbt_model"]
    n95ef686237ab54fa99de4e67f2dffd69 -->|Calls| n4976bdefe8ee5e029603fe82d738e9c7
    n0a8b417e5dc951999efe5d7b8d2b02b5["no_upstream_placeholder"]
    n4976bdefe8ee5e029603fe82d738e9c7 -->|Calls| n0a8b417e5dc951999efe5d7b8d2b02b5
    n92ef0b6b150b5f26bfbd07900bf340c8["get_str"]
    n4976bdefe8ee5e029603fe82d738e9c7 -->|Calls| n92ef0b6b150b5f26bfbd07900bf340c8
```

## Evidence

_No evidence cited._
