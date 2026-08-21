# render_sink (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← render_dbt_model (`95ef6862-37ab-54fa-99de-4e67f2dffd69`)
- → no_upstream_placeholder (`0a8b417e-5dc9-5199-9efe-5d7b8d2b02b5`)

### Contains

- ← ekos/crates/dbt-gen/src/lib.rs (`20d45ed0-c411-592d-8034-6486682f898c`)

## Diagram

```mermaid
graph TD
    nf037d176e9fa5caba531f7995e09727b["render_sink"]
    n20d45ed0c411592d80346486682f898c["ekos/crates/dbt-gen/src/lib.rs"]
    n20d45ed0c411592d80346486682f898c -->|Contains| nf037d176e9fa5caba531f7995e09727b
    n95ef686237ab54fa99de4e67f2dffd69["render_dbt_model"]
    n95ef686237ab54fa99de4e67f2dffd69 -->|Calls| nf037d176e9fa5caba531f7995e09727b
    n0a8b417e5dc951999efe5d7b8d2b02b5["no_upstream_placeholder"]
    nf037d176e9fa5caba531f7995e09727b -->|Calls| n0a8b417e5dc951999efe5d7b8d2b02b5
```

## Evidence

_No evidence cited._
