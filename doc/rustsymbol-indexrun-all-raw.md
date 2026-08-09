# IndexRun::all_raw (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → IndexRun::read_block_raw (`d34fdca8-cb7f-5efa-bf77-acf0e6f3479b`)
- ← IndexRun::all (`fc613d47-220e-54bb-8822-c82bf2cacea4`)
- ← FactIndexes::merge_runs (`7656c624-f7b2-53e8-9af8-faafa094666c`)

### Contains

- ← ekos/crates/ledger/src/index.rs (`a43fa387-2cac-5166-bfc2-ae4c965fc2ac`)

## Diagram

```mermaid
graph TD
    nb5bb159806cb5273b6b4ea02baf2112a["IndexRun::all_raw"]
    na43fa3872cac5166bfc2ae4c965fc2ac["ekos/crates/ledger/src/index.rs"]
    na43fa3872cac5166bfc2ae4c965fc2ac -->|Contains| nb5bb159806cb5273b6b4ea02baf2112a
    nd34fdca8cb7f5efabf77acf0e6f3479b["IndexRun::read_block_raw"]
    nb5bb159806cb5273b6b4ea02baf2112a -->|Calls| nd34fdca8cb7f5efabf77acf0e6f3479b
    nfc613d47220e54bb8822c82bf2cacea4["IndexRun::all"]
    nfc613d47220e54bb8822c82bf2cacea4 -->|Calls| nb5bb159806cb5273b6b4ea02baf2112a
    n7656c624f7b253e89af8faafa094666c["FactIndexes::merge_runs"]
    n7656c624f7b253e89af8faafa094666c -->|Calls| nb5bb159806cb5273b6b4ea02baf2112a
```

## Evidence

_No evidence cited._
