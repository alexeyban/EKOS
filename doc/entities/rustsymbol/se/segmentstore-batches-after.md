# SegmentStore::batches_after (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- ← SegmentStore::batches (`b9ee061b-7f32-5424-9ce5-e6bdd92b9a51`)
- → scan_batches_filtered (`cefece15-35d7-567a-889d-48edf2ee6fe1`)
- → SegmentStore::active_batches (`4129d5a0-4d0d-52d4-a36e-ea91694c7f1f`)
- → segment_path (`a07b97cc-69e4-5c35-bedf-11119542af72`)

### Contains

- ← ekos/crates/ledger/src/segment/mod.rs (`80a64e0a-b0ac-51df-b3be-128cddd1cc83`)

## Diagram

```mermaid
graph TD
    necc4c700b5375187a5a9ee023b1d6bf4["SegmentStore::batches_after"]
    n80a64e0ab0ac51dfb3be128cddd1cc83["ekos/crates/ledger/src/segment/mod.rs"]
    n80a64e0ab0ac51dfb3be128cddd1cc83 -->|Contains| necc4c700b5375187a5a9ee023b1d6bf4
    nb9ee061b7f3254249ce5e6bdd92b9a51["SegmentStore::batches"]
    nb9ee061b7f3254249ce5e6bdd92b9a51 -->|Calls| necc4c700b5375187a5a9ee023b1d6bf4
    ncefece1535d7567a889d48edf2ee6fe1["scan_batches_filtered"]
    necc4c700b5375187a5a9ee023b1d6bf4 -->|Calls| ncefece1535d7567a889d48edf2ee6fe1
    n4129d5a04d0d52d4a36eea91694c7f1f["SegmentStore::active_batches"]
    necc4c700b5375187a5a9ee023b1d6bf4 -->|Calls| n4129d5a04d0d52d4a36eea91694c7f1f
    na07b97cc69e45c35bedf11119542af72["segment_path"]
    necc4c700b5375187a5a9ee023b1d6bf4 -->|Calls| na07b97cc69e45c35bedf11119542af72
```

## Evidence

_No evidence cited._
