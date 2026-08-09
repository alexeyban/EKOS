# crate::llm::LlmRequest (RustModule)

## Properties

_No compiled properties._

## Relationships

### DependsOn

- ← ekos/crates/recovery/src/sql_analyzer.rs (`cd768c5e-1640-51c3-b6ec-cb7ac78ade6d`)
- ← ekos/crates/recovery/src/ollama.rs (`952b3eaf-406f-5c22-b538-f5c2d5fbe2f9`)
- ← ekos/crates/recovery/src/anthropic.rs (`2b1d458b-2cbb-5b8a-9932-9c15c981a99e`)
- ← ekos/crates/recovery/src/cache.rs (`0b06681a-4e07-5e02-a8d6-433ccf4aadc4`)
- ← ekos/crates/recovery/src/document_semantics_analyzer.rs (`62e92526-f096-5ed2-bc72-0bdae8703aa3`)

## Diagram

```mermaid
graph TD
    n7bbbda68c4575060baec126b283cb02f["crate::llm::LlmRequest"]
    ncd768c5e164051c3b6eccb7ac78ade6d["ekos/crates/recovery/src/sql_analyzer.rs"]
    ncd768c5e164051c3b6eccb7ac78ade6d -->|DependsOn| n7bbbda68c4575060baec126b283cb02f
    n952b3eaf406f5c22b538f5c2d5fbe2f9["ekos/crates/recovery/src/ollama.rs"]
    n952b3eaf406f5c22b538f5c2d5fbe2f9 -->|DependsOn| n7bbbda68c4575060baec126b283cb02f
    n2b1d458b2cbb5b8a99329c15c981a99e["ekos/crates/recovery/src/anthropic.rs"]
    n2b1d458b2cbb5b8a99329c15c981a99e -->|DependsOn| n7bbbda68c4575060baec126b283cb02f
    n0b06681a4e075e02a8d6433ccf4aadc4["ekos/crates/recovery/src/cache.rs"]
    n0b06681a4e075e02a8d6433ccf4aadc4 -->|DependsOn| n7bbbda68c4575060baec126b283cb02f
    n62e92526f0965ed2bc720bdae8703aa3["ekos/crates/recovery/src/document_semantics_analyzer.rs"]
    n62e92526f0965ed2bc720bdae8703aa3 -->|DependsOn| n7bbbda68c4575060baec126b283cb02f
```

## Evidence

_No evidence cited._
