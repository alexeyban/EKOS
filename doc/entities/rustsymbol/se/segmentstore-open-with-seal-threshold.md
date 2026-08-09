# SegmentStore::open_with_seal_threshold (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- ← SegmentStore::open (`3ca4488c-70ab-5992-bde2-4a810cfd8e8d`)
- → write_head (`203436f8-4f5f-5f2b-93a8-b25fd11e5174`)
- → build_dict (`2479e892-73dc-5eb6-91fc-6f345d564fdf`)
- → segment_path (`a07b97cc-69e4-5c35-bedf-11119542af72`)
- → load_manifest (`9f01d13d-b1a8-5afe-bc1a-a68a3c06fe3d`)
- → SegmentStore::open (`3ca4488c-70ab-5992-bde2-4a810cfd8e8d`)
- → scan_slice (`ff90a0b4-5f45-58af-bb52-2dbcb2df5ccc`)
- → SegmentStore::append (`b324f197-c4fc-5b55-89b4-22ca137bf445`)

### Contains

- ← ekos/crates/ledger/src/segment/mod.rs (`80a64e0a-b0ac-51df-b3be-128cddd1cc83`)

## Diagram

```mermaid
graph TD
    nf884dec9a3e75e4e88f61dc62c172078["SegmentStore::open_with_seal_threshold"]
    n80a64e0ab0ac51dfb3be128cddd1cc83["ekos/crates/ledger/src/segment/mod.rs"]
    n80a64e0ab0ac51dfb3be128cddd1cc83 -->|Contains| nf884dec9a3e75e4e88f61dc62c172078
    n3ca4488c70ab5992bde24a810cfd8e8d["SegmentStore::open"]
    n3ca4488c70ab5992bde24a810cfd8e8d -->|Calls| nf884dec9a3e75e4e88f61dc62c172078
    n203436f84f5f5f2b93a8b25fd11e5174["write_head"]
    nf884dec9a3e75e4e88f61dc62c172078 -->|Calls| n203436f84f5f5f2b93a8b25fd11e5174
    n2479e89273dc5eb691fc6f345d564fdf["build_dict"]
    nf884dec9a3e75e4e88f61dc62c172078 -->|Calls| n2479e89273dc5eb691fc6f345d564fdf
    na07b97cc69e45c35bedf11119542af72["segment_path"]
    nf884dec9a3e75e4e88f61dc62c172078 -->|Calls| na07b97cc69e45c35bedf11119542af72
    n9f01d13db1a85afebc1aa68a3c06fe3d["load_manifest"]
    nf884dec9a3e75e4e88f61dc62c172078 -->|Calls| n9f01d13db1a85afebc1aa68a3c06fe3d
    nf884dec9a3e75e4e88f61dc62c172078 -->|Calls| n3ca4488c70ab5992bde24a810cfd8e8d
    nff90a0b45f4558afbb522dbcb2df5ccc["scan_slice"]
    nf884dec9a3e75e4e88f61dc62c172078 -->|Calls| nff90a0b45f4558afbb522dbcb2df5ccc
    nb324f197c4fc5b5589b422ca137bf445["SegmentStore::append"]
    nf884dec9a3e75e4e88f61dc62c172078 -->|Calls| nb324f197c4fc5b5589b422ca137bf445
```

## Evidence

_No evidence cited._
