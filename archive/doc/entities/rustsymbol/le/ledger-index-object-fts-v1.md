# Ledger::index_object_fts_v1 (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- ← Ledger::migrate_fts_v2 (`2c3a50d1-1ba0-54fd-8509-2493f809dc4c`)
- ← Ledger::append_object (`b71bb7ad-337a-518f-9b6e-316178f45928`)

### Contains

- ← ekos/crates/ledger/src/lib.rs (`21938f45-b767-5933-9e71-12e15ff53eb1`)

## Diagram

```mermaid
graph TD
    ne17ebc72482b55d08cb3d68e31347276["Ledger::index_object_fts_v1"]
    n21938f45b76759339e7112e15ff53eb1["ekos/crates/ledger/src/lib.rs"]
    n21938f45b76759339e7112e15ff53eb1 -->|Contains| ne17ebc72482b55d08cb3d68e31347276
    n2c3a50d11ba054fd85092493f809dc4c["Ledger::migrate_fts_v2"]
    n2c3a50d11ba054fd85092493f809dc4c -->|Calls| ne17ebc72482b55d08cb3d68e31347276
    nb71bb7ad337a518f9b6e316178f45928["Ledger::append_object"]
    nb71bb7ad337a518f9b6e316178f45928 -->|Calls| ne17ebc72482b55d08cb3d68e31347276
```

## Evidence

_No evidence cited._
