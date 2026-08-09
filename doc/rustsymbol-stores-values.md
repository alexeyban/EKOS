# stores_values (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← project (`775a5c11-b65e-541d-a531-4ca75476a2b6`)
- ← encode_block (`37e31f5f-37d3-5c9b-8e9a-9f783c727d76`)
- ← decode_block (`b8a0bf10-9570-574c-a274-53930cfd176b`)

### Contains

- ← ekos/crates/ledger/src/index.rs (`a43fa387-2cac-5166-bfc2-ae4c965fc2ac`)

## Diagram

```mermaid
graph TD
    nbd8d2a77ff4c5ab3b99f39afccdb9f6e["stores_values"]
    na43fa3872cac5166bfc2ae4c965fc2ac["ekos/crates/ledger/src/index.rs"]
    na43fa3872cac5166bfc2ae4c965fc2ac -->|Contains| nbd8d2a77ff4c5ab3b99f39afccdb9f6e
    n775a5c11b65e541da5314ca75476a2b6["project"]
    n775a5c11b65e541da5314ca75476a2b6 -->|Calls| nbd8d2a77ff4c5ab3b99f39afccdb9f6e
    n37e31f5f37d35c9b8e9a9f783c727d76["encode_block"]
    n37e31f5f37d35c9b8e9a9f783c727d76 -->|Calls| nbd8d2a77ff4c5ab3b99f39afccdb9f6e
    nb8a0bf109570574ca27453930cfd176b["decode_block"]
    nb8a0bf109570574ca27453930cfd176b -->|Calls| nbd8d2a77ff4c5ab3b99f39afccdb9f6e
```

## Evidence

_No evidence cited._
