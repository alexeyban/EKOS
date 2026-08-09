# Parser::parse_predicate (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- ← Parser::parse_query (`4147b7b9-9d99-5be9-b91f-42752c6f6561`)
- → Parser::expect_ident (`b6c351ae-b268-5230-a6df-6a4cfd1a8f80`)
- → Parser::parse_literal (`bcca247d-bb0c-526d-a316-20cd7dbe0337`)
- → Parser::parse_op (`e087e932-24ce-558e-98f1-852fd478017f`)

### Contains

- ← ekos/crates/ekl/src/parser.rs (`7eda2531-87c8-5048-92b2-c98483606431`)

## Diagram

```mermaid
graph TD
    n13dfbb5c4862500da859066930201fb1["Parser::parse_predicate"]
    n7eda253187c8504892b2c98483606431["ekos/crates/ekl/src/parser.rs"]
    n7eda253187c8504892b2c98483606431 -->|Contains| n13dfbb5c4862500da859066930201fb1
    n4147b7b99d995be9b91f42752c6f6561["Parser::parse_query"]
    n4147b7b99d995be9b91f42752c6f6561 -->|Calls| n13dfbb5c4862500da859066930201fb1
    nb6c351aeb2685230a6df6a4cfd1a8f80["Parser::expect_ident"]
    n13dfbb5c4862500da859066930201fb1 -->|Calls| nb6c351aeb2685230a6df6a4cfd1a8f80
    nbcca247dbb0c526da31620cd7dbe0337["Parser::parse_literal"]
    n13dfbb5c4862500da859066930201fb1 -->|Calls| nbcca247dbb0c526da31620cd7dbe0337
    ne087e93224ce558e98f1852fd478017f["Parser::parse_op"]
    n13dfbb5c4862500da859066930201fb1 -->|Calls| ne087e93224ce558e98f1852fd478017f
```

## Evidence

_No evidence cited._
