# write_run_raw (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← write_run (`944c015c-ad50-5e55-9238-7d57ee4ed67b`)
- → encode_block (`37e31f5f-37d3-5c9b-8e9a-9f783c727d76`)
- ← FactIndexes::merge_runs (`7656c624-f7b2-53e8-9af8-faafa094666c`)

### Contains

- ← ekos/crates/ledger/src/index.rs (`a43fa387-2cac-5166-bfc2-ae4c965fc2ac`)

## Diagram

```mermaid
graph TD
    nbe4b7353317e58f9a870099ca471e2d5["write_run_raw"]
    na43fa3872cac5166bfc2ae4c965fc2ac["ekos/crates/ledger/src/index.rs"]
    na43fa3872cac5166bfc2ae4c965fc2ac -->|Contains| nbe4b7353317e58f9a870099ca471e2d5
    n944c015cad505e5592387d57ee4ed67b["write_run"]
    n944c015cad505e5592387d57ee4ed67b -->|Calls| nbe4b7353317e58f9a870099ca471e2d5
    n37e31f5f37d35c9b8e9a9f783c727d76["encode_block"]
    nbe4b7353317e58f9a870099ca471e2d5 -->|Calls| n37e31f5f37d35c9b8e9a9f783c727d76
    n7656c624f7b253e89af8faafa094666c["FactIndexes::merge_runs"]
    n7656c624f7b253e89af8faafa094666c -->|Calls| nbe4b7353317e58f9a870099ca471e2d5
```

## Evidence

_No evidence cited._
