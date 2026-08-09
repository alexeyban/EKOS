# scan_batches_filtered (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← SegmentStore::batches_after (`ecc4c700-b537-5187-a5a9-ee023b1d6bf4`)
- ← SegmentStore::active_batches (`4129d5a0-4d0d-52d4-a36e-ea91694c7f1f`)
- ← scan_slice (`ff90a0b4-5f45-58af-bb52-2dbcb2df5ccc`)
- → decode_frame (`ac39fcb4-9dc0-595d-af9b-bce1eb69f50c`)
- → decode_header (`c0caddb8-2278-5f35-b313-f93159e56dbf`)
- → walk_frames (`630f07e4-a407-59c6-bdb6-e7ed592a703b`)

### Contains

- ← ekos/crates/ledger/src/segment/mod.rs (`80a64e0a-b0ac-51df-b3be-128cddd1cc83`)

## Diagram

```mermaid
graph TD
    ncefece1535d7567a889d48edf2ee6fe1["scan_batches_filtered"]
    n80a64e0ab0ac51dfb3be128cddd1cc83["ekos/crates/ledger/src/segment/mod.rs"]
    n80a64e0ab0ac51dfb3be128cddd1cc83 -->|Contains| ncefece1535d7567a889d48edf2ee6fe1
    necc4c700b5375187a5a9ee023b1d6bf4["SegmentStore::batches_after"]
    necc4c700b5375187a5a9ee023b1d6bf4 -->|Calls| ncefece1535d7567a889d48edf2ee6fe1
    n4129d5a04d0d52d4a36eea91694c7f1f["SegmentStore::active_batches"]
    n4129d5a04d0d52d4a36eea91694c7f1f -->|Calls| ncefece1535d7567a889d48edf2ee6fe1
    nff90a0b45f4558afbb522dbcb2df5ccc["scan_slice"]
    nff90a0b45f4558afbb522dbcb2df5ccc -->|Calls| ncefece1535d7567a889d48edf2ee6fe1
    nac39fcb49dc0595daf9bbce1eb69f50c["decode_frame"]
    ncefece1535d7567a889d48edf2ee6fe1 -->|Calls| nac39fcb49dc0595daf9bbce1eb69f50c
    nc0caddb822785f35b313f93159e56dbf["decode_header"]
    ncefece1535d7567a889d48edf2ee6fe1 -->|Calls| nc0caddb822785f35b313f93159e56dbf
    n630f07e4a40759c6bdb6e7ed592a703b["walk_frames"]
    ncefece1535d7567a889d48edf2ee6fe1 -->|Calls| n630f07e4a40759c6bdb6e7ed592a703b
```

## Evidence

_No evidence cited._
