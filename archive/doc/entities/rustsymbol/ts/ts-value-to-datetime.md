# ts_value_to_datetime (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← migrate_to_v2 (`fee5c44c-a2e1-59db-bf5d-b63aff20f8c9`)
- ← Ledger::export_versions (`1ed3c4b0-eefc-5cee-8f3b-f559c0e5f97e`)

### Contains

- ← ekos/crates/ledger/src/lib.rs (`21938f45-b767-5933-9e71-12e15ff53eb1`)

## Diagram

```mermaid
graph TD
    n34ebc45de42657889f8bc605bf91a6a3["ts_value_to_datetime"]
    n21938f45b76759339e7112e15ff53eb1["ekos/crates/ledger/src/lib.rs"]
    n21938f45b76759339e7112e15ff53eb1 -->|Contains| n34ebc45de42657889f8bc605bf91a6a3
    nfee5c44ca2e159dbbf5db63aff20f8c9["migrate_to_v2"]
    nfee5c44ca2e159dbbf5db63aff20f8c9 -->|Calls| n34ebc45de42657889f8bc605bf91a6a3
    n1ed3c4b0eefc5cee8f3bf559c0e5f97e["Ledger::export_versions"]
    n1ed3c4b0eefc5cee8f3bf559c0e5f97e -->|Calls| n34ebc45de42657889f8bc605bf91a6a3
```

## Evidence

_No evidence cited._
