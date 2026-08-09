# decode_block (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- → stores_values (`bd8d2a77-ff4c-5ab3-b99f-39afccdb9f6e`)
- ← IndexRun::read_block_raw (`d34fdca8-cb7f-5efa-bf77-acf0e6f3479b`)

### Contains

- ← ekos/crates/ledger/src/index.rs (`a43fa387-2cac-5166-bfc2-ae4c965fc2ac`)

## Diagram

```mermaid
graph TD
    nb8a0bf109570574ca27453930cfd176b["decode_block"]
    na43fa3872cac5166bfc2ae4c965fc2ac["ekos/crates/ledger/src/index.rs"]
    na43fa3872cac5166bfc2ae4c965fc2ac -->|Contains| nb8a0bf109570574ca27453930cfd176b
    nbd8d2a77ff4c5ab3b99f39afccdb9f6e["stores_values"]
    nb8a0bf109570574ca27453930cfd176b -->|Calls| nbd8d2a77ff4c5ab3b99f39afccdb9f6e
    nd34fdca8cb7f5efabf77acf0e6f3479b["IndexRun::read_block_raw"]
    nd34fdca8cb7f5efabf77acf0e6f3479b -->|Calls| nb8a0bf109570574ca27453930cfd176b
```

## Evidence

_No evidence cited._
