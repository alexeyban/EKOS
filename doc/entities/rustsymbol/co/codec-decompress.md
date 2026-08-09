# Codec::decompress (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- ← Ledger::payload_to_string (`b30e2764-552e-5d3e-a1e5-34c523dd7475`)
- ← migrate_to_v2 (`fee5c44c-a2e1-59db-bf5d-b63aff20f8c9`)

### Contains

- ← ekos/crates/ledger/src/lib.rs (`21938f45-b767-5933-9e71-12e15ff53eb1`)

## Diagram

```mermaid
graph TD
    n3ee6589ff3ba5e67b7b68950c0575ae5["Codec::decompress"]
    n21938f45b76759339e7112e15ff53eb1["ekos/crates/ledger/src/lib.rs"]
    n21938f45b76759339e7112e15ff53eb1 -->|Contains| n3ee6589ff3ba5e67b7b68950c0575ae5
    nb30e2764552e5d3ea1e534c523dd7475["Ledger::payload_to_string"]
    nb30e2764552e5d3ea1e534c523dd7475 -->|Calls| n3ee6589ff3ba5e67b7b68950c0575ae5
    nfee5c44ca2e159dbbf5db63aff20f8c9["migrate_to_v2"]
    nfee5c44ca2e159dbbf5db63aff20f8c9 -->|Calls| n3ee6589ff3ba5e67b7b68950c0575ae5
```

## Evidence

_No evidence cited._
