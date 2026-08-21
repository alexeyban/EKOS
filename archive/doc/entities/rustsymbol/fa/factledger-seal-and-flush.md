# FactLedger::seal_and_flush (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → Inner::flush_memtable (`ec9d46ec-9576-5ab5-a014-4e0946f5dc5e`)

### Contains

- ← ekos/crates/ledger/src/fact_ledger.rs (`eadcb59e-818f-5d1e-af87-ff29aba11423`)

## Diagram

```mermaid
graph TD
    n928856d42abc5ba682bac6c17887db5e["FactLedger::seal_and_flush"]
    neadcb59e818f5d1eaf87ff29aba11423["ekos/crates/ledger/src/fact_ledger.rs"]
    neadcb59e818f5d1eaf87ff29aba11423 -->|Contains| n928856d42abc5ba682bac6c17887db5e
    nec9d46ec95765ab5a0144e0946f5dc5e["Inner::flush_memtable"]
    n928856d42abc5ba682bac6c17887db5e -->|Calls| nec9d46ec95765ab5a0144e0946f5dc5e
```

## Evidence

_No evidence cited._
