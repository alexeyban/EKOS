# PackArtifactStore::open (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → scan_segment (`c4b5e70f-e265-5b5b-9fa4-a7aed1ea48cf`)
- → segment_paths (`798cd9c1-371f-505d-813f-5bb2db68c7fb`)
- ← PackArtifactStore::write_packed (`2c2ad043-598d-5fcb-bf2d-ed4347ca2ae6`)
- ← scan_segment (`c4b5e70f-e265-5b5b-9fa4-a7aed1ea48cf`)

### Contains

- ← ekos/crates/artifact/src/pack.rs (`98cd7507-d9e7-59e3-acfb-c7ffb05d9f73`)

## Diagram

```mermaid
graph TD
    n0047714766fe5667876f59e9dc819b1c["PackArtifactStore::open"]
    n98cd7507d9e759e3acfbc7ffb05d9f73["ekos/crates/artifact/src/pack.rs"]
    n98cd7507d9e759e3acfbc7ffb05d9f73 -->|Contains| n0047714766fe5667876f59e9dc819b1c
    nc4b5e70fe2655b5b9fa4a7aed1ea48cf["scan_segment"]
    n0047714766fe5667876f59e9dc819b1c -->|Calls| nc4b5e70fe2655b5b9fa4a7aed1ea48cf
    n798cd9c1371f505d813f5bb2db68c7fb["segment_paths"]
    n0047714766fe5667876f59e9dc819b1c -->|Calls| n798cd9c1371f505d813f5bb2db68c7fb
    n2c2ad043598d5fcbbf2ded4347ca2ae6["PackArtifactStore::write_packed"]
    n2c2ad043598d5fcbbf2ded4347ca2ae6 -->|Calls| n0047714766fe5667876f59e9dc819b1c
    nc4b5e70fe2655b5b9fa4a7aed1ea48cf -->|Calls| n0047714766fe5667876f59e9dc819b1c
```

## Evidence

_No evidence cited._
