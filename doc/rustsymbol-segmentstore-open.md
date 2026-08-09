# SegmentStore::open (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → SegmentStore::open_with_seal_threshold (`f884dec9-a3e7-5e4e-88f6-1dc62c172078`)
- ← SegmentStore::open_with_seal_threshold (`f884dec9-a3e7-5e4e-88f6-1dc62c172078`)
- ← SegmentStore::seal_active (`f2a44d2b-2547-5a57-b0b3-ec7494028286`)

### Contains

- ← ekos/crates/ledger/src/segment/mod.rs (`80a64e0a-b0ac-51df-b3be-128cddd1cc83`)

## Diagram

```mermaid
graph TD
    n3ca4488c70ab5992bde24a810cfd8e8d["SegmentStore::open"]
    n80a64e0ab0ac51dfb3be128cddd1cc83["ekos/crates/ledger/src/segment/mod.rs"]
    n80a64e0ab0ac51dfb3be128cddd1cc83 -->|Contains| n3ca4488c70ab5992bde24a810cfd8e8d
    nf884dec9a3e75e4e88f61dc62c172078["SegmentStore::open_with_seal_threshold"]
    n3ca4488c70ab5992bde24a810cfd8e8d -->|Calls| nf884dec9a3e75e4e88f61dc62c172078
    nf884dec9a3e75e4e88f61dc62c172078 -->|Calls| n3ca4488c70ab5992bde24a810cfd8e8d
    nf2a44d2b25475a57b0b3ec7494028286["SegmentStore::seal_active"]
    nf2a44d2b25475a57b0b3ec7494028286 -->|Calls| n3ca4488c70ab5992bde24a810cfd8e8d
```

## Evidence

_No evidence cited._
