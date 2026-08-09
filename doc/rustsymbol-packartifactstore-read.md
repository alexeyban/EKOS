# PackArtifactStore::read (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- ← PackArtifactStore::repack_loose (`a9bd0f7a-c0de-5f6f-8504-6946351fd959`)
- → PackArtifactStore::segment_path (`81f9c174-cef7-5857-bc30-9e1cdef8c67a`)
- → PackArtifactStore::read (`a5061f98-e89d-569e-a13c-cca36a9e7f0a`)
- ← scan_segment (`c4b5e70f-e265-5b5b-9fa4-a7aed1ea48cf`)

### Contains

- ← ekos/crates/artifact/src/pack.rs (`98cd7507-d9e7-59e3-acfb-c7ffb05d9f73`)

## Diagram

```mermaid
graph TD
    na5061f98e89d569ea13ccca36a9e7f0a["PackArtifactStore::read"]
    n98cd7507d9e759e3acfbc7ffb05d9f73["ekos/crates/artifact/src/pack.rs"]
    n98cd7507d9e759e3acfbc7ffb05d9f73 -->|Contains| na5061f98e89d569ea13ccca36a9e7f0a
    na9bd0f7ac0de5f6f85046946351fd959["PackArtifactStore::repack_loose"]
    na9bd0f7ac0de5f6f85046946351fd959 -->|Calls| na5061f98e89d569ea13ccca36a9e7f0a
    n81f9c174cef75857bc309e1cdef8c67a["PackArtifactStore::segment_path"]
    na5061f98e89d569ea13ccca36a9e7f0a -->|Calls| n81f9c174cef75857bc309e1cdef8c67a
    na5061f98e89d569ea13ccca36a9e7f0a -->|Calls| na5061f98e89d569ea13ccca36a9e7f0a
    nc4b5e70fe2655b5b9fa4a7aed1ea48cf["scan_segment"]
    nc4b5e70fe2655b5b9fa4a7aed1ea48cf -->|Calls| na5061f98e89d569ea13ccca36a9e7f0a
```

## Evidence

_No evidence cited._
