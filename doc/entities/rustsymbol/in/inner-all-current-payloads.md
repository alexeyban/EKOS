# Inner::all_current_payloads (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- ← FactLedger::all_of_kind (`96be8663-06e0-5092-9271-602b15b98872`)
- → fold_state (`42e75b53-6365-5fbc-83a1-26fcd87d8f3c`)

### Contains

- ← ekos/crates/ledger/src/fact_ledger.rs (`eadcb59e-818f-5d1e-af87-ff29aba11423`)

## Diagram

```mermaid
graph TD
    ncdae7ff9bb1c5b309d98ae096c1c521f["Inner::all_current_payloads"]
    neadcb59e818f5d1eaf87ff29aba11423["ekos/crates/ledger/src/fact_ledger.rs"]
    neadcb59e818f5d1eaf87ff29aba11423 -->|Contains| ncdae7ff9bb1c5b309d98ae096c1c521f
    n96be866306e050929271602b15b98872["FactLedger::all_of_kind"]
    n96be866306e050929271602b15b98872 -->|Calls| ncdae7ff9bb1c5b309d98ae096c1c521f
    n42e75b5363655fbc83a126fcd87d8f3c["fold_state"]
    ncdae7ff9bb1c5b309d98ae096c1c521f -->|Calls| n42e75b5363655fbc83a126fcd87d8f3c
```

## Evidence

_No evidence cited._
