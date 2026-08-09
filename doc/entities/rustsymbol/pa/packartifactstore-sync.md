# PackArtifactStore::sync (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- ← PackArtifactStore::repack_loose (`a9bd0f7a-c0de-5f6f-8504-6946351fd959`)
- ← PackArtifactStore::drop (`71adff97-0fbf-57dd-a203-0739f8dd7bf8`)

### Contains

- ← ekos/crates/artifact/src/pack.rs (`98cd7507-d9e7-59e3-acfb-c7ffb05d9f73`)

## Diagram

```mermaid
graph TD
    ndf1c1ef5b7605b80a4bd17b6354cf81b["PackArtifactStore::sync"]
    n98cd7507d9e759e3acfbc7ffb05d9f73["ekos/crates/artifact/src/pack.rs"]
    n98cd7507d9e759e3acfbc7ffb05d9f73 -->|Contains| ndf1c1ef5b7605b80a4bd17b6354cf81b
    na9bd0f7ac0de5f6f85046946351fd959["PackArtifactStore::repack_loose"]
    na9bd0f7ac0de5f6f85046946351fd959 -->|Calls| ndf1c1ef5b7605b80a4bd17b6354cf81b
    n71adff970fbf57dda2030739f8dd7bf8["PackArtifactStore::drop"]
    n71adff970fbf57dda2030739f8dd7bf8 -->|Calls| ndf1c1ef5b7605b80a4bd17b6354cf81b
```

## Evidence

_No evidence cited._
