# Ledger::append_object (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → Ledger::append_versioned (`fd02b8da-192d-585b-a46d-996b4095186c`)
- → Ledger::index_object_fts_v2 (`76356616-7801-5ca1-9003-e69db3599198`)
- → Ledger::index_object_fts_v1 (`e17ebc72-482b-55d0-8cb3-d68e31347276`)
- ← merge_branch (`16be84c8-16f2-5d63-8dff-104f7296fc29`)
- ← merge_stores (`35e9663b-3b6d-50ec-ad16-9721c45eb3d1`)

### Contains

- ← ekos/crates/ledger/src/lib.rs (`21938f45-b767-5933-9e71-12e15ff53eb1`)

## Diagram

```mermaid
graph TD
    nb71bb7ad337a518f9b6e316178f45928["Ledger::append_object"]
    n21938f45b76759339e7112e15ff53eb1["ekos/crates/ledger/src/lib.rs"]
    n21938f45b76759339e7112e15ff53eb1 -->|Contains| nb71bb7ad337a518f9b6e316178f45928
    nfd02b8da192d585ba46d996b4095186c["Ledger::append_versioned"]
    nb71bb7ad337a518f9b6e316178f45928 -->|Calls| nfd02b8da192d585ba46d996b4095186c
    n7635661678015ca19003e69db3599198["Ledger::index_object_fts_v2"]
    nb71bb7ad337a518f9b6e316178f45928 -->|Calls| n7635661678015ca19003e69db3599198
    ne17ebc72482b55d08cb3d68e31347276["Ledger::index_object_fts_v1"]
    nb71bb7ad337a518f9b6e316178f45928 -->|Calls| ne17ebc72482b55d08cb3d68e31347276
    n16be84c816f25d638dff104f7296fc29["merge_branch"]
    n16be84c816f25d638dff104f7296fc29 -->|Calls| nb71bb7ad337a518f9b6e316178f45928
    n35e9663b3b6d50ecad169721c45eb3d1["merge_stores"]
    n35e9663b3b6d50ecad169721c45eb3d1 -->|Calls| nb71bb7ad337a518f9b6e316178f45928
```

## Evidence

_No evidence cited._
