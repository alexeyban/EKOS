# id_value_to_string (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← Ledger::versions_in_window (`972c0223-2c64-54bc-b774-890fc6b61ab1`)
- ← migrate_to_v2 (`fee5c44c-a2e1-59db-bf5d-b63aff20f8c9`)
- ← Ledger::export_versions (`1ed3c4b0-eefc-5cee-8f3b-f559c0e5f97e`)

### Contains

- ← ekos/crates/ledger/src/lib.rs (`21938f45-b767-5933-9e71-12e15ff53eb1`)

## Diagram

```mermaid
graph TD
    na0c3d0ec32945534a1f2b2295cc7d77a["id_value_to_string"]
    n21938f45b76759339e7112e15ff53eb1["ekos/crates/ledger/src/lib.rs"]
    n21938f45b76759339e7112e15ff53eb1 -->|Contains| na0c3d0ec32945534a1f2b2295cc7d77a
    n972c02232c6454bcb774890fc6b61ab1["Ledger::versions_in_window"]
    n972c02232c6454bcb774890fc6b61ab1 -->|Calls| na0c3d0ec32945534a1f2b2295cc7d77a
    nfee5c44ca2e159dbbf5db63aff20f8c9["migrate_to_v2"]
    nfee5c44ca2e159dbbf5db63aff20f8c9 -->|Calls| na0c3d0ec32945534a1f2b2295cc7d77a
    n1ed3c4b0eefc5cee8f3bf559c0e5f97e["Ledger::export_versions"]
    n1ed3c4b0eefc5cee8f3bf559c0e5f97e -->|Calls| na0c3d0ec32945534a1f2b2295cc7d77a
```

## Evidence

_No evidence cited._
