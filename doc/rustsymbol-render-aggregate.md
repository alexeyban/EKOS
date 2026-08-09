# render_aggregate (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← render_dbt_model (`95ef6862-37ab-54fa-99de-4e67f2dffd69`)
- → get_aggs (`e956ee45-8ad2-51b9-b513-3fba564be113`)
- → no_upstream_placeholder (`0a8b417e-5dc9-5199-9efe-5d7b8d2b02b5`)
- → get_str_vec (`3cb9bf32-bfc0-5c54-b647-de3f7ed2c4ae`)

### Contains

- ← ekos/crates/dbt-gen/src/lib.rs (`20d45ed0-c411-592d-8034-6486682f898c`)

## Diagram

```mermaid
graph TD
    nf5e3021531665317b596b7e4503b3607["render_aggregate"]
    n20d45ed0c411592d80346486682f898c["ekos/crates/dbt-gen/src/lib.rs"]
    n20d45ed0c411592d80346486682f898c -->|Contains| nf5e3021531665317b596b7e4503b3607
    n95ef686237ab54fa99de4e67f2dffd69["render_dbt_model"]
    n95ef686237ab54fa99de4e67f2dffd69 -->|Calls| nf5e3021531665317b596b7e4503b3607
    ne956ee458ad251b9b5133fba564be113["get_aggs"]
    nf5e3021531665317b596b7e4503b3607 -->|Calls| ne956ee458ad251b9b5133fba564be113
    n0a8b417e5dc951999efe5d7b8d2b02b5["no_upstream_placeholder"]
    nf5e3021531665317b596b7e4503b3607 -->|Calls| n0a8b417e5dc951999efe5d7b8d2b02b5
    n3cb9bf32bfc05c54b647de3f7ed2c4ae["get_str_vec"]
    nf5e3021531665317b596b7e4503b3607 -->|Calls| n3cb9bf32bfc05c54b647de3f7ed2c4ae
```

## Evidence

_No evidence cited._
