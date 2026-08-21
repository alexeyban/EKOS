# CryptoAnalyzerPass::run (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → parse_attrs (`5fedc248-e713-5749-865e-909f63819859`)
- → deterministic_id (`461c25e4-b57b-54cf-b1db-8a03eb01bbd5`)

### Contains

- ← ekos/crates/recovery/src/crypto_analyzer.rs (`c5652a0f-42a3-5e1c-82d4-0cf4d37fab34`)

## Diagram

```mermaid
graph TD
    n83cf6d5880b454fa8f5a26a8cba46481["CryptoAnalyzerPass::run"]
    nc5652a0f42a35e1c82d40cf4d37fab34["ekos/crates/recovery/src/crypto_analyzer.rs"]
    nc5652a0f42a35e1c82d40cf4d37fab34 -->|Contains| n83cf6d5880b454fa8f5a26a8cba46481
    n5fedc248e7135749865e909f63819859["parse_attrs"]
    n83cf6d5880b454fa8f5a26a8cba46481 -->|Calls| n5fedc248e7135749865e909f63819859
    n461c25e4b57b54cfb1db8a03eb01bbd5["deterministic_id"]
    n83cf6d5880b454fa8f5a26a8cba46481 -->|Calls| n461c25e4b57b54cfb1db8a03eb01bbd5
```

## Evidence

_No evidence cited._
