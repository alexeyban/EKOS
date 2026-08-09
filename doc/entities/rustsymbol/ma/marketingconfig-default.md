# MarketingConfig::default (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → default_github (`81f7cad3-3ade-55cb-a07f-9d2c85cbcc12`)
- → default_hashtags (`88f766d2-9693-5579-8cf3-a2624f967bc2`)
- ← EkosConfig::default (`96f3be94-9761-5db0-b99a-0c705c9c55d0`)

### Contains

- ← ekos/crates/compiler-core/src/config.rs (`d0747f34-25e1-5426-80eb-a54fffcac598`)

## Diagram

```mermaid
graph TD
    n3fa1fe515c40500ea4afe9b3dfeb907f["MarketingConfig::default"]
    nd0747f3425e1542680eba54fffcac598["ekos/crates/compiler-core/src/config.rs"]
    nd0747f3425e1542680eba54fffcac598 -->|Contains| n3fa1fe515c40500ea4afe9b3dfeb907f
    n81f7cad33ade55cba07f9d2c85cbcc12["default_github"]
    n3fa1fe515c40500ea4afe9b3dfeb907f -->|Calls| n81f7cad33ade55cba07f9d2c85cbcc12
    n88f766d2969355798cf3a2624f967bc2["default_hashtags"]
    n3fa1fe515c40500ea4afe9b3dfeb907f -->|Calls| n88f766d2969355798cf3a2624f967bc2
    n96f3be9497615db0b99a0c705c9c55d0["EkosConfig::default"]
    n96f3be9497615db0b99a0c705c9c55d0 -->|Calls| n3fa1fe515c40500ea4afe9b3dfeb907f
```

## Evidence

_No evidence cited._
