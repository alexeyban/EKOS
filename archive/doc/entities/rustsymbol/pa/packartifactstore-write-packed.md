# PackArtifactStore::write_packed (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- ← PackArtifactStore::repack_loose (`a9bd0f7a-c0de-5f6f-8504-6946351fd959`)
- → PackArtifactStore::segment_path (`81f9c174-cef7-5857-bc30-9e1cdef8c67a`)
- → hex_id_to_raw (`e1da57b7-834b-5ec6-aaa8-ccb775efd8bc`)
- → PackArtifactStore::open (`00477147-66fe-5667-876f-59e9dc819b1c`)
- → compress_frame_body (`3ac7addf-75fc-54be-b6be-8627d750462e`)
- ← PackArtifactStore::write (`f153ee62-96d1-5be6-bdb9-249eab66c33b`)

### Contains

- ← ekos/crates/artifact/src/pack.rs (`98cd7507-d9e7-59e3-acfb-c7ffb05d9f73`)

## Diagram

```mermaid
graph TD
    n2c2ad043598d5fcbbf2ded4347ca2ae6["PackArtifactStore::write_packed"]
    n98cd7507d9e759e3acfbc7ffb05d9f73["ekos/crates/artifact/src/pack.rs"]
    n98cd7507d9e759e3acfbc7ffb05d9f73 -->|Contains| n2c2ad043598d5fcbbf2ded4347ca2ae6
    na9bd0f7ac0de5f6f85046946351fd959["PackArtifactStore::repack_loose"]
    na9bd0f7ac0de5f6f85046946351fd959 -->|Calls| n2c2ad043598d5fcbbf2ded4347ca2ae6
    n81f9c174cef75857bc309e1cdef8c67a["PackArtifactStore::segment_path"]
    n2c2ad043598d5fcbbf2ded4347ca2ae6 -->|Calls| n81f9c174cef75857bc309e1cdef8c67a
    ne1da57b7834b5ec6aaa8ccb775efd8bc["hex_id_to_raw"]
    n2c2ad043598d5fcbbf2ded4347ca2ae6 -->|Calls| ne1da57b7834b5ec6aaa8ccb775efd8bc
    n0047714766fe5667876f59e9dc819b1c["PackArtifactStore::open"]
    n2c2ad043598d5fcbbf2ded4347ca2ae6 -->|Calls| n0047714766fe5667876f59e9dc819b1c
    n3ac7addf75fc54beb6be8627d750462e["compress_frame_body"]
    n2c2ad043598d5fcbbf2ded4347ca2ae6 -->|Calls| n3ac7addf75fc54beb6be8627d750462e
    nf153ee6296d15be6bdb9249eab66c33b["PackArtifactStore::write"]
    nf153ee6296d15be6bdb9249eab66c33b -->|Calls| n2c2ad043598d5fcbbf2ded4347ca2ae6
```

## Evidence

_No evidence cited._
