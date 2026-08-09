# ekl_parse (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- → Lexer::new (`719d674a-ff2f-560b-8cce-a9458c019026`)
- → Parser::new (`7d33f28a-3f46-59ed-b13d-496c9f39216d`)
- → Parser::parse_query (`4147b7b9-9d99-5be9-b91f-42752c6f6561`)
- → Lexer::tokenize (`75766aca-22b9-59f6-aa0b-7e6588ae0af9`)

### Contains

- ← ekos/crates/ekl/src/parser.rs (`7eda2531-87c8-5048-92b2-c98483606431`)

## Diagram

```mermaid
graph TD
    n590311ca281e528aae356f89aceae476["ekl_parse"]
    n7eda253187c8504892b2c98483606431["ekos/crates/ekl/src/parser.rs"]
    n7eda253187c8504892b2c98483606431 -->|Contains| n590311ca281e528aae356f89aceae476
    n719d674aff2f560b8ccea9458c019026["Lexer::new"]
    n590311ca281e528aae356f89aceae476 -->|Calls| n719d674aff2f560b8ccea9458c019026
    n7d33f28a3f4659edb13d496c9f39216d["Parser::new"]
    n590311ca281e528aae356f89aceae476 -->|Calls| n7d33f28a3f4659edb13d496c9f39216d
    n4147b7b99d995be9b91f42752c6f6561["Parser::parse_query"]
    n590311ca281e528aae356f89aceae476 -->|Calls| n4147b7b99d995be9b91f42752c6f6561
    n75766aca22b959f6aa0b7e6588ae0af9["Lexer::tokenize"]
    n590311ca281e528aae356f89aceae476 -->|Calls| n75766aca22b959f6aa0b7e6588ae0af9
```

## Evidence

_No evidence cited._
