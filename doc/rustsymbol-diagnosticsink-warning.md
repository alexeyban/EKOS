# DiagnosticSink::warning (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → DiagnosticSink::emit (`ba8ffc80-3c11-5877-bbd3-a465cd305be4`)
- → Diagnostic::warning (`ef370595-8fee-58e2-9dfb-a62ab3481d89`)

### Contains

- ← ekos/crates/compiler-core/src/diagnostics.rs (`e5b5b2e0-3763-5cb8-a914-f113bb9e3ac4`)

## Diagram

```mermaid
graph TD
    n24645a2fef1f5a55b6fdb94baf480fb0["DiagnosticSink::warning"]
    ne5b5b2e037635cb8a914f113bb9e3ac4["ekos/crates/compiler-core/src/diagnostics.rs"]
    ne5b5b2e037635cb8a914f113bb9e3ac4 -->|Contains| n24645a2fef1f5a55b6fdb94baf480fb0
    nba8ffc803c115877bbd3a465cd305be4["DiagnosticSink::emit"]
    n24645a2fef1f5a55b6fdb94baf480fb0 -->|Calls| nba8ffc803c115877bbd3a465cd305be4
    nef3705958fee58e29dfba62ab3481d89["Diagnostic::warning"]
    n24645a2fef1f5a55b6fdb94baf480fb0 -->|Calls| nef3705958fee58e29dfba62ab3481d89
```

## Evidence

_No evidence cited._
