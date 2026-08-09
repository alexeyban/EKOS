# scan_slice (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← SegmentStore::open_with_seal_threshold (`f884dec9-a3e7-5e4e-88f6-1dc62c172078`)
- → scan_batches_filtered (`cefece15-35d7-567a-889d-48edf2ee6fe1`)

### Contains

- ← ekos/crates/ledger/src/segment/mod.rs (`80a64e0a-b0ac-51df-b3be-128cddd1cc83`)

## Diagram

```mermaid
graph TD
    nff90a0b45f4558afbb522dbcb2df5ccc["scan_slice"]
    n80a64e0ab0ac51dfb3be128cddd1cc83["ekos/crates/ledger/src/segment/mod.rs"]
    n80a64e0ab0ac51dfb3be128cddd1cc83 -->|Contains| nff90a0b45f4558afbb522dbcb2df5ccc
    nf884dec9a3e75e4e88f61dc62c172078["SegmentStore::open_with_seal_threshold"]
    nf884dec9a3e75e4e88f61dc62c172078 -->|Calls| nff90a0b45f4558afbb522dbcb2df5ccc
    ncefece1535d7567a889d48edf2ee6fe1["scan_batches_filtered"]
    nff90a0b45f4558afbb522dbcb2df5ccc -->|Calls| ncefece1535d7567a889d48edf2ee6fe1
```

## Evidence

_No evidence cited._
