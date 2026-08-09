# EkosConfig::default (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → MarketingConfig::default (`3fa1fe51-5c40-500e-a4af-e9b3dfeb907f`)
- → WorkspaceConfig::default (`86487b2d-47d6-5105-807b-404b12767a95`)
- → ObserveConfig::default (`db53c97e-788e-587b-a5d4-fbff7497f34e`)
- ← EkosConfig::from_file_or_default (`5c036ac3-203e-5c59-88f2-d79a35ccf921`)

### Contains

- ← ekos/crates/compiler-core/src/config.rs (`d0747f34-25e1-5426-80eb-a54fffcac598`)

## Diagram

```mermaid
graph TD
    n96f3be9497615db0b99a0c705c9c55d0["EkosConfig::default"]
    nd0747f3425e1542680eba54fffcac598["ekos/crates/compiler-core/src/config.rs"]
    nd0747f3425e1542680eba54fffcac598 -->|Contains| n96f3be9497615db0b99a0c705c9c55d0
    n3fa1fe515c40500ea4afe9b3dfeb907f["MarketingConfig::default"]
    n96f3be9497615db0b99a0c705c9c55d0 -->|Calls| n3fa1fe515c40500ea4afe9b3dfeb907f
    n86487b2d47d65105807b404b12767a95["WorkspaceConfig::default"]
    n96f3be9497615db0b99a0c705c9c55d0 -->|Calls| n86487b2d47d65105807b404b12767a95
    ndb53c97e788e587ba5d4fbff7497f34e["ObserveConfig::default"]
    n96f3be9497615db0b99a0c705c9c55d0 -->|Calls| ndb53c97e788e587ba5d4fbff7497f34e
    n5c036ac3203e5c5988f2d79a35ccf921["EkosConfig::from_file_or_default"]
    n5c036ac3203e5c5988f2d79a35ccf921 -->|Calls| n96f3be9497615db0b99a0c705c9c55d0
```

## Evidence

_No evidence cited._
