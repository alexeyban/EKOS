# authorization_header (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- → sign (`d047fc97-d007-5911-8fb5-7f43a79a6b03`)
- → generate_nonce (`c15fbd69-b60d-5004-b305-6a836bf18ebd`)
- → unix_timestamp (`c3d5f345-a134-51fe-84fa-e2d08acd4eaa`)

### Contains

- ← ekos/crates/marketing/src/oauth1.rs (`d9547f96-f031-5a9d-875d-5912db96b474`)

## Diagram

```mermaid
graph TD
    nf41ca41a496150d1925e01ca53aaa086["authorization_header"]
    nd9547f96f0315a9d875d5912db96b474["ekos/crates/marketing/src/oauth1.rs"]
    nd9547f96f0315a9d875d5912db96b474 -->|Contains| nf41ca41a496150d1925e01ca53aaa086
    nd047fc97d00759118fb57f43a79a6b03["sign"]
    nf41ca41a496150d1925e01ca53aaa086 -->|Calls| nd047fc97d00759118fb57f43a79a6b03
    nc15fbd69b60d5004b3056a836bf18ebd["generate_nonce"]
    nf41ca41a496150d1925e01ca53aaa086 -->|Calls| nc15fbd69b60d5004b3056a836bf18ebd
    nc3d5f345a13451fe84fae2d08acd4eaa["unix_timestamp"]
    nf41ca41a496150d1925e01ca53aaa086 -->|Calls| nc3d5f345a13451fe84fae2d08acd4eaa
```

## Evidence

_No evidence cited._
