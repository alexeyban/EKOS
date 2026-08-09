# Ledger::versions_in_window (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → id_value_to_string (`a0c3d0ec-3294-5534-a1f2-b2295cc7d77a`)
- ← diff_ledger (`efce5b16-7270-58fe-b278-442b178d7df3`)

### Contains

- ← ekos/crates/ledger/src/lib.rs (`21938f45-b767-5933-9e71-12e15ff53eb1`)

## Diagram

```mermaid
graph TD
    n972c02232c6454bcb774890fc6b61ab1["Ledger::versions_in_window"]
    n21938f45b76759339e7112e15ff53eb1["ekos/crates/ledger/src/lib.rs"]
    n21938f45b76759339e7112e15ff53eb1 -->|Contains| n972c02232c6454bcb774890fc6b61ab1
    na0c3d0ec32945534a1f2b2295cc7d77a["id_value_to_string"]
    n972c02232c6454bcb774890fc6b61ab1 -->|Calls| na0c3d0ec32945534a1f2b2295cc7d77a
    nefce5b16727058feb278442b178d7df3["diff_ledger"]
    nefce5b16727058feb278442b178d7df3 -->|Calls| n972c02232c6454bcb774890fc6b61ab1
```

## Evidence

_No evidence cited._
