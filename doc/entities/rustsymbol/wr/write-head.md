# write_head (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← SegmentStore::open_with_seal_threshold (`f884dec9-a3e7-5e4e-88f6-1dc62c172078`)
- ← SegmentStore::append_with_seal (`e15adeb3-bb2f-54d6-83ee-e79281bea443`)
- ← SegmentStore::seal_active (`f2a44d2b-2547-5a57-b0b3-ec7494028286`)
- → atomic_write (`71c4923a-8f1e-587c-a91a-37769f53c149`)

### Contains

- ← ekos/crates/ledger/src/segment/mod.rs (`80a64e0a-b0ac-51df-b3be-128cddd1cc83`)

## Diagram

```mermaid
graph TD
    n203436f84f5f5f2b93a8b25fd11e5174["write_head"]
    n80a64e0ab0ac51dfb3be128cddd1cc83["ekos/crates/ledger/src/segment/mod.rs"]
    n80a64e0ab0ac51dfb3be128cddd1cc83 -->|Contains| n203436f84f5f5f2b93a8b25fd11e5174
    nf884dec9a3e75e4e88f61dc62c172078["SegmentStore::open_with_seal_threshold"]
    nf884dec9a3e75e4e88f61dc62c172078 -->|Calls| n203436f84f5f5f2b93a8b25fd11e5174
    ne15adeb3bb2f54d683eee79281bea443["SegmentStore::append_with_seal"]
    ne15adeb3bb2f54d683eee79281bea443 -->|Calls| n203436f84f5f5f2b93a8b25fd11e5174
    nf2a44d2b25475a57b0b3ec7494028286["SegmentStore::seal_active"]
    nf2a44d2b25475a57b0b3ec7494028286 -->|Calls| n203436f84f5f5f2b93a8b25fd11e5174
    n71c4923a8f1e587ca91a37769f53c149["atomic_write"]
    n203436f84f5f5f2b93a8b25fd11e5174 -->|Calls| n71c4923a8f1e587ca91a37769f53c149
```

## Evidence

_No evidence cited._
