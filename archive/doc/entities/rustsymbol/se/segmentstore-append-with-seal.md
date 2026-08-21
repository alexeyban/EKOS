# SegmentStore::append_with_seal (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- ← SegmentStore::append (`b324f197-c4fc-5b55-89b4-22ca137bf445`)
- → write_head (`203436f8-4f5f-5f2b-93a8-b25fd11e5174`)
- → SegmentStore::encode_frame (`38292a24-6783-520d-a381-4370713746a4`)
- → SegmentStore::seal_active (`f2a44d2b-2547-5a57-b0b3-ec7494028286`)

### Contains

- ← ekos/crates/ledger/src/segment/mod.rs (`80a64e0a-b0ac-51df-b3be-128cddd1cc83`)

## Diagram

```mermaid
graph TD
    ne15adeb3bb2f54d683eee79281bea443["SegmentStore::append_with_seal"]
    n80a64e0ab0ac51dfb3be128cddd1cc83["ekos/crates/ledger/src/segment/mod.rs"]
    n80a64e0ab0ac51dfb3be128cddd1cc83 -->|Contains| ne15adeb3bb2f54d683eee79281bea443
    nb324f197c4fc5b5589b422ca137bf445["SegmentStore::append"]
    nb324f197c4fc5b5589b422ca137bf445 -->|Calls| ne15adeb3bb2f54d683eee79281bea443
    n203436f84f5f5f2b93a8b25fd11e5174["write_head"]
    ne15adeb3bb2f54d683eee79281bea443 -->|Calls| n203436f84f5f5f2b93a8b25fd11e5174
    n38292a246783520da3814370713746a4["SegmentStore::encode_frame"]
    ne15adeb3bb2f54d683eee79281bea443 -->|Calls| n38292a246783520da3814370713746a4
    nf2a44d2b25475a57b0b3ec7494028286["SegmentStore::seal_active"]
    ne15adeb3bb2f54d683eee79281bea443 -->|Calls| nf2a44d2b25475a57b0b3ec7494028286
```

## Evidence

_No evidence cited._
