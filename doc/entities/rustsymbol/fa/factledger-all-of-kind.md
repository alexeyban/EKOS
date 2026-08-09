# FactLedger::all_of_kind (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- ← FactLedger::all_objects (`10d9bb5d-983b-571b-80a4-980eda139690`)
- ← FactLedger::all_relationships (`09e04918-5498-5c94-8d69-c84e67fef17d`)
- → kind_of_payload (`abd8ce5a-e663-50ce-a42f-e2c4a13c43fb`)
- → Inner::all_current_payloads (`cdae7ff9-bb1c-5b30-9d98-ae096c1c521f`)

### Contains

- ← ekos/crates/ledger/src/fact_ledger.rs (`eadcb59e-818f-5d1e-af87-ff29aba11423`)

## Diagram

```mermaid
graph TD
    n96be866306e050929271602b15b98872["FactLedger::all_of_kind"]
    neadcb59e818f5d1eaf87ff29aba11423["ekos/crates/ledger/src/fact_ledger.rs"]
    neadcb59e818f5d1eaf87ff29aba11423 -->|Contains| n96be866306e050929271602b15b98872
    n10d9bb5d983b571b80a4980eda139690["FactLedger::all_objects"]
    n10d9bb5d983b571b80a4980eda139690 -->|Calls| n96be866306e050929271602b15b98872
    n09e0491854985c948d69c84e67fef17d["FactLedger::all_relationships"]
    n09e0491854985c948d69c84e67fef17d -->|Calls| n96be866306e050929271602b15b98872
    nabd8ce5ae66350cea42fe2c4a13c43fb["kind_of_payload"]
    n96be866306e050929271602b15b98872 -->|Calls| nabd8ce5ae66350cea42fe2c4a13c43fb
    ncdae7ff9bb1c5b309d98ae096c1c521f["Inner::all_current_payloads"]
    n96be866306e050929271602b15b98872 -->|Calls| ncdae7ff9bb1c5b309d98ae096c1c521f
```

## Evidence

_No evidence cited._
