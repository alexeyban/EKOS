# ObserveConfig::default (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → default_ignore_patterns (`8365dc18-7a68-5b6c-bb0b-fcddf13ef488`)
- ← EkosConfig::default (`96f3be94-9761-5db0-b99a-0c705c9c55d0`)

### Contains

- ← ekos/crates/compiler-core/src/config.rs (`d0747f34-25e1-5426-80eb-a54fffcac598`)

## Diagram

```mermaid
graph TD
    ndb53c97e788e587ba5d4fbff7497f34e["ObserveConfig::default"]
    nd0747f3425e1542680eba54fffcac598["ekos/crates/compiler-core/src/config.rs"]
    nd0747f3425e1542680eba54fffcac598 -->|Contains| ndb53c97e788e587ba5d4fbff7497f34e
    n8365dc187a685b6cbb0bfcddf13ef488["default_ignore_patterns"]
    ndb53c97e788e587ba5d4fbff7497f34e -->|Calls| n8365dc187a685b6cbb0bfcddf13ef488
    n96f3be9497615db0b99a0c705c9c55d0["EkosConfig::default"]
    n96f3be9497615db0b99a0c705c9c55d0 -->|Calls| ndb53c97e788e587ba5d4fbff7497f34e
```

## Evidence

_No evidence cited._
