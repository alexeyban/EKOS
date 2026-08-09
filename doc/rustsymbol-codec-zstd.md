# Codec::zstd (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- ← Ledger::open (`1202f2b1-c8ed-5a89-aac3-5ef29891cb8b`)
- ← Ledger::create_v2 (`e0a15224-8267-58c6-9f12-b6f33a379ceb`)

### Contains

- ← ekos/crates/ledger/src/lib.rs (`21938f45-b767-5933-9e71-12e15ff53eb1`)

## Diagram

```mermaid
graph TD
    n90b5c6e9ab205f91af23dfa3538059e2["Codec::zstd"]
    n21938f45b76759339e7112e15ff53eb1["ekos/crates/ledger/src/lib.rs"]
    n21938f45b76759339e7112e15ff53eb1 -->|Contains| n90b5c6e9ab205f91af23dfa3538059e2
    n1202f2b1c8ed5a89aac35ef29891cb8b["Ledger::open"]
    n1202f2b1c8ed5a89aac35ef29891cb8b -->|Calls| n90b5c6e9ab205f91af23dfa3538059e2
    ne0a15224826758c69f12b6f33a379ceb["Ledger::create_v2"]
    ne0a15224826758c69f12b6f33a379ceb -->|Calls| n90b5c6e9ab205f91af23dfa3538059e2
```

## Evidence

_No evidence cited._
