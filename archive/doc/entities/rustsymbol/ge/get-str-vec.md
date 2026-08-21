# get_str_vec (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← render_source (`02d30b92-2113-5cbb-8b26-27374d4a83c9`)
- ← render_aggregate (`f5e30215-3166-5317-b596-b7e4503b3607`)

### Contains

- ← ekos/crates/dbt-gen/src/lib.rs (`20d45ed0-c411-592d-8034-6486682f898c`)

## Diagram

```mermaid
graph TD
    n3cb9bf32bfc05c54b647de3f7ed2c4ae["get_str_vec"]
    n20d45ed0c411592d80346486682f898c["ekos/crates/dbt-gen/src/lib.rs"]
    n20d45ed0c411592d80346486682f898c -->|Contains| n3cb9bf32bfc05c54b647de3f7ed2c4ae
    n02d30b9221135cbb8b2627374d4a83c9["render_source"]
    n02d30b9221135cbb8b2627374d4a83c9 -->|Calls| n3cb9bf32bfc05c54b647de3f7ed2c4ae
    nf5e3021531665317b596b7e4503b3607["render_aggregate"]
    nf5e3021531665317b596b7e4503b3607 -->|Calls| n3cb9bf32bfc05c54b647de3f7ed2c4ae
```

## Evidence

_No evidence cited._
