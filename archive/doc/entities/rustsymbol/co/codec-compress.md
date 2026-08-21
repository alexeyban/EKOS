# Codec::compress (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → Codec::compress (`599e1672-3536-58b3-8cc1-736e192923ad`)
- ← Ledger::payload_param (`d1e47b92-7f6e-559a-8f56-a2f6b571fd0d`)
- ← migrate_to_v2 (`fee5c44c-a2e1-59db-bf5d-b63aff20f8c9`)

### Contains

- ← ekos/crates/ledger/src/lib.rs (`21938f45-b767-5933-9e71-12e15ff53eb1`)

## Diagram

```mermaid
graph TD
    n599e1672353658b38cc1736e192923ad["Codec::compress"]
    n21938f45b76759339e7112e15ff53eb1["ekos/crates/ledger/src/lib.rs"]
    n21938f45b76759339e7112e15ff53eb1 -->|Contains| n599e1672353658b38cc1736e192923ad
    n599e1672353658b38cc1736e192923ad -->|Calls| n599e1672353658b38cc1736e192923ad
    nd1e47b927f6e559a8f56a2f6b571fd0d["Ledger::payload_param"]
    nd1e47b927f6e559a8f56a2f6b571fd0d -->|Calls| n599e1672353658b38cc1736e192923ad
    nfee5c44ca2e159dbbf5db63aff20f8c9["migrate_to_v2"]
    nfee5c44ca2e159dbbf5db63aff20f8c9 -->|Calls| n599e1672353658b38cc1736e192923ad
```

## Evidence

_No evidence cited._
