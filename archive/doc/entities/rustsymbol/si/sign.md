# sign (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- → signature_base_string (`26cd4cad-2e8c-57aa-8742-f30a3e5eea61`)
- ← authorization_header (`f41ca41a-4961-50d1-925e-01ca53aaa086`)

### Contains

- ← ekos/crates/marketing/src/oauth1.rs (`d9547f96-f031-5a9d-875d-5912db96b474`)

## Diagram

```mermaid
graph TD
    nd047fc97d00759118fb57f43a79a6b03["sign"]
    nd9547f96f0315a9d875d5912db96b474["ekos/crates/marketing/src/oauth1.rs"]
    nd9547f96f0315a9d875d5912db96b474 -->|Contains| nd047fc97d00759118fb57f43a79a6b03
    n26cd4cad2e8c57aa8742f30a3e5eea61["signature_base_string"]
    nd047fc97d00759118fb57f43a79a6b03 -->|Calls| n26cd4cad2e8c57aa8742f30a3e5eea61
    nf41ca41a496150d1925e01ca53aaa086["authorization_header"]
    nf41ca41a496150d1925e01ca53aaa086 -->|Calls| nd047fc97d00759118fb57f43a79a6b03
```

## Evidence

_No evidence cited._
