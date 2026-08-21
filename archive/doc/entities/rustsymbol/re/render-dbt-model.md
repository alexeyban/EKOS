# render_dbt_model (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- → dbt_model_name (`4e0e3bc6-d261-5d55-a9da-1bd8f3173cf8`)
- → render_unmapped (`130a2e45-93d9-563f-b669-b7e6e8bd7900`)
- → render_join (`38a7c372-2ec4-5a38-a5fd-53f7c4bbf55c`)
- → render_sink (`f037d176-e9fa-5cab-a531-f7995e09727b`)
- → render_calculate (`b7519843-e48d-57b7-ac81-6fdf21a72cdd`)
- → render_filter (`4976bdef-e8ee-5e02-9603-fe82d738e9c7`)
- → render_source (`02d30b92-2113-5cbb-8b26-27374d4a83c9`)
- → get_str (`92ef0b6b-150b-5f26-bfbd-07900bf340c8`)
- → render_aggregate (`f5e30215-3166-5317-b596-b7e4503b3607`)

### Contains

- ← ekos/crates/dbt-gen/src/lib.rs (`20d45ed0-c411-592d-8034-6486682f898c`)

## Diagram

```mermaid
graph TD
    n95ef686237ab54fa99de4e67f2dffd69["render_dbt_model"]
    n20d45ed0c411592d80346486682f898c["ekos/crates/dbt-gen/src/lib.rs"]
    n20d45ed0c411592d80346486682f898c -->|Contains| n95ef686237ab54fa99de4e67f2dffd69
    n4e0e3bc6d2615d55a9da1bd8f3173cf8["dbt_model_name"]
    n95ef686237ab54fa99de4e67f2dffd69 -->|Calls| n4e0e3bc6d2615d55a9da1bd8f3173cf8
    n130a2e4593d9563fb669b7e6e8bd7900["render_unmapped"]
    n95ef686237ab54fa99de4e67f2dffd69 -->|Calls| n130a2e4593d9563fb669b7e6e8bd7900
    n38a7c3722ec45a38a5fd53f7c4bbf55c["render_join"]
    n95ef686237ab54fa99de4e67f2dffd69 -->|Calls| n38a7c3722ec45a38a5fd53f7c4bbf55c
    nf037d176e9fa5caba531f7995e09727b["render_sink"]
    n95ef686237ab54fa99de4e67f2dffd69 -->|Calls| nf037d176e9fa5caba531f7995e09727b
    nb7519843e48d57b7ac816fdf21a72cdd["render_calculate"]
    n95ef686237ab54fa99de4e67f2dffd69 -->|Calls| nb7519843e48d57b7ac816fdf21a72cdd
    n4976bdefe8ee5e029603fe82d738e9c7["render_filter"]
    n95ef686237ab54fa99de4e67f2dffd69 -->|Calls| n4976bdefe8ee5e029603fe82d738e9c7
    n02d30b9221135cbb8b2627374d4a83c9["render_source"]
    n95ef686237ab54fa99de4e67f2dffd69 -->|Calls| n02d30b9221135cbb8b2627374d4a83c9
    n92ef0b6b150b5f26bfbd07900bf340c8["get_str"]
    n95ef686237ab54fa99de4e67f2dffd69 -->|Calls| n92ef0b6b150b5f26bfbd07900bf340c8
    nf5e3021531665317b596b7e4503b3607["render_aggregate"]
    n95ef686237ab54fa99de4e67f2dffd69 -->|Calls| nf5e3021531665317b596b7e4503b3607
```

## Evidence

_No evidence cited._
