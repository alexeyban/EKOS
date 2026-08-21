# AiRuntime::ask (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → AiRuntime::gather_context (`e0e5f206-042c-5a81-8e86-1b9fbc4ab976`)
- → extract_citations (`8512d06f-0c17-5396-a99f-c7ab9a8705d4`)

### Contains

- ← ekos/crates/runtime/src/ai.rs (`e85e734d-ef58-5185-835a-34896d2da3f1`)

## Diagram

```mermaid
graph TD
    nd454db52b9775cf88af96f5559a47666["AiRuntime::ask"]
    ne85e734def585185835a34896d2da3f1["ekos/crates/runtime/src/ai.rs"]
    ne85e734def585185835a34896d2da3f1 -->|Contains| nd454db52b9775cf88af96f5559a47666
    ne0e5f206042c5a818e861b9fbc4ab976["AiRuntime::gather_context"]
    nd454db52b9775cf88af96f5559a47666 -->|Calls| ne0e5f206042c5a818e861b9fbc4ab976
    n8512d06f0c175396a99fc7ab9a8705d4["extract_citations"]
    nd454db52b9775cf88af96f5559a47666 -->|Calls| n8512d06f0c175396a99fc7ab9a8705d4
```

## Evidence

_No evidence cited._
