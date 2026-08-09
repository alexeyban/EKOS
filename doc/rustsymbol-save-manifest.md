# save_manifest (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← SegmentStore::set_dictionary (`2b91f5c6-37e4-54ff-9f4b-735785f13c04`)
- ← SegmentStore::seal_active (`f2a44d2b-2547-5a57-b0b3-ec7494028286`)
- ← SegmentStore::persist_manifest (`f95100d5-153e-5cba-a18c-3f4cc681d1d1`)
- → atomic_write (`71c4923a-8f1e-587c-a91a-37769f53c149`)

### Contains

- ← ekos/crates/ledger/src/segment/mod.rs (`80a64e0a-b0ac-51df-b3be-128cddd1cc83`)

## Diagram

```mermaid
graph TD
    na67a0f9ce21350e7982168cfa9ebf4d4["save_manifest"]
    n80a64e0ab0ac51dfb3be128cddd1cc83["ekos/crates/ledger/src/segment/mod.rs"]
    n80a64e0ab0ac51dfb3be128cddd1cc83 -->|Contains| na67a0f9ce21350e7982168cfa9ebf4d4
    n2b91f5c637e454ff9f4b735785f13c04["SegmentStore::set_dictionary"]
    n2b91f5c637e454ff9f4b735785f13c04 -->|Calls| na67a0f9ce21350e7982168cfa9ebf4d4
    nf2a44d2b25475a57b0b3ec7494028286["SegmentStore::seal_active"]
    nf2a44d2b25475a57b0b3ec7494028286 -->|Calls| na67a0f9ce21350e7982168cfa9ebf4d4
    nf95100d5153e5cbaa18c3f4cc681d1d1["SegmentStore::persist_manifest"]
    nf95100d5153e5cbaa18c3f4cc681d1d1 -->|Calls| na67a0f9ce21350e7982168cfa9ebf4d4
    n71c4923a8f1e587ca91a37769f53c149["atomic_write"]
    na67a0f9ce21350e7982168cfa9ebf4d4 -->|Calls| n71c4923a8f1e587ca91a37769f53c149
```

## Evidence

_No evidence cited._
