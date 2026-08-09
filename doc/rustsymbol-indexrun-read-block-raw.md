# IndexRun::read_block_raw (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → decode_block (`b8a0bf10-9570-574c-a274-53930cfd176b`)
- ← IndexRun::scan (`b5797161-ebeb-5639-8a39-f3a0a5517ee2`)
- ← IndexRun::all_raw (`b5bb1598-06cb-5273-b6b4-ea02baf2112a`)

### Contains

- ← ekos/crates/ledger/src/index.rs (`a43fa387-2cac-5166-bfc2-ae4c965fc2ac`)

## Diagram

```mermaid
graph TD
    nd34fdca8cb7f5efabf77acf0e6f3479b["IndexRun::read_block_raw"]
    na43fa3872cac5166bfc2ae4c965fc2ac["ekos/crates/ledger/src/index.rs"]
    na43fa3872cac5166bfc2ae4c965fc2ac -->|Contains| nd34fdca8cb7f5efabf77acf0e6f3479b
    nb8a0bf109570574ca27453930cfd176b["decode_block"]
    nd34fdca8cb7f5efabf77acf0e6f3479b -->|Calls| nb8a0bf109570574ca27453930cfd176b
    nb5797161ebeb56398a39f3a0a5517ee2["IndexRun::scan"]
    nb5797161ebeb56398a39f3a0a5517ee2 -->|Calls| nd34fdca8cb7f5efabf77acf0e6f3479b
    nb5bb159806cb5273b6b4ea02baf2112a["IndexRun::all_raw"]
    nb5bb159806cb5273b6b4ea02baf2112a -->|Calls| nd34fdca8cb7f5efabf77acf0e6f3479b
```

## Evidence

_No evidence cited._
