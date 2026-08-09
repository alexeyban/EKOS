# Parser::parse_entity (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- ← Parser::parse_query (`4147b7b9-9d99-5be9-b91f-42752c6f6561`)
- → Parser::expect_ident (`b6c351ae-b268-5230-a6df-6a4cfd1a8f80`)
- → Parser::peek_pos (`e00f6daf-8ff4-5f41-9ac9-cdab8bb25782`)

### Contains

- ← ekos/crates/ekl/src/parser.rs (`7eda2531-87c8-5048-92b2-c98483606431`)

## Diagram

```mermaid
graph TD
    na27888f16c0b53eda4a8c95d9fd15c0d["Parser::parse_entity"]
    n7eda253187c8504892b2c98483606431["ekos/crates/ekl/src/parser.rs"]
    n7eda253187c8504892b2c98483606431 -->|Contains| na27888f16c0b53eda4a8c95d9fd15c0d
    n4147b7b99d995be9b91f42752c6f6561["Parser::parse_query"]
    n4147b7b99d995be9b91f42752c6f6561 -->|Calls| na27888f16c0b53eda4a8c95d9fd15c0d
    nb6c351aeb2685230a6df6a4cfd1a8f80["Parser::expect_ident"]
    na27888f16c0b53eda4a8c95d9fd15c0d -->|Calls| nb6c351aeb2685230a6df6a4cfd1a8f80
    ne00f6daf8ff45f419ac9cdab8bb25782["Parser::peek_pos"]
    na27888f16c0b53eda4a8c95d9fd15c0d -->|Calls| ne00f6daf8ff45f419ac9cdab8bb25782
```

## Evidence

_No evidence cited._
