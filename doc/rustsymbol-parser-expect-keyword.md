# Parser::expect_keyword (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → Parser::peek_pos (`e00f6daf-8ff4-5f41-9ac9-cdab8bb25782`)
- → Parser::advance (`cf978e57-771f-5071-bc41-90610efbe4cd`)
- ← Parser::parse_query (`4147b7b9-9d99-5be9-b91f-42752c6f6561`)

### Contains

- ← ekos/crates/ekl/src/parser.rs (`7eda2531-87c8-5048-92b2-c98483606431`)

## Diagram

```mermaid
graph TD
    n251e54ef3f2859549014ee9ae388c8ba["Parser::expect_keyword"]
    n7eda253187c8504892b2c98483606431["ekos/crates/ekl/src/parser.rs"]
    n7eda253187c8504892b2c98483606431 -->|Contains| n251e54ef3f2859549014ee9ae388c8ba
    ne00f6daf8ff45f419ac9cdab8bb25782["Parser::peek_pos"]
    n251e54ef3f2859549014ee9ae388c8ba -->|Calls| ne00f6daf8ff45f419ac9cdab8bb25782
    ncf978e57771f5071bc4190610efbe4cd["Parser::advance"]
    n251e54ef3f2859549014ee9ae388c8ba -->|Calls| ncf978e57771f5071bc4190610efbe4cd
    n4147b7b99d995be9b91f42752c6f6561["Parser::parse_query"]
    n4147b7b99d995be9b91f42752c6f6561 -->|Calls| n251e54ef3f2859549014ee9ae388c8ba
```

## Evidence

_No evidence cited._
