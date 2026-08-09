# FileSystemArtifactStore::write (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → FileSystemArtifactStore::exists (`09a9951a-1e35-5a79-ae5c-2c9793ae4e34`)
- → FileSystemArtifactStore::artifact_path (`9de3c96c-a715-5997-a156-2a4c8c0b7d70`)

### Contains

- ← ekos/crates/artifact/src/store.rs (`d997f78d-b111-570e-b530-510e98c14df8`)

## Diagram

```mermaid
graph TD
    ncb393b412582587ca703fa1a655dc611["FileSystemArtifactStore::write"]
    nd997f78db111570eb530510e98c14df8["ekos/crates/artifact/src/store.rs"]
    nd997f78db111570eb530510e98c14df8 -->|Contains| ncb393b412582587ca703fa1a655dc611
    n09a9951a1e355a79ae5c2c9793ae4e34["FileSystemArtifactStore::exists"]
    ncb393b412582587ca703fa1a655dc611 -->|Calls| n09a9951a1e355a79ae5c2c9793ae4e34
    n9de3c96ca7155997a1562a4c8c0b7d70["FileSystemArtifactStore::artifact_path"]
    ncb393b412582587ca703fa1a655dc611 -->|Calls| n9de3c96ca7155997a1562a4c8c0b7d70
```

## Evidence

_No evidence cited._
