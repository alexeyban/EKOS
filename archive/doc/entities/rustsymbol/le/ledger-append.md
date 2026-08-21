# Ledger::append (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → Ledger::append_versioned (`fd02b8da-192d-585b-a46d-996b4095186c`)
- ← Ledger::append_evidence (`46d35bf5-9fac-5d57-bfb9-856246acd8b7`)
- ← Ledger::append_event (`0605f067-82d6-503b-9400-c9879e68fe96`)

### Contains

- ← ekos/crates/ledger/src/lib.rs (`21938f45-b767-5933-9e71-12e15ff53eb1`)

## Diagram

```mermaid
graph TD
    n7b3089e3ed305f52bc3ca296097d3b8f["Ledger::append"]
    n21938f45b76759339e7112e15ff53eb1["ekos/crates/ledger/src/lib.rs"]
    n21938f45b76759339e7112e15ff53eb1 -->|Contains| n7b3089e3ed305f52bc3ca296097d3b8f
    nfd02b8da192d585ba46d996b4095186c["Ledger::append_versioned"]
    n7b3089e3ed305f52bc3ca296097d3b8f -->|Calls| nfd02b8da192d585ba46d996b4095186c
    n46d35bf59fac5d57bfb9856246acd8b7["Ledger::append_evidence"]
    n46d35bf59fac5d57bfb9856246acd8b7 -->|Calls| n7b3089e3ed305f52bc3ca296097d3b8f
    n0605f06782d6503b9400c9879e68fe96["Ledger::append_event"]
    n0605f06782d6503b9400c9879e68fe96 -->|Calls| n7b3089e3ed305f52bc3ca296097d3b8f
```

## Evidence

_No evidence cited._
