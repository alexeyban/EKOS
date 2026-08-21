# FactIndexes::scan (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → ScanPrefix::bytes (`ca1b3e8c-f49b-5d36-a16e-a45d06960172`)
- → encode_key (`5f3049a2-8e65-5510-9ee3-ab92690f254a`)

### Contains

- ← ekos/crates/ledger/src/index.rs (`a43fa387-2cac-5166-bfc2-ae4c965fc2ac`)

## Diagram

```mermaid
graph TD
    na6eecec9893d552b9d21a9ca35b1c87d["FactIndexes::scan"]
    na43fa3872cac5166bfc2ae4c965fc2ac["ekos/crates/ledger/src/index.rs"]
    na43fa3872cac5166bfc2ae4c965fc2ac -->|Contains| na6eecec9893d552b9d21a9ca35b1c87d
    nca1b3e8cf49b5d36a16ea45d06960172["ScanPrefix::bytes"]
    na6eecec9893d552b9d21a9ca35b1c87d -->|Calls| nca1b3e8cf49b5d36a16ea45d06960172
    n5f3049a28e6555109ee3ab92690f254a["encode_key"]
    na6eecec9893d552b9d21a9ca35b1c87d -->|Calls| n5f3049a28e6555109ee3ab92690f254a
```

## Evidence

_No evidence cited._
