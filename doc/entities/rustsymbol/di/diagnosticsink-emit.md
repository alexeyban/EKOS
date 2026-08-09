# DiagnosticSink::emit (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- ← DiagnosticSink::error (`e6093eea-c3f9-5894-b146-d449ee5b6f84`)
- ← DiagnosticSink::warning (`24645a2f-ef1f-5a55-b6fd-b94baf480fb0`)
- ← DiagnosticSink::info (`07782ac5-d687-54d6-b6b8-2aa02ac02499`)

### Contains

- ← ekos/crates/compiler-core/src/diagnostics.rs (`e5b5b2e0-3763-5cb8-a914-f113bb9e3ac4`)

## Diagram

```mermaid
graph TD
    nba8ffc803c115877bbd3a465cd305be4["DiagnosticSink::emit"]
    ne5b5b2e037635cb8a914f113bb9e3ac4["ekos/crates/compiler-core/src/diagnostics.rs"]
    ne5b5b2e037635cb8a914f113bb9e3ac4 -->|Contains| nba8ffc803c115877bbd3a465cd305be4
    ne6093eeac3f95894b146d449ee5b6f84["DiagnosticSink::error"]
    ne6093eeac3f95894b146d449ee5b6f84 -->|Calls| nba8ffc803c115877bbd3a465cd305be4
    n24645a2fef1f5a55b6fdb94baf480fb0["DiagnosticSink::warning"]
    n24645a2fef1f5a55b6fdb94baf480fb0 -->|Calls| nba8ffc803c115877bbd3a465cd305be4
    n07782ac5d68754d6b6b82aa02ac02499["DiagnosticSink::info"]
    n07782ac5d68754d6b6b82aa02ac02499 -->|Calls| nba8ffc803c115877bbd3a465cd305be4
```

## Evidence

_No evidence cited._
