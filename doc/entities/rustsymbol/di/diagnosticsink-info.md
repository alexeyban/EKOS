# DiagnosticSink::info (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → Diagnostic::info (`50b0b84a-7b74-5f94-8378-42ae38261000`)
- → DiagnosticSink::emit (`ba8ffc80-3c11-5877-bbd3-a465cd305be4`)

### Contains

- ← ekos/crates/compiler-core/src/diagnostics.rs (`e5b5b2e0-3763-5cb8-a914-f113bb9e3ac4`)

## Diagram

```mermaid
graph TD
    n07782ac5d68754d6b6b82aa02ac02499["DiagnosticSink::info"]
    ne5b5b2e037635cb8a914f113bb9e3ac4["ekos/crates/compiler-core/src/diagnostics.rs"]
    ne5b5b2e037635cb8a914f113bb9e3ac4 -->|Contains| n07782ac5d68754d6b6b82aa02ac02499
    n50b0b84a7b745f94837842ae38261000["Diagnostic::info"]
    n07782ac5d68754d6b6b82aa02ac02499 -->|Calls| n50b0b84a7b745f94837842ae38261000
    nba8ffc803c115877bbd3a465cd305be4["DiagnosticSink::emit"]
    n07782ac5d68754d6b6b82aa02ac02499 -->|Calls| nba8ffc803c115877bbd3a465cd305be4
```

## Evidence

_No evidence cited._
