# render_join (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← render_dbt_model (`95ef6862-37ab-54fa-99de-4e67f2dffd69`)
- → get_str (`92ef0b6b-150b-5f26-bfbd-07900bf340c8`)
- → get_pairs (`0d9817e0-e1a6-593e-8889-f7658e34fcfc`)
- → no_upstream_placeholder (`0a8b417e-5dc9-5199-9efe-5d7b8d2b02b5`)

### Contains

- ← ekos/crates/dbt-gen/src/lib.rs (`20d45ed0-c411-592d-8034-6486682f898c`)

## Diagram

```mermaid
graph TD
    n38a7c3722ec45a38a5fd53f7c4bbf55c["render_join"]
    n20d45ed0c411592d80346486682f898c["ekos/crates/dbt-gen/src/lib.rs"]
    n20d45ed0c411592d80346486682f898c -->|Contains| n38a7c3722ec45a38a5fd53f7c4bbf55c
    n95ef686237ab54fa99de4e67f2dffd69["render_dbt_model"]
    n95ef686237ab54fa99de4e67f2dffd69 -->|Calls| n38a7c3722ec45a38a5fd53f7c4bbf55c
    n92ef0b6b150b5f26bfbd07900bf340c8["get_str"]
    n38a7c3722ec45a38a5fd53f7c4bbf55c -->|Calls| n92ef0b6b150b5f26bfbd07900bf340c8
    n0d9817e0e1a6593e8889f7658e34fcfc["get_pairs"]
    n38a7c3722ec45a38a5fd53f7c4bbf55c -->|Calls| n0d9817e0e1a6593e8889f7658e34fcfc
    n0a8b417e5dc951999efe5d7b8d2b02b5["no_upstream_placeholder"]
    n38a7c3722ec45a38a5fd53f7c4bbf55c -->|Calls| n0a8b417e5dc951999efe5d7b8d2b02b5
```

## Evidence

_No evidence cited._
