# Ledger::get_object (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → Ledger::payload_to_string (`b30e2764-552e-5d3e-a1e5-34c523dd7475`)
- ← merge_branch (`16be84c8-16f2-5d63-8dff-104f7296fc29`)
- ← merge_stores (`35e9663b-3b6d-50ec-ad16-9721c45eb3d1`)

### Contains

- ← ekos/crates/ledger/src/lib.rs (`21938f45-b767-5933-9e71-12e15ff53eb1`)

## Diagram

```mermaid
graph TD
    nbc4b77e96e8d54b0aa9a8fc066a535b3["Ledger::get_object"]
    n21938f45b76759339e7112e15ff53eb1["ekos/crates/ledger/src/lib.rs"]
    n21938f45b76759339e7112e15ff53eb1 -->|Contains| nbc4b77e96e8d54b0aa9a8fc066a535b3
    nb30e2764552e5d3ea1e534c523dd7475["Ledger::payload_to_string"]
    nbc4b77e96e8d54b0aa9a8fc066a535b3 -->|Calls| nb30e2764552e5d3ea1e534c523dd7475
    n16be84c816f25d638dff104f7296fc29["merge_branch"]
    n16be84c816f25d638dff104f7296fc29 -->|Calls| nbc4b77e96e8d54b0aa9a8fc066a535b3
    n35e9663b3b6d50ecad169721c45eb3d1["merge_stores"]
    n35e9663b3b6d50ecad169721c45eb3d1 -->|Calls| nbc4b77e96e8d54b0aa9a8fc066a535b3
```

## Evidence

_No evidence cited._
