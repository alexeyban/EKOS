# value_order_key (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← encode_key (`5f3049a2-8e65-5510-9ee3-ab92690f254a`)
- ← ScanPrefix::bytes (`ca1b3e8c-f49b-5d36-a16e-a45d06960172`)

### Contains

- ← ekos/crates/ledger/src/index.rs (`a43fa387-2cac-5166-bfc2-ae4c965fc2ac`)

## Diagram

```mermaid
graph TD
    n9a9ea627889152e2a6a71ade17a48fa6["value_order_key"]
    na43fa3872cac5166bfc2ae4c965fc2ac["ekos/crates/ledger/src/index.rs"]
    na43fa3872cac5166bfc2ae4c965fc2ac -->|Contains| n9a9ea627889152e2a6a71ade17a48fa6
    n5f3049a28e6555109ee3ab92690f254a["encode_key"]
    n5f3049a28e6555109ee3ab92690f254a -->|Calls| n9a9ea627889152e2a6a71ade17a48fa6
    nca1b3e8cf49b5d36a16ea45d06960172["ScanPrefix::bytes"]
    nca1b3e8cf49b5d36a16ea45d06960172 -->|Calls| n9a9ea627889152e2a6a71ade17a48fa6
```

## Evidence

_No evidence cited._
