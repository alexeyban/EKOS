# Inner::flush_memtable (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- ← FactLedger::seal_and_flush (`928856d4-2abc-5ba6-82ba-c6c17887db5e`)
- ← FactLedger::append_inner (`abbc13e0-8705-5d3c-aa78-d21e9a9882ee`)
- → Inner::runs_dir (`d8a4a271-e1a6-5168-9d3e-6965e3c3c9bc`)
- → FactLedger::run_count (`faf5db4a-a16d-59fe-b9e9-4db75e4bce6a`)

### Contains

- ← ekos/crates/ledger/src/fact_ledger.rs (`eadcb59e-818f-5d1e-af87-ff29aba11423`)

## Diagram

```mermaid
graph TD
    nec9d46ec95765ab5a0144e0946f5dc5e["Inner::flush_memtable"]
    neadcb59e818f5d1eaf87ff29aba11423["ekos/crates/ledger/src/fact_ledger.rs"]
    neadcb59e818f5d1eaf87ff29aba11423 -->|Contains| nec9d46ec95765ab5a0144e0946f5dc5e
    n928856d42abc5ba682bac6c17887db5e["FactLedger::seal_and_flush"]
    n928856d42abc5ba682bac6c17887db5e -->|Calls| nec9d46ec95765ab5a0144e0946f5dc5e
    nabbc13e087055d3caa78d21e9a9882ee["FactLedger::append_inner"]
    nabbc13e087055d3caa78d21e9a9882ee -->|Calls| nec9d46ec95765ab5a0144e0946f5dc5e
    nd8a4a271e1a651689d3e6965e3c3c9bc["Inner::runs_dir"]
    nec9d46ec95765ab5a0144e0946f5dc5e -->|Calls| nd8a4a271e1a651689d3e6965e3c3c9bc
    nfaf5db4aa16d59feb9e94db75e4bce6a["FactLedger::run_count"]
    nec9d46ec95765ab5a0144e0946f5dc5e -->|Calls| nfaf5db4aa16d59feb9e94db75e4bce6a
```

## Evidence

_No evidence cited._
