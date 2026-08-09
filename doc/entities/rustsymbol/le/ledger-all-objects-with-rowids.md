# Ledger::all_objects_with_rowids (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- ← migrate_to_v2 (`fee5c44c-a2e1-59db-bf5d-b63aff20f8c9`)
- → Ledger::payload_to_string (`b30e2764-552e-5d3e-a1e5-34c523dd7475`)

### Contains

- ← ekos/crates/ledger/src/lib.rs (`21938f45-b767-5933-9e71-12e15ff53eb1`)

## Diagram

```mermaid
graph TD
    nf2714bfaa29a5e5cb6ce96c95bd2a1af["Ledger::all_objects_with_rowids"]
    n21938f45b76759339e7112e15ff53eb1["ekos/crates/ledger/src/lib.rs"]
    n21938f45b76759339e7112e15ff53eb1 -->|Contains| nf2714bfaa29a5e5cb6ce96c95bd2a1af
    nfee5c44ca2e159dbbf5db63aff20f8c9["migrate_to_v2"]
    nfee5c44ca2e159dbbf5db63aff20f8c9 -->|Calls| nf2714bfaa29a5e5cb6ce96c95bd2a1af
    nb30e2764552e5d3ea1e534c523dd7475["Ledger::payload_to_string"]
    nf2714bfaa29a5e5cb6ce96c95bd2a1af -->|Calls| nb30e2764552e5d3ea1e534c523dd7475
```

## Evidence

_No evidence cited._
