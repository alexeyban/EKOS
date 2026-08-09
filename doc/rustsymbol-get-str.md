# get_str (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← render_dbt_model (`95ef6862-37ab-54fa-99de-4e67f2dffd69`)
- ← render_source (`02d30b92-2113-5cbb-8b26-27374d4a83c9`)
- ← render_join (`38a7c372-2ec4-5a38-a5fd-53f7c4bbf55c`)
- ← render_filter (`4976bdef-e8ee-5e02-9603-fe82d738e9c7`)
- ← render_calculate (`b7519843-e48d-57b7-ac81-6fdf21a72cdd`)
- ← render_unmapped (`130a2e45-93d9-563f-b669-b7e6e8bd7900`)

### Contains

- ← ekos/crates/dbt-gen/src/lib.rs (`20d45ed0-c411-592d-8034-6486682f898c`)

## Diagram

```mermaid
graph TD
    n92ef0b6b150b5f26bfbd07900bf340c8["get_str"]
    n20d45ed0c411592d80346486682f898c["ekos/crates/dbt-gen/src/lib.rs"]
    n20d45ed0c411592d80346486682f898c -->|Contains| n92ef0b6b150b5f26bfbd07900bf340c8
    n95ef686237ab54fa99de4e67f2dffd69["render_dbt_model"]
    n95ef686237ab54fa99de4e67f2dffd69 -->|Calls| n92ef0b6b150b5f26bfbd07900bf340c8
    n02d30b9221135cbb8b2627374d4a83c9["render_source"]
    n02d30b9221135cbb8b2627374d4a83c9 -->|Calls| n92ef0b6b150b5f26bfbd07900bf340c8
    n38a7c3722ec45a38a5fd53f7c4bbf55c["render_join"]
    n38a7c3722ec45a38a5fd53f7c4bbf55c -->|Calls| n92ef0b6b150b5f26bfbd07900bf340c8
    n4976bdefe8ee5e029603fe82d738e9c7["render_filter"]
    n4976bdefe8ee5e029603fe82d738e9c7 -->|Calls| n92ef0b6b150b5f26bfbd07900bf340c8
    nb7519843e48d57b7ac816fdf21a72cdd["render_calculate"]
    nb7519843e48d57b7ac816fdf21a72cdd -->|Calls| n92ef0b6b150b5f26bfbd07900bf340c8
    n130a2e4593d9563fb669b7e6e8bd7900["render_unmapped"]
    n130a2e4593d9563fb669b7e6e8bd7900 -->|Calls| n92ef0b6b150b5f26bfbd07900bf340c8
```

## Evidence

_No evidence cited._
