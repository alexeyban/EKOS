# FactLedger::run_count (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → FactLedger::run_count (`faf5db4a-a16d-59fe-b9e9-4db75e4bce6a`)
- ← Inner::flush_memtable (`ec9d46ec-9576-5ab5-a014-4e0946f5dc5e`)

### Contains

- ← ekos/crates/ledger/src/fact_ledger.rs (`eadcb59e-818f-5d1e-af87-ff29aba11423`)

## Diagram

```mermaid
graph TD
    nfaf5db4aa16d59feb9e94db75e4bce6a["FactLedger::run_count"]
    neadcb59e818f5d1eaf87ff29aba11423["ekos/crates/ledger/src/fact_ledger.rs"]
    neadcb59e818f5d1eaf87ff29aba11423 -->|Contains| nfaf5db4aa16d59feb9e94db75e4bce6a
    nfaf5db4aa16d59feb9e94db75e4bce6a -->|Calls| nfaf5db4aa16d59feb9e94db75e4bce6a
    nec9d46ec95765ab5a0144e0946f5dc5e["Inner::flush_memtable"]
    nec9d46ec95765ab5a0144e0946f5dc5e -->|Calls| nfaf5db4aa16d59feb9e94db75e4bce6a
```

## Evidence

_No evidence cited._
