# Ledger::export_versions (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → ts_value_to_datetime (`34ebc45d-e426-5788-9f8b-c605bf91a6a3`)
- → Ledger::payload_to_string (`b30e2764-552e-5d3e-a1e5-34c523dd7475`)
- → id_value_to_string (`a0c3d0ec-3294-5534-a1f2-b2295cc7d77a`)
- ← migrate_to_v3 (`1dab3f65-615b-56e9-ae9b-e92c32a2cb63`)

### Contains

- ← ekos/crates/ledger/src/lib.rs (`21938f45-b767-5933-9e71-12e15ff53eb1`)

## Diagram

```mermaid
graph TD
    n1ed3c4b0eefc5cee8f3bf559c0e5f97e["Ledger::export_versions"]
    n21938f45b76759339e7112e15ff53eb1["ekos/crates/ledger/src/lib.rs"]
    n21938f45b76759339e7112e15ff53eb1 -->|Contains| n1ed3c4b0eefc5cee8f3bf559c0e5f97e
    n34ebc45de42657889f8bc605bf91a6a3["ts_value_to_datetime"]
    n1ed3c4b0eefc5cee8f3bf559c0e5f97e -->|Calls| n34ebc45de42657889f8bc605bf91a6a3
    nb30e2764552e5d3ea1e534c523dd7475["Ledger::payload_to_string"]
    n1ed3c4b0eefc5cee8f3bf559c0e5f97e -->|Calls| nb30e2764552e5d3ea1e534c523dd7475
    na0c3d0ec32945534a1f2b2295cc7d77a["id_value_to_string"]
    n1ed3c4b0eefc5cee8f3bf559c0e5f97e -->|Calls| na0c3d0ec32945534a1f2b2295cc7d77a
    n1dab3f65615b56e9ae9be92c32a2cb63["migrate_to_v3"]
    n1dab3f65615b56e9ae9be92c32a2cb63 -->|Calls| n1ed3c4b0eefc5cee8f3bf559c0e5f97e
```

## Evidence

_No evidence cited._
