# SegmentStore::read_active_committed (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- ← SegmentStore::batch_headers (`18f1b204-1468-5fdb-a06f-d7baad93b9f6`)
- → segment_path (`a07b97cc-69e4-5c35-bedf-11119542af72`)
- ← SegmentStore::active_batches (`4129d5a0-4d0d-52d4-a36e-ea91694c7f1f`)

### Contains

- ← ekos/crates/ledger/src/segment/mod.rs (`80a64e0a-b0ac-51df-b3be-128cddd1cc83`)

## Diagram

```mermaid
graph TD
    n2119ef15fc0f57859e29d84e4f14ac23["SegmentStore::read_active_committed"]
    n80a64e0ab0ac51dfb3be128cddd1cc83["ekos/crates/ledger/src/segment/mod.rs"]
    n80a64e0ab0ac51dfb3be128cddd1cc83 -->|Contains| n2119ef15fc0f57859e29d84e4f14ac23
    n18f1b20414685fdba06fd7baad93b9f6["SegmentStore::batch_headers"]
    n18f1b20414685fdba06fd7baad93b9f6 -->|Calls| n2119ef15fc0f57859e29d84e4f14ac23
    na07b97cc69e45c35bedf11119542af72["segment_path"]
    n2119ef15fc0f57859e29d84e4f14ac23 -->|Calls| na07b97cc69e45c35bedf11119542af72
    n4129d5a04d0d52d4a36eea91694c7f1f["SegmentStore::active_batches"]
    n4129d5a04d0d52d4a36eea91694c7f1f -->|Calls| n2119ef15fc0f57859e29d84e4f14ac23
```

## Evidence

_No evidence cited._
