# EkosConfig::ekos_dir (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- ← EkosConfig::artifact_dir (`40a21a83-7f24-5951-9ce3-be90d0e89682`)
- ← EkosConfig::ledger_dir (`3356e338-00e2-5af6-ae58-8eab5122377b`)

### Contains

- ← ekos/crates/compiler-core/src/config.rs (`d0747f34-25e1-5426-80eb-a54fffcac598`)

## Diagram

```mermaid
graph TD
    n2ae501da572c53d984c5837ab91b23f9["EkosConfig::ekos_dir"]
    nd0747f3425e1542680eba54fffcac598["ekos/crates/compiler-core/src/config.rs"]
    nd0747f3425e1542680eba54fffcac598 -->|Contains| n2ae501da572c53d984c5837ab91b23f9
    n40a21a837f2459519ce3be90d0e89682["EkosConfig::artifact_dir"]
    n40a21a837f2459519ce3be90d0e89682 -->|Calls| n2ae501da572c53d984c5837ab91b23f9
    n3356e33800e25af6ae588eab5122377b["EkosConfig::ledger_dir"]
    n3356e33800e25af6ae588eab5122377b -->|Calls| n2ae501da572c53d984c5837ab91b23f9
```

## Evidence

_No evidence cited._
