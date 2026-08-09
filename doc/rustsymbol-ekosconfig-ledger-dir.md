# EkosConfig::ledger_dir (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → EkosConfig::ekos_dir (`2ae501da-572c-53d9-84c5-837ab91b23f9`)
- ← EkosConfig::ledger_path (`07b84559-17ba-5a9b-9464-d009a63af093`)
- ← EkosConfig::branch_ledger_path (`b05f3338-a437-555a-a050-36a4731498a1`)

### Contains

- ← ekos/crates/compiler-core/src/config.rs (`d0747f34-25e1-5426-80eb-a54fffcac598`)

## Diagram

```mermaid
graph TD
    n3356e33800e25af6ae588eab5122377b["EkosConfig::ledger_dir"]
    nd0747f3425e1542680eba54fffcac598["ekos/crates/compiler-core/src/config.rs"]
    nd0747f3425e1542680eba54fffcac598 -->|Contains| n3356e33800e25af6ae588eab5122377b
    n2ae501da572c53d984c5837ab91b23f9["EkosConfig::ekos_dir"]
    n3356e33800e25af6ae588eab5122377b -->|Calls| n2ae501da572c53d984c5837ab91b23f9
    n07b8455917ba5a9b9464d009a63af093["EkosConfig::ledger_path"]
    n07b8455917ba5a9b9464d009a63af093 -->|Calls| n3356e33800e25af6ae588eab5122377b
    nb05f3338a437555aa05036a4731498a1["EkosConfig::branch_ledger_path"]
    nb05f3338a437555aa05036a4731498a1 -->|Calls| n3356e33800e25af6ae588eab5122377b
```

## Evidence

_No evidence cited._
