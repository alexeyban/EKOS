# dbt_model_name (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- → slugify_snake (`48bceba8-935c-53fe-9561-9485bc8ccca3`)
- ← render_dbt_model (`95ef6862-37ab-54fa-99de-4e67f2dffd69`)

### Contains

- ← ekos/crates/dbt-gen/src/lib.rs (`20d45ed0-c411-592d-8034-6486682f898c`)

## Diagram

```mermaid
graph TD
    n4e0e3bc6d2615d55a9da1bd8f3173cf8["dbt_model_name"]
    n20d45ed0c411592d80346486682f898c["ekos/crates/dbt-gen/src/lib.rs"]
    n20d45ed0c411592d80346486682f898c -->|Contains| n4e0e3bc6d2615d55a9da1bd8f3173cf8
    n48bceba8935c53fe95619485bc8ccca3["slugify_snake"]
    n4e0e3bc6d2615d55a9da1bd8f3173cf8 -->|Calls| n48bceba8935c53fe95619485bc8ccca3
    n95ef686237ab54fa99de4e67f2dffd69["render_dbt_model"]
    n95ef686237ab54fa99de4e67f2dffd69 -->|Calls| n4e0e3bc6d2615d55a9da1bd8f3173cf8
```

## Evidence

_No evidence cited._
