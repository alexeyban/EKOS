# FactLedger::append_object (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → FactLedger::append_payload (`59ba0d10-1ac6-5b4b-96f4-c69cac0c6d89`)
- ← FactLedger::merge_from (`f49a099f-a554-5182-9200-3f842f56b4b7`)

### Contains

- ← ekos/crates/ledger/src/fact_ledger.rs (`eadcb59e-818f-5d1e-af87-ff29aba11423`)

## Diagram

```mermaid
graph TD
    n80968d4b8f2c5ecc936842863245986e["FactLedger::append_object"]
    neadcb59e818f5d1eaf87ff29aba11423["ekos/crates/ledger/src/fact_ledger.rs"]
    neadcb59e818f5d1eaf87ff29aba11423 -->|Contains| n80968d4b8f2c5ecc936842863245986e
    n59ba0d101ac65b4b96f4c69cac0c6d89["FactLedger::append_payload"]
    n80968d4b8f2c5ecc936842863245986e -->|Calls| n59ba0d101ac65b4b96f4c69cac0c6d89
    nf49a099fa554518292003f842f56b4b7["FactLedger::merge_from"]
    nf49a099fa554518292003f842f56b4b7 -->|Calls| n80968d4b8f2c5ecc936842863245986e
```

## Evidence

_No evidence cited._
