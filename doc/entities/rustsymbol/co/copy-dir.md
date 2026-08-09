# copy_dir (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← FactLedger::vacuum_into (`15d6a903-48f1-5963-9aba-8ee50bcf8c6c`)
- → copy_dir (`3dcea5c3-8c02-5ccc-ac49-8f4903706db4`)

### Contains

- ← ekos/crates/ledger/src/fact_ledger.rs (`eadcb59e-818f-5d1e-af87-ff29aba11423`)

## Diagram

```mermaid
graph TD
    n3dcea5c38c025cccac498f4903706db4["copy_dir"]
    neadcb59e818f5d1eaf87ff29aba11423["ekos/crates/ledger/src/fact_ledger.rs"]
    neadcb59e818f5d1eaf87ff29aba11423 -->|Contains| n3dcea5c38c025cccac498f4903706db4
    n15d6a90348f159639aba8ee50bcf8c6c["FactLedger::vacuum_into"]
    n15d6a90348f159639aba8ee50bcf8c6c -->|Calls| n3dcea5c38c025cccac498f4903706db4
    n3dcea5c38c025cccac498f4903706db4 -->|Calls| n3dcea5c38c025cccac498f4903706db4
```

## Evidence

_No evidence cited._
