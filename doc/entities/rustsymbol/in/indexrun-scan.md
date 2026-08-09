# IndexRun::scan (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → in_prefix (`5aefd266-8072-5c7c-a7da-5e830d6c975d`)
- → IndexRun::read_block_raw (`d34fdca8-cb7f-5efa-bf77-acf0e6f3479b`)

### Contains

- ← ekos/crates/ledger/src/index.rs (`a43fa387-2cac-5166-bfc2-ae4c965fc2ac`)

## Diagram

```mermaid
graph TD
    nb5797161ebeb56398a39f3a0a5517ee2["IndexRun::scan"]
    na43fa3872cac5166bfc2ae4c965fc2ac["ekos/crates/ledger/src/index.rs"]
    na43fa3872cac5166bfc2ae4c965fc2ac -->|Contains| nb5797161ebeb56398a39f3a0a5517ee2
    n5aefd26680725c7ca7da5e830d6c975d["in_prefix"]
    nb5797161ebeb56398a39f3a0a5517ee2 -->|Calls| n5aefd26680725c7ca7da5e830d6c975d
    nd34fdca8cb7f5efabf77acf0e6f3479b["IndexRun::read_block_raw"]
    nb5797161ebeb56398a39f3a0a5517ee2 -->|Calls| nd34fdca8cb7f5efabf77acf0e6f3479b
```

## Evidence

_No evidence cited._
