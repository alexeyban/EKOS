# SegmentStore::seal_active (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- ← SegmentStore::append_with_seal (`e15adeb3-bb2f-54d6-83ee-e79281bea443`)
- → write_head (`203436f8-4f5f-5f2b-93a8-b25fd11e5174`)
- → save_manifest (`a67a0f9c-e213-50e7-9821-68cfa9ebf4d4`)
- → hash_file (`a95eaa64-3902-5bdf-905e-c2436d2c8cc4`)
- → segment_path (`a07b97cc-69e4-5c35-bedf-11119542af72`)
- → scan_headers_slice (`58cd3f37-9374-5ff9-aeea-9fef60615002`)
- → SegmentStore::append (`b324f197-c4fc-5b55-89b4-22ca137bf445`)
- → SegmentStore::open (`3ca4488c-70ab-5992-bde2-4a810cfd8e8d`)

### Contains

- ← ekos/crates/ledger/src/segment/mod.rs (`80a64e0a-b0ac-51df-b3be-128cddd1cc83`)

## Diagram

```mermaid
graph TD
    nf2a44d2b25475a57b0b3ec7494028286["SegmentStore::seal_active"]
    n80a64e0ab0ac51dfb3be128cddd1cc83["ekos/crates/ledger/src/segment/mod.rs"]
    n80a64e0ab0ac51dfb3be128cddd1cc83 -->|Contains| nf2a44d2b25475a57b0b3ec7494028286
    ne15adeb3bb2f54d683eee79281bea443["SegmentStore::append_with_seal"]
    ne15adeb3bb2f54d683eee79281bea443 -->|Calls| nf2a44d2b25475a57b0b3ec7494028286
    n203436f84f5f5f2b93a8b25fd11e5174["write_head"]
    nf2a44d2b25475a57b0b3ec7494028286 -->|Calls| n203436f84f5f5f2b93a8b25fd11e5174
    na67a0f9ce21350e7982168cfa9ebf4d4["save_manifest"]
    nf2a44d2b25475a57b0b3ec7494028286 -->|Calls| na67a0f9ce21350e7982168cfa9ebf4d4
    na95eaa6439025bdf905ec2436d2c8cc4["hash_file"]
    nf2a44d2b25475a57b0b3ec7494028286 -->|Calls| na95eaa6439025bdf905ec2436d2c8cc4
    na07b97cc69e45c35bedf11119542af72["segment_path"]
    nf2a44d2b25475a57b0b3ec7494028286 -->|Calls| na07b97cc69e45c35bedf11119542af72
    n58cd3f3793745ff9aeea9fef60615002["scan_headers_slice"]
    nf2a44d2b25475a57b0b3ec7494028286 -->|Calls| n58cd3f3793745ff9aeea9fef60615002
    nb324f197c4fc5b5589b422ca137bf445["SegmentStore::append"]
    nf2a44d2b25475a57b0b3ec7494028286 -->|Calls| nb324f197c4fc5b5589b422ca137bf445
    n3ca4488c70ab5992bde24a810cfd8e8d["SegmentStore::open"]
    nf2a44d2b25475a57b0b3ec7494028286 -->|Calls| n3ca4488c70ab5992bde24a810cfd8e8d
```

## Evidence

_No evidence cited._
