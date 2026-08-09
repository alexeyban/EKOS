# ScanPrefix::bytes (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → push_escaped (`479cc024-8a4a-58db-9137-6a0b043be5e8`)
- → value_order_key (`9a9ea627-8891-52e2-a6a7-1ade17a48fa6`)
- ← FactIndexes::scan (`a6eecec9-893d-552b-9d21-a9ca35b1c87d`)

### Contains

- ← ekos/crates/ledger/src/index.rs (`a43fa387-2cac-5166-bfc2-ae4c965fc2ac`)

## Diagram

```mermaid
graph TD
    nca1b3e8cf49b5d36a16ea45d06960172["ScanPrefix::bytes"]
    na43fa3872cac5166bfc2ae4c965fc2ac["ekos/crates/ledger/src/index.rs"]
    na43fa3872cac5166bfc2ae4c965fc2ac -->|Contains| nca1b3e8cf49b5d36a16ea45d06960172
    n479cc0248a4a58db91376a0b043be5e8["push_escaped"]
    nca1b3e8cf49b5d36a16ea45d06960172 -->|Calls| n479cc0248a4a58db91376a0b043be5e8
    n9a9ea627889152e2a6a71ade17a48fa6["value_order_key"]
    nca1b3e8cf49b5d36a16ea45d06960172 -->|Calls| n9a9ea627889152e2a6a71ade17a48fa6
    na6eecec9893d552b9d21a9ca35b1c87d["FactIndexes::scan"]
    na6eecec9893d552b9d21a9ca35b1c87d -->|Calls| nca1b3e8cf49b5d36a16ea45d06960172
```

## Evidence

_No evidence cited._
