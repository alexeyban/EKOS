# PackArtifactStore::write (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → PackArtifactStore::write_packed (`2c2ad043-598d-5fcb-bf2d-ed4347ca2ae6`)
- → PackArtifactStore::exists (`a58cd217-2bcf-5516-83b6-1a7b8db7f75e`)
- ← scan_segment (`c4b5e70f-e265-5b5b-9fa4-a7aed1ea48cf`)

### Contains

- ← ekos/crates/artifact/src/pack.rs (`98cd7507-d9e7-59e3-acfb-c7ffb05d9f73`)

## Diagram

```mermaid
graph TD
    nf153ee6296d15be6bdb9249eab66c33b["PackArtifactStore::write"]
    n98cd7507d9e759e3acfbc7ffb05d9f73["ekos/crates/artifact/src/pack.rs"]
    n98cd7507d9e759e3acfbc7ffb05d9f73 -->|Contains| nf153ee6296d15be6bdb9249eab66c33b
    n2c2ad043598d5fcbbf2ded4347ca2ae6["PackArtifactStore::write_packed"]
    nf153ee6296d15be6bdb9249eab66c33b -->|Calls| n2c2ad043598d5fcbbf2ded4347ca2ae6
    na58cd2172bcf551683b61a7b8db7f75e["PackArtifactStore::exists"]
    nf153ee6296d15be6bdb9249eab66c33b -->|Calls| na58cd2172bcf551683b61a7b8db7f75e
    nc4b5e70fe2655b5b9fa4a7aed1ea48cf["scan_segment"]
    nc4b5e70fe2655b5b9fa4a7aed1ea48cf -->|Calls| nf153ee6296d15be6bdb9249eab66c33b
```

## Evidence

_No evidence cited._
