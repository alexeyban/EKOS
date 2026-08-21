# FactLedger::merge_from (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → FactLedger::get_relationship (`c6fb9c16-6ed5-5cea-98ef-75a6ed96b979`)
- → FactLedger::append_relationship (`13eab4a6-a185-5803-a090-8def742c8045`)
- → FactLedger::all_objects (`10d9bb5d-983b-571b-80a4-980eda139690`)
- → FactLedger::all_relationships (`09e04918-5498-5c94-8d69-c84e67fef17d`)
- → FactLedger::append_object (`80968d4b-8f2c-5ecc-9368-42863245986e`)
- → FactLedger::get_object (`7c9ba14e-5785-5789-88ed-e471ea3451de`)

### Contains

- ← ekos/crates/ledger/src/fact_ledger.rs (`eadcb59e-818f-5d1e-af87-ff29aba11423`)

## Diagram

```mermaid
graph TD
    nf49a099fa554518292003f842f56b4b7["FactLedger::merge_from"]
    neadcb59e818f5d1eaf87ff29aba11423["ekos/crates/ledger/src/fact_ledger.rs"]
    neadcb59e818f5d1eaf87ff29aba11423 -->|Contains| nf49a099fa554518292003f842f56b4b7
    nc6fb9c166ed55cea98ef75a6ed96b979["FactLedger::get_relationship"]
    nf49a099fa554518292003f842f56b4b7 -->|Calls| nc6fb9c166ed55cea98ef75a6ed96b979
    n13eab4a6a1855803a0908def742c8045["FactLedger::append_relationship"]
    nf49a099fa554518292003f842f56b4b7 -->|Calls| n13eab4a6a1855803a0908def742c8045
    n10d9bb5d983b571b80a4980eda139690["FactLedger::all_objects"]
    nf49a099fa554518292003f842f56b4b7 -->|Calls| n10d9bb5d983b571b80a4980eda139690
    n09e0491854985c948d69c84e67fef17d["FactLedger::all_relationships"]
    nf49a099fa554518292003f842f56b4b7 -->|Calls| n09e0491854985c948d69c84e67fef17d
    n80968d4b8f2c5ecc936842863245986e["FactLedger::append_object"]
    nf49a099fa554518292003f842f56b4b7 -->|Calls| n80968d4b8f2c5ecc936842863245986e
    n7c9ba14e5785578988ede471ea3451de["FactLedger::get_object"]
    nf49a099fa554518292003f842f56b4b7 -->|Calls| n7c9ba14e5785578988ede471ea3451de
```

## Evidence

_No evidence cited._
