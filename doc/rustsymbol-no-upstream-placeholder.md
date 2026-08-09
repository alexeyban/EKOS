# no_upstream_placeholder (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← render_sink (`f037d176-e9fa-5cab-a531-f7995e09727b`)
- ← render_join (`38a7c372-2ec4-5a38-a5fd-53f7c4bbf55c`)
- ← render_aggregate (`f5e30215-3166-5317-b596-b7e4503b3607`)
- ← render_filter (`4976bdef-e8ee-5e02-9603-fe82d738e9c7`)
- ← render_calculate (`b7519843-e48d-57b7-ac81-6fdf21a72cdd`)

### Contains

- ← ekos/crates/dbt-gen/src/lib.rs (`20d45ed0-c411-592d-8034-6486682f898c`)

## Diagram

```mermaid
graph TD
    n0a8b417e5dc951999efe5d7b8d2b02b5["no_upstream_placeholder"]
    n20d45ed0c411592d80346486682f898c["ekos/crates/dbt-gen/src/lib.rs"]
    n20d45ed0c411592d80346486682f898c -->|Contains| n0a8b417e5dc951999efe5d7b8d2b02b5
    nf037d176e9fa5caba531f7995e09727b["render_sink"]
    nf037d176e9fa5caba531f7995e09727b -->|Calls| n0a8b417e5dc951999efe5d7b8d2b02b5
    n38a7c3722ec45a38a5fd53f7c4bbf55c["render_join"]
    n38a7c3722ec45a38a5fd53f7c4bbf55c -->|Calls| n0a8b417e5dc951999efe5d7b8d2b02b5
    nf5e3021531665317b596b7e4503b3607["render_aggregate"]
    nf5e3021531665317b596b7e4503b3607 -->|Calls| n0a8b417e5dc951999efe5d7b8d2b02b5
    n4976bdefe8ee5e029603fe82d738e9c7["render_filter"]
    n4976bdefe8ee5e029603fe82d738e9c7 -->|Calls| n0a8b417e5dc951999efe5d7b8d2b02b5
    nb7519843e48d57b7ac816fdf21a72cdd["render_calculate"]
    nb7519843e48d57b7ac816fdf21a72cdd -->|Calls| n0a8b417e5dc951999efe5d7b8d2b02b5
```

## Evidence

_No evidence cited._
