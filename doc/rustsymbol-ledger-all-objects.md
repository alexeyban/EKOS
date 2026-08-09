# Ledger::all_objects (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- ← Ledger::migrate_fts_v2 (`2c3a50d1-1ba0-54fd-8509-2493f809dc4c`)
- → Ledger::query_payloads (`b8401b6d-6d8d-5633-9b6a-27c093ab2db6`)
- ← merge_branch (`16be84c8-16f2-5d63-8dff-104f7296fc29`)
- ← merge_stores (`35e9663b-3b6d-50ec-ad16-9721c45eb3d1`)

### Contains

- ← ekos/crates/ledger/src/lib.rs (`21938f45-b767-5933-9e71-12e15ff53eb1`)

## Diagram

```mermaid
graph TD
    nd640b0e7cfd156938c96022d84598df3["Ledger::all_objects"]
    n21938f45b76759339e7112e15ff53eb1["ekos/crates/ledger/src/lib.rs"]
    n21938f45b76759339e7112e15ff53eb1 -->|Contains| nd640b0e7cfd156938c96022d84598df3
    n2c3a50d11ba054fd85092493f809dc4c["Ledger::migrate_fts_v2"]
    n2c3a50d11ba054fd85092493f809dc4c -->|Calls| nd640b0e7cfd156938c96022d84598df3
    nb8401b6d6d8d56339b6a27c093ab2db6["Ledger::query_payloads"]
    nd640b0e7cfd156938c96022d84598df3 -->|Calls| nb8401b6d6d8d56339b6a27c093ab2db6
    n16be84c816f25d638dff104f7296fc29["merge_branch"]
    n16be84c816f25d638dff104f7296fc29 -->|Calls| nd640b0e7cfd156938c96022d84598df3
    n35e9663b3b6d50ecad169721c45eb3d1["merge_stores"]
    n35e9663b3b6d50ecad169721c45eb3d1 -->|Calls| nd640b0e7cfd156938c96022d84598df3
```

## Evidence

_No evidence cited._
