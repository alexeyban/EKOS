# FileSystemArtifactStore::exists (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- ← FileSystemArtifactStore::write (`cb393b41-2582-587c-a703-fa1a655dc611`)
- ← FileSystemArtifactStore::read (`d58044fe-3015-5aa0-be86-8031721cf915`)
- → FileSystemArtifactStore::exists (`09a9951a-1e35-5a79-ae5c-2c9793ae4e34`)
- → FileSystemArtifactStore::artifact_path (`9de3c96c-a715-5997-a156-2a4c8c0b7d70`)
- ← FileSystemArtifactStore::list (`c92e2e65-19d6-5c97-be3a-8f52696b3fc3`)

### Contains

- ← ekos/crates/artifact/src/store.rs (`d997f78d-b111-570e-b530-510e98c14df8`)

## Diagram

```mermaid
graph TD
    n09a9951a1e355a79ae5c2c9793ae4e34["FileSystemArtifactStore::exists"]
    nd997f78db111570eb530510e98c14df8["ekos/crates/artifact/src/store.rs"]
    nd997f78db111570eb530510e98c14df8 -->|Contains| n09a9951a1e355a79ae5c2c9793ae4e34
    ncb393b412582587ca703fa1a655dc611["FileSystemArtifactStore::write"]
    ncb393b412582587ca703fa1a655dc611 -->|Calls| n09a9951a1e355a79ae5c2c9793ae4e34
    nd58044fe30155aa0be868031721cf915["FileSystemArtifactStore::read"]
    nd58044fe30155aa0be868031721cf915 -->|Calls| n09a9951a1e355a79ae5c2c9793ae4e34
    n09a9951a1e355a79ae5c2c9793ae4e34 -->|Calls| n09a9951a1e355a79ae5c2c9793ae4e34
    n9de3c96ca7155997a1562a4c8c0b7d70["FileSystemArtifactStore::artifact_path"]
    n09a9951a1e355a79ae5c2c9793ae4e34 -->|Calls| n9de3c96ca7155997a1562a4c8c0b7d70
    nc92e2e6519d65c97be3a8f52696b3fc3["FileSystemArtifactStore::list"]
    nc92e2e6519d65c97be3a8f52696b3fc3 -->|Calls| n09a9951a1e355a79ae5c2c9793ae4e34
```

## Evidence

_No evidence cited._
