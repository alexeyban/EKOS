# SegmentStore::active_batches (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- ← SegmentStore::batches_after (`ecc4c700-b537-5187-a5a9-ee023b1d6bf4`)
- → scan_batches_filtered (`cefece15-35d7-567a-889d-48edf2ee6fe1`)
- → SegmentStore::read_active_committed (`2119ef15-fc0f-5785-9e29-d84e4f14ac23`)

### Contains

- ← ekos/crates/ledger/src/segment/mod.rs (`80a64e0a-b0ac-51df-b3be-128cddd1cc83`)

## Diagram

```mermaid
graph TD
    n4129d5a04d0d52d4a36eea91694c7f1f["SegmentStore::active_batches"]
    n80a64e0ab0ac51dfb3be128cddd1cc83["ekos/crates/ledger/src/segment/mod.rs"]
    n80a64e0ab0ac51dfb3be128cddd1cc83 -->|Contains| n4129d5a04d0d52d4a36eea91694c7f1f
    necc4c700b5375187a5a9ee023b1d6bf4["SegmentStore::batches_after"]
    necc4c700b5375187a5a9ee023b1d6bf4 -->|Calls| n4129d5a04d0d52d4a36eea91694c7f1f
    ncefece1535d7567a889d48edf2ee6fe1["scan_batches_filtered"]
    n4129d5a04d0d52d4a36eea91694c7f1f -->|Calls| ncefece1535d7567a889d48edf2ee6fe1
    n2119ef15fc0f57859e29d84e4f14ac23["SegmentStore::read_active_committed"]
    n4129d5a04d0d52d4a36eea91694c7f1f -->|Calls| n2119ef15fc0f57859e29d84e4f14ac23
```

## Evidence

_No evidence cited._
