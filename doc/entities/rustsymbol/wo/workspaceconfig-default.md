# WorkspaceConfig::default (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → default_log_level (`09665155-ef9e-5259-b379-8452b328264d`)
- → default_log_format (`08ad7fde-95bd-5d73-8bc4-8cb5a70b8fd9`)
- → default_root (`7b4baf1d-a71f-5879-b162-60a8f58bb004`)
- ← EkosConfig::default (`96f3be94-9761-5db0-b99a-0c705c9c55d0`)

### Contains

- ← ekos/crates/compiler-core/src/config.rs (`d0747f34-25e1-5426-80eb-a54fffcac598`)

## Diagram

```mermaid
graph TD
    n86487b2d47d65105807b404b12767a95["WorkspaceConfig::default"]
    nd0747f3425e1542680eba54fffcac598["ekos/crates/compiler-core/src/config.rs"]
    nd0747f3425e1542680eba54fffcac598 -->|Contains| n86487b2d47d65105807b404b12767a95
    n09665155ef9e5259b3798452b328264d["default_log_level"]
    n86487b2d47d65105807b404b12767a95 -->|Calls| n09665155ef9e5259b3798452b328264d
    n08ad7fde95bd5d738bc48cb5a70b8fd9["default_log_format"]
    n86487b2d47d65105807b404b12767a95 -->|Calls| n08ad7fde95bd5d738bc48cb5a70b8fd9
    n7b4baf1da71f5879b16260a8f58bb004["default_root"]
    n86487b2d47d65105807b404b12767a95 -->|Calls| n7b4baf1da71f5879b16260a8f58bb004
    n96f3be9497615db0b99a0c705c9c55d0["EkosConfig::default"]
    n96f3be9497615db0b99a0c705c9c55d0 -->|Calls| n86487b2d47d65105807b404b12767a95
```

## Evidence

_No evidence cited._
