# PackArtifactStore::segment_path (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- ← PackArtifactStore::write_packed (`2c2ad043-598d-5fcb-bf2d-ed4347ca2ae6`)
- ← PackArtifactStore::read (`a5061f98-e89d-569e-a13c-cca36a9e7f0a`)

### Contains

- ← ekos/crates/artifact/src/pack.rs (`98cd7507-d9e7-59e3-acfb-c7ffb05d9f73`)

## Diagram

```mermaid
graph TD
    n81f9c174cef75857bc309e1cdef8c67a["PackArtifactStore::segment_path"]
    n98cd7507d9e759e3acfbc7ffb05d9f73["ekos/crates/artifact/src/pack.rs"]
    n98cd7507d9e759e3acfbc7ffb05d9f73 -->|Contains| n81f9c174cef75857bc309e1cdef8c67a
    n2c2ad043598d5fcbbf2ded4347ca2ae6["PackArtifactStore::write_packed"]
    n2c2ad043598d5fcbbf2ded4347ca2ae6 -->|Calls| n81f9c174cef75857bc309e1cdef8c67a
    na5061f98e89d569ea13ccca36a9e7f0a["PackArtifactStore::read"]
    na5061f98e89d569ea13ccca36a9e7f0a -->|Calls| n81f9c174cef75857bc309e1cdef8c67a
```

## Evidence

_No evidence cited._
