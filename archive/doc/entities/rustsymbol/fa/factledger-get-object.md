# FactLedger::get_object (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → FactLedger::typed_current (`70c6fe5b-8567-5988-b867-f2d5db8b76a8`)
- ← FactLedger::merge_from (`f49a099f-a554-5182-9200-3f842f56b4b7`)

### Contains

- ← ekos/crates/ledger/src/fact_ledger.rs (`eadcb59e-818f-5d1e-af87-ff29aba11423`)

## Diagram

```mermaid
graph TD
    n7c9ba14e5785578988ede471ea3451de["FactLedger::get_object"]
    neadcb59e818f5d1eaf87ff29aba11423["ekos/crates/ledger/src/fact_ledger.rs"]
    neadcb59e818f5d1eaf87ff29aba11423 -->|Contains| n7c9ba14e5785578988ede471ea3451de
    n70c6fe5b85675988b867f2d5db8b76a8["FactLedger::typed_current"]
    n7c9ba14e5785578988ede471ea3451de -->|Calls| n70c6fe5b85675988b867f2d5db8b76a8
    nf49a099fa554518292003f842f56b4b7["FactLedger::merge_from"]
    nf49a099fa554518292003f842f56b4b7 -->|Calls| n7c9ba14e5785578988ede471ea3451de
```

## Evidence

_No evidence cited._
