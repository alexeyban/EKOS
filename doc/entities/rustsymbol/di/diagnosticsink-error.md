# DiagnosticSink::error (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → DiagnosticSink::emit (`ba8ffc80-3c11-5877-bbd3-a465cd305be4`)
- → Diagnostic::error (`b50ccff7-7377-5f32-a71c-c1aa3f5895db`)

### Contains

- ← ekos/crates/compiler-core/src/diagnostics.rs (`e5b5b2e0-3763-5cb8-a914-f113bb9e3ac4`)

## Diagram

```mermaid
graph TD
    ne6093eeac3f95894b146d449ee5b6f84["DiagnosticSink::error"]
    ne5b5b2e037635cb8a914f113bb9e3ac4["ekos/crates/compiler-core/src/diagnostics.rs"]
    ne5b5b2e037635cb8a914f113bb9e3ac4 -->|Contains| ne6093eeac3f95894b146d449ee5b6f84
    nba8ffc803c115877bbd3a465cd305be4["DiagnosticSink::emit"]
    ne6093eeac3f95894b146d449ee5b6f84 -->|Calls| nba8ffc803c115877bbd3a465cd305be4
    nb50ccff773775f32a71cc1aa3f5895db["Diagnostic::error"]
    ne6093eeac3f95894b146d449ee5b6f84 -->|Calls| nb50ccff773775f32a71cc1aa3f5895db
```

## Evidence

_No evidence cited._
