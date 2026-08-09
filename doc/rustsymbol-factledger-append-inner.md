# FactLedger::append_inner (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- ← FactLedger::append_payload (`59ba0d10-1ac6-5b4b-96f4-c69cac0c6d89`)
- ← FactLedger::append_version (`fa5c07dd-0d4b-5578-b912-83e1377ef34c`)
- → Inner::index_object (`c32e3f6e-7f6e-585f-ae03-f82f97de91ed`)
- → kind_of_payload (`abd8ce5a-e663-50ce-a42f-e2c4a13c43fb`)
- → Inner::state_at (`f8ecc412-0c51-5275-8a0d-2f41777af9ac`)
- → Inner::current_sig (`ba605f6f-3be3-5689-94ce-047beed35236`)
- → Inner::flush_memtable (`ec9d46ec-9576-5ab5-a014-4e0946f5dc5e`)

### Contains

- ← ekos/crates/ledger/src/fact_ledger.rs (`eadcb59e-818f-5d1e-af87-ff29aba11423`)

## Diagram

```mermaid
graph TD
    nabbc13e087055d3caa78d21e9a9882ee["FactLedger::append_inner"]
    neadcb59e818f5d1eaf87ff29aba11423["ekos/crates/ledger/src/fact_ledger.rs"]
    neadcb59e818f5d1eaf87ff29aba11423 -->|Contains| nabbc13e087055d3caa78d21e9a9882ee
    n59ba0d101ac65b4b96f4c69cac0c6d89["FactLedger::append_payload"]
    n59ba0d101ac65b4b96f4c69cac0c6d89 -->|Calls| nabbc13e087055d3caa78d21e9a9882ee
    nfa5c07dd0d4b5578b91283e1377ef34c["FactLedger::append_version"]
    nfa5c07dd0d4b5578b91283e1377ef34c -->|Calls| nabbc13e087055d3caa78d21e9a9882ee
    nc32e3f6e7f6e585fae03f82f97de91ed["Inner::index_object"]
    nabbc13e087055d3caa78d21e9a9882ee -->|Calls| nc32e3f6e7f6e585fae03f82f97de91ed
    nabd8ce5ae66350cea42fe2c4a13c43fb["kind_of_payload"]
    nabbc13e087055d3caa78d21e9a9882ee -->|Calls| nabd8ce5ae66350cea42fe2c4a13c43fb
    nf8ecc4120c5152758a0d2f41777af9ac["Inner::state_at"]
    nabbc13e087055d3caa78d21e9a9882ee -->|Calls| nf8ecc4120c5152758a0d2f41777af9ac
    nba605f6f3be3568994ce047beed35236["Inner::current_sig"]
    nabbc13e087055d3caa78d21e9a9882ee -->|Calls| nba605f6f3be3568994ce047beed35236
    nec9d46ec95765ab5a0144e0946f5dc5e["Inner::flush_memtable"]
    nabbc13e087055d3caa78d21e9a9882ee -->|Calls| nec9d46ec95765ab5a0144e0946f5dc5e
```

## Evidence

_No evidence cited._
