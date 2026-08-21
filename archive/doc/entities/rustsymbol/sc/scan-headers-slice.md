# scan_headers_slice (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← SegmentStore::seal_active (`f2a44d2b-2547-5a57-b0b3-ec7494028286`)
- ← SegmentStore::batch_headers (`18f1b204-1468-5fdb-a06f-d7baad93b9f6`)
- → walk_frames (`630f07e4-a407-59c6-bdb6-e7ed592a703b`)
- → decode_header (`c0caddb8-2278-5f35-b313-f93159e56dbf`)

### Contains

- ← ekos/crates/ledger/src/segment/mod.rs (`80a64e0a-b0ac-51df-b3be-128cddd1cc83`)

## Diagram

```mermaid
graph TD
    n58cd3f3793745ff9aeea9fef60615002["scan_headers_slice"]
    n80a64e0ab0ac51dfb3be128cddd1cc83["ekos/crates/ledger/src/segment/mod.rs"]
    n80a64e0ab0ac51dfb3be128cddd1cc83 -->|Contains| n58cd3f3793745ff9aeea9fef60615002
    nf2a44d2b25475a57b0b3ec7494028286["SegmentStore::seal_active"]
    nf2a44d2b25475a57b0b3ec7494028286 -->|Calls| n58cd3f3793745ff9aeea9fef60615002
    n18f1b20414685fdba06fd7baad93b9f6["SegmentStore::batch_headers"]
    n18f1b20414685fdba06fd7baad93b9f6 -->|Calls| n58cd3f3793745ff9aeea9fef60615002
    n630f07e4a40759c6bdb6e7ed592a703b["walk_frames"]
    n58cd3f3793745ff9aeea9fef60615002 -->|Calls| n630f07e4a40759c6bdb6e7ed592a703b
    nc0caddb822785f35b313f93159e56dbf["decode_header"]
    n58cd3f3793745ff9aeea9fef60615002 -->|Calls| nc0caddb822785f35b313f93159e56dbf
```

## Evidence

_No evidence cited._
