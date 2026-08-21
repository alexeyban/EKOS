# FactLedger::open (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → FactLedger::open_with_seal_threshold (`50a7d9c4-7eb2-5d0c-9c80-5e2982e59574`)
- ← FactLedger::vacuum_into (`15d6a903-48f1-5963-9aba-8ee50bcf8c6c`)

### Contains

- ← ekos/crates/ledger/src/fact_ledger.rs (`eadcb59e-818f-5d1e-af87-ff29aba11423`)

## Diagram

```mermaid
graph TD
    n0b6a6624b3115cb084cacc0ba0b33ed1["FactLedger::open"]
    neadcb59e818f5d1eaf87ff29aba11423["ekos/crates/ledger/src/fact_ledger.rs"]
    neadcb59e818f5d1eaf87ff29aba11423 -->|Contains| n0b6a6624b3115cb084cacc0ba0b33ed1
    n50a7d9c47eb25d0c9c805e2982e59574["FactLedger::open_with_seal_threshold"]
    n0b6a6624b3115cb084cacc0ba0b33ed1 -->|Calls| n50a7d9c47eb25d0c9c805e2982e59574
    n15d6a90348f159639aba8ee50bcf8c6c["FactLedger::vacuum_into"]
    n15d6a90348f159639aba8ee50bcf8c6c -->|Calls| n0b6a6624b3115cb084cacc0ba0b33ed1
```

## Evidence

_No evidence cited._
