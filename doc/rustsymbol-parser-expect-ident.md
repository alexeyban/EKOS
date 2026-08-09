# Parser::expect_ident (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → Parser::peek_pos (`e00f6daf-8ff4-5f41-9ac9-cdab8bb25782`)
- → Parser::advance (`cf978e57-771f-5071-bc41-90610efbe4cd`)
- ← Parser::parse_query (`4147b7b9-9d99-5be9-b91f-42752c6f6561`)
- ← Parser::parse_entity (`a27888f1-6c0b-53ed-a4a8-c95d9fd15c0d`)
- ← Parser::parse_predicate (`13dfbb5c-4862-500d-a859-066930201fb1`)

### Contains

- ← ekos/crates/ekl/src/parser.rs (`7eda2531-87c8-5048-92b2-c98483606431`)

## Diagram

```mermaid
graph TD
    nb6c351aeb2685230a6df6a4cfd1a8f80["Parser::expect_ident"]
    n7eda253187c8504892b2c98483606431["ekos/crates/ekl/src/parser.rs"]
    n7eda253187c8504892b2c98483606431 -->|Contains| nb6c351aeb2685230a6df6a4cfd1a8f80
    ne00f6daf8ff45f419ac9cdab8bb25782["Parser::peek_pos"]
    nb6c351aeb2685230a6df6a4cfd1a8f80 -->|Calls| ne00f6daf8ff45f419ac9cdab8bb25782
    ncf978e57771f5071bc4190610efbe4cd["Parser::advance"]
    nb6c351aeb2685230a6df6a4cfd1a8f80 -->|Calls| ncf978e57771f5071bc4190610efbe4cd
    n4147b7b99d995be9b91f42752c6f6561["Parser::parse_query"]
    n4147b7b99d995be9b91f42752c6f6561 -->|Calls| nb6c351aeb2685230a6df6a4cfd1a8f80
    na27888f16c0b53eda4a8c95d9fd15c0d["Parser::parse_entity"]
    na27888f16c0b53eda4a8c95d9fd15c0d -->|Calls| nb6c351aeb2685230a6df6a4cfd1a8f80
    n13dfbb5c4862500da859066930201fb1["Parser::parse_predicate"]
    n13dfbb5c4862500da859066930201fb1 -->|Calls| nb6c351aeb2685230a6df6a4cfd1a8f80
```

## Evidence

_No evidence cited._
