# encode_block (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← write_run_raw (`be4b7353-317e-58f9-a870-099ca471e2d5`)
- → stores_values (`bd8d2a77-ff4c-5ab3-b99f-39afccdb9f6e`)

### Contains

- ← ekos/crates/ledger/src/index.rs (`a43fa387-2cac-5166-bfc2-ae4c965fc2ac`)

## Diagram

```mermaid
graph TD
    n37e31f5f37d35c9b8e9a9f783c727d76["encode_block"]
    na43fa3872cac5166bfc2ae4c965fc2ac["ekos/crates/ledger/src/index.rs"]
    na43fa3872cac5166bfc2ae4c965fc2ac -->|Contains| n37e31f5f37d35c9b8e9a9f783c727d76
    nbe4b7353317e58f9a870099ca471e2d5["write_run_raw"]
    nbe4b7353317e58f9a870099ca471e2d5 -->|Calls| n37e31f5f37d35c9b8e9a9f783c727d76
    nbd8d2a77ff4c5ab3b99f39afccdb9f6e["stores_values"]
    n37e31f5f37d35c9b8e9a9f783c727d76 -->|Calls| nbd8d2a77ff4c5ab3b99f39afccdb9f6e
```

## Evidence

_No evidence cited._
