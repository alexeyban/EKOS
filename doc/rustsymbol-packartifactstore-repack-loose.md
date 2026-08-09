# PackArtifactStore::repack_loose (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → PackArtifactStore::write_packed (`2c2ad043-598d-5fcb-bf2d-ed4347ca2ae6`)
- → PackArtifactStore::sync (`df1c1ef5-b760-5b80-a4bd-17b6354cf81b`)
- → prune_empty_dirs (`9b7c0842-a194-54db-ba30-8784f41ab6f6`)
- → PackArtifactStore::loose_path (`a2a2fda7-285e-5052-adf8-adb45d5323ab`)
- → PackArtifactStore::list (`fefadbb6-d1c4-5a6e-8f67-72b562790708`)
- → PackArtifactStore::read (`a5061f98-e89d-569e-a13c-cca36a9e7f0a`)

### Contains

- ← ekos/crates/artifact/src/pack.rs (`98cd7507-d9e7-59e3-acfb-c7ffb05d9f73`)

## Diagram

```mermaid
graph TD
    na9bd0f7ac0de5f6f85046946351fd959["PackArtifactStore::repack_loose"]
    n98cd7507d9e759e3acfbc7ffb05d9f73["ekos/crates/artifact/src/pack.rs"]
    n98cd7507d9e759e3acfbc7ffb05d9f73 -->|Contains| na9bd0f7ac0de5f6f85046946351fd959
    n2c2ad043598d5fcbbf2ded4347ca2ae6["PackArtifactStore::write_packed"]
    na9bd0f7ac0de5f6f85046946351fd959 -->|Calls| n2c2ad043598d5fcbbf2ded4347ca2ae6
    ndf1c1ef5b7605b80a4bd17b6354cf81b["PackArtifactStore::sync"]
    na9bd0f7ac0de5f6f85046946351fd959 -->|Calls| ndf1c1ef5b7605b80a4bd17b6354cf81b
    n9b7c0842a19454dbba308784f41ab6f6["prune_empty_dirs"]
    na9bd0f7ac0de5f6f85046946351fd959 -->|Calls| n9b7c0842a19454dbba308784f41ab6f6
    na2a2fda7285e5052adf8adb45d5323ab["PackArtifactStore::loose_path"]
    na9bd0f7ac0de5f6f85046946351fd959 -->|Calls| na2a2fda7285e5052adf8adb45d5323ab
    nfefadbb6d1c45a6e8f6772b562790708["PackArtifactStore::list"]
    na9bd0f7ac0de5f6f85046946351fd959 -->|Calls| nfefadbb6d1c45a6e8f6772b562790708
    na5061f98e89d569ea13ccca36a9e7f0a["PackArtifactStore::read"]
    na9bd0f7ac0de5f6f85046946351fd959 -->|Calls| na5061f98e89d569ea13ccca36a9e7f0a
```

## Evidence

_No evidence cited._
