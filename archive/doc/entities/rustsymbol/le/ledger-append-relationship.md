# Ledger::append_relationship (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → Ledger::append_versioned (`fd02b8da-192d-585b-a46d-996b4095186c`)
- ← merge_branch (`16be84c8-16f2-5d63-8dff-104f7296fc29`)
- ← merge_stores (`35e9663b-3b6d-50ec-ad16-9721c45eb3d1`)

### Contains

- ← ekos/crates/ledger/src/lib.rs (`21938f45-b767-5933-9e71-12e15ff53eb1`)

## Diagram

```mermaid
graph TD
    n7cfea3496f7b55018b201291291d672b["Ledger::append_relationship"]
    n21938f45b76759339e7112e15ff53eb1["ekos/crates/ledger/src/lib.rs"]
    n21938f45b76759339e7112e15ff53eb1 -->|Contains| n7cfea3496f7b55018b201291291d672b
    nfd02b8da192d585ba46d996b4095186c["Ledger::append_versioned"]
    n7cfea3496f7b55018b201291291d672b -->|Calls| nfd02b8da192d585ba46d996b4095186c
    n16be84c816f25d638dff104f7296fc29["merge_branch"]
    n16be84c816f25d638dff104f7296fc29 -->|Calls| n7cfea3496f7b55018b201291291d672b
    n35e9663b3b6d50ecad169721c45eb3d1["merge_stores"]
    n35e9663b3b6d50ecad169721c45eb3d1 -->|Calls| n7cfea3496f7b55018b201291291d672b
```

## Evidence

_No evidence cited._
