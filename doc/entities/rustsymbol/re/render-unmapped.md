# render_unmapped (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← render_dbt_model (`95ef6862-37ab-54fa-99de-4e67f2dffd69`)
- → comment_block (`0536e9e3-6c9e-5a30-85fc-35488a8733c1`)
- → get_str (`92ef0b6b-150b-5f26-bfbd-07900bf340c8`)

### Contains

- ← ekos/crates/dbt-gen/src/lib.rs (`20d45ed0-c411-592d-8034-6486682f898c`)

## Diagram

```mermaid
graph TD
    n130a2e4593d9563fb669b7e6e8bd7900["render_unmapped"]
    n20d45ed0c411592d80346486682f898c["ekos/crates/dbt-gen/src/lib.rs"]
    n20d45ed0c411592d80346486682f898c -->|Contains| n130a2e4593d9563fb669b7e6e8bd7900
    n95ef686237ab54fa99de4e67f2dffd69["render_dbt_model"]
    n95ef686237ab54fa99de4e67f2dffd69 -->|Calls| n130a2e4593d9563fb669b7e6e8bd7900
    n0536e9e36c9e5a3085fc35488a8733c1["comment_block"]
    n130a2e4593d9563fb669b7e6e8bd7900 -->|Calls| n0536e9e36c9e5a3085fc35488a8733c1
    n92ef0b6b150b5f26bfbd07900bf340c8["get_str"]
    n130a2e4593d9563fb669b7e6e8bd7900 -->|Calls| n92ef0b6b150b5f26bfbd07900bf340c8
```

## Evidence

_No evidence cited._
