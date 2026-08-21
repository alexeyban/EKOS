# build_dict (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← SegmentStore::open_with_seal_threshold (`f884dec9-a3e7-5e4e-88f6-1dc62c172078`)
- ← SegmentStore::set_dictionary (`2b91f5c6-37e4-54ff-9f4b-735785f13c04`)

### Contains

- ← ekos/crates/ledger/src/segment/mod.rs (`80a64e0a-b0ac-51df-b3be-128cddd1cc83`)

## Diagram

```mermaid
graph TD
    n2479e89273dc5eb691fc6f345d564fdf["build_dict"]
    n80a64e0ab0ac51dfb3be128cddd1cc83["ekos/crates/ledger/src/segment/mod.rs"]
    n80a64e0ab0ac51dfb3be128cddd1cc83 -->|Contains| n2479e89273dc5eb691fc6f345d564fdf
    nf884dec9a3e75e4e88f61dc62c172078["SegmentStore::open_with_seal_threshold"]
    nf884dec9a3e75e4e88f61dc62c172078 -->|Calls| n2479e89273dc5eb691fc6f345d564fdf
    n2b91f5c637e454ff9f4b735785f13c04["SegmentStore::set_dictionary"]
    n2b91f5c637e454ff9f4b735785f13c04 -->|Calls| n2479e89273dc5eb691fc6f345d564fdf
```

## Evidence

_No evidence cited._
