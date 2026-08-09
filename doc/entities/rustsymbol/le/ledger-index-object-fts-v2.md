# Ledger::index_object_fts_v2 (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- ← Ledger::append_object (`b71bb7ad-337a-518f-9b6e-316178f45928`)
- ← migrate_to_v2 (`fee5c44c-a2e1-59db-bf5d-b63aff20f8c9`)

### Contains

- ← ekos/crates/ledger/src/lib.rs (`21938f45-b767-5933-9e71-12e15ff53eb1`)

## Diagram

```mermaid
graph TD
    n7635661678015ca19003e69db3599198["Ledger::index_object_fts_v2"]
    n21938f45b76759339e7112e15ff53eb1["ekos/crates/ledger/src/lib.rs"]
    n21938f45b76759339e7112e15ff53eb1 -->|Contains| n7635661678015ca19003e69db3599198
    nb71bb7ad337a518f9b6e316178f45928["Ledger::append_object"]
    nb71bb7ad337a518f9b6e316178f45928 -->|Calls| n7635661678015ca19003e69db3599198
    nfee5c44ca2e159dbbf5db63aff20f8c9["migrate_to_v2"]
    nfee5c44ca2e159dbbf5db63aff20f8c9 -->|Calls| n7635661678015ca19003e69db3599198
```

## Evidence

_No evidence cited._
