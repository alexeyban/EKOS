# Inner::index_object (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- ← FactLedger::open_with_seal_threshold (`50a7d9c4-7eb2-5d0c-9c80-5e2982e59574`)
- ← FactLedger::append_inner (`abbc13e0-8705-5d3c-aa78-d21e9a9882ee`)

### Contains

- ← ekos/crates/ledger/src/fact_ledger.rs (`eadcb59e-818f-5d1e-af87-ff29aba11423`)

## Diagram

```mermaid
graph TD
    nc32e3f6e7f6e585fae03f82f97de91ed["Inner::index_object"]
    neadcb59e818f5d1eaf87ff29aba11423["ekos/crates/ledger/src/fact_ledger.rs"]
    neadcb59e818f5d1eaf87ff29aba11423 -->|Contains| nc32e3f6e7f6e585fae03f82f97de91ed
    n50a7d9c47eb25d0c9c805e2982e59574["FactLedger::open_with_seal_threshold"]
    n50a7d9c47eb25d0c9c805e2982e59574 -->|Calls| nc32e3f6e7f6e585fae03f82f97de91ed
    nabbc13e087055d3caa78d21e9a9882ee["FactLedger::append_inner"]
    nabbc13e087055d3caa78d21e9a9882ee -->|Calls| nc32e3f6e7f6e585fae03f82f97de91ed
```

## Evidence

_No evidence cited._
