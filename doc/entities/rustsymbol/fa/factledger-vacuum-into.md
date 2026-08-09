# FactLedger::vacuum_into (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → FactLedger::open (`0b6a6624-b311-5cb0-84ca-cc0ba0b33ed1`)
- → copy_dir (`3dcea5c3-8c02-5ccc-ac49-8f4903706db4`)

### Contains

- ← ekos/crates/ledger/src/fact_ledger.rs (`eadcb59e-818f-5d1e-af87-ff29aba11423`)

## Diagram

```mermaid
graph TD
    n15d6a90348f159639aba8ee50bcf8c6c["FactLedger::vacuum_into"]
    neadcb59e818f5d1eaf87ff29aba11423["ekos/crates/ledger/src/fact_ledger.rs"]
    neadcb59e818f5d1eaf87ff29aba11423 -->|Contains| n15d6a90348f159639aba8ee50bcf8c6c
    n0b6a6624b3115cb084cacc0ba0b33ed1["FactLedger::open"]
    n15d6a90348f159639aba8ee50bcf8c6c -->|Calls| n0b6a6624b3115cb084cacc0ba0b33ed1
    n3dcea5c38c025cccac498f4903706db4["copy_dir"]
    n15d6a90348f159639aba8ee50bcf8c6c -->|Calls| n3dcea5c38c025cccac498f4903706db4
```

## Evidence

_No evidence cited._
