# scan_segment (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← PackArtifactStore::open (`00477147-66fe-5667-876f-59e9dc819b1c`)
- → PackArtifactStore::read (`a5061f98-e89d-569e-a13c-cca36a9e7f0a`)
- → PackArtifactStore::write (`f153ee62-96d1-5be6-bdb9-249eab66c33b`)
- → PackArtifactStore::open (`00477147-66fe-5667-876f-59e9dc819b1c`)

### Contains

- ← ekos/crates/artifact/src/pack.rs (`98cd7507-d9e7-59e3-acfb-c7ffb05d9f73`)

## Diagram

```mermaid
graph TD
    nc4b5e70fe2655b5b9fa4a7aed1ea48cf["scan_segment"]
    n98cd7507d9e759e3acfbc7ffb05d9f73["ekos/crates/artifact/src/pack.rs"]
    n98cd7507d9e759e3acfbc7ffb05d9f73 -->|Contains| nc4b5e70fe2655b5b9fa4a7aed1ea48cf
    n0047714766fe5667876f59e9dc819b1c["PackArtifactStore::open"]
    n0047714766fe5667876f59e9dc819b1c -->|Calls| nc4b5e70fe2655b5b9fa4a7aed1ea48cf
    na5061f98e89d569ea13ccca36a9e7f0a["PackArtifactStore::read"]
    nc4b5e70fe2655b5b9fa4a7aed1ea48cf -->|Calls| na5061f98e89d569ea13ccca36a9e7f0a
    nf153ee6296d15be6bdb9249eab66c33b["PackArtifactStore::write"]
    nc4b5e70fe2655b5b9fa4a7aed1ea48cf -->|Calls| nf153ee6296d15be6bdb9249eab66c33b
    nc4b5e70fe2655b5b9fa4a7aed1ea48cf -->|Calls| n0047714766fe5667876f59e9dc819b1c
```

## Evidence

_No evidence cited._
