# fold_state (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← Inner::all_current_payloads (`cdae7ff9-bb1c-5b30-9d98-ae096c1c521f`)
- ← Inner::state_at (`f8ecc412-0c51-5275-8a0d-2f41777af9ac`)

### Contains

- ← ekos/crates/ledger/src/fact_ledger.rs (`eadcb59e-818f-5d1e-af87-ff29aba11423`)

## Diagram

```mermaid
graph TD
    n42e75b5363655fbc83a126fcd87d8f3c["fold_state"]
    neadcb59e818f5d1eaf87ff29aba11423["ekos/crates/ledger/src/fact_ledger.rs"]
    neadcb59e818f5d1eaf87ff29aba11423 -->|Contains| n42e75b5363655fbc83a126fcd87d8f3c
    ncdae7ff9bb1c5b309d98ae096c1c521f["Inner::all_current_payloads"]
    ncdae7ff9bb1c5b309d98ae096c1c521f -->|Calls| n42e75b5363655fbc83a126fcd87d8f3c
    nf8ecc4120c5152758a0d2f41777af9ac["Inner::state_at"]
    nf8ecc4120c5152758a0d2f41777af9ac -->|Calls| n42e75b5363655fbc83a126fcd87d8f3c
```

## Evidence

_No evidence cited._
