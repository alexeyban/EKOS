# SegmentStore::batch_headers (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → segment_path (`a07b97cc-69e4-5c35-bedf-11119542af72`)
- → SegmentStore::read_active_committed (`2119ef15-fc0f-5785-9e29-d84e4f14ac23`)
- → scan_headers_slice (`58cd3f37-9374-5ff9-aeea-9fef60615002`)

### Contains

- ← ekos/crates/ledger/src/segment/mod.rs (`80a64e0a-b0ac-51df-b3be-128cddd1cc83`)

## Diagram

```mermaid
graph TD
    n18f1b20414685fdba06fd7baad93b9f6["SegmentStore::batch_headers"]
    n80a64e0ab0ac51dfb3be128cddd1cc83["ekos/crates/ledger/src/segment/mod.rs"]
    n80a64e0ab0ac51dfb3be128cddd1cc83 -->|Contains| n18f1b20414685fdba06fd7baad93b9f6
    na07b97cc69e45c35bedf11119542af72["segment_path"]
    n18f1b20414685fdba06fd7baad93b9f6 -->|Calls| na07b97cc69e45c35bedf11119542af72
    n2119ef15fc0f57859e29d84e4f14ac23["SegmentStore::read_active_committed"]
    n18f1b20414685fdba06fd7baad93b9f6 -->|Calls| n2119ef15fc0f57859e29d84e4f14ac23
    n58cd3f3793745ff9aeea9fef60615002["scan_headers_slice"]
    n18f1b20414685fdba06fd7baad93b9f6 -->|Calls| n58cd3f3793745ff9aeea9fef60615002
```

## Evidence

_No evidence cited._
