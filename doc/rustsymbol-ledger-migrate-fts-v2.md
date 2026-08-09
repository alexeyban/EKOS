# Ledger::migrate_fts_v2 (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- ← Ledger::open (`1202f2b1-c8ed-5a89-aac3-5ef29891cb8b`)
- → Ledger::index_object_fts_v1 (`e17ebc72-482b-55d0-8cb3-d68e31347276`)
- → Ledger::all_objects (`d640b0e7-cfd1-5693-8c96-022d84598df3`)

### Contains

- ← ekos/crates/ledger/src/lib.rs (`21938f45-b767-5933-9e71-12e15ff53eb1`)

## Diagram

```mermaid
graph TD
    n2c3a50d11ba054fd85092493f809dc4c["Ledger::migrate_fts_v2"]
    n21938f45b76759339e7112e15ff53eb1["ekos/crates/ledger/src/lib.rs"]
    n21938f45b76759339e7112e15ff53eb1 -->|Contains| n2c3a50d11ba054fd85092493f809dc4c
    n1202f2b1c8ed5a89aac35ef29891cb8b["Ledger::open"]
    n1202f2b1c8ed5a89aac35ef29891cb8b -->|Calls| n2c3a50d11ba054fd85092493f809dc4c
    ne17ebc72482b55d08cb3d68e31347276["Ledger::index_object_fts_v1"]
    n2c3a50d11ba054fd85092493f809dc4c -->|Calls| ne17ebc72482b55d08cb3d68e31347276
    nd640b0e7cfd156938c96022d84598df3["Ledger::all_objects"]
    n2c3a50d11ba054fd85092493f809dc4c -->|Calls| nd640b0e7cfd156938c96022d84598df3
```

## Evidence

_No evidence cited._
