# Parser::parse_literal (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- ← Parser::parse_predicate (`13dfbb5c-4862-500d-a859-066930201fb1`)
- → Parser::advance (`cf978e57-771f-5071-bc41-90610efbe4cd`)
- → Parser::peek_pos (`e00f6daf-8ff4-5f41-9ac9-cdab8bb25782`)

### Contains

- ← ekos/crates/ekl/src/parser.rs (`7eda2531-87c8-5048-92b2-c98483606431`)

## Diagram

```mermaid
graph TD
    nbcca247dbb0c526da31620cd7dbe0337["Parser::parse_literal"]
    n7eda253187c8504892b2c98483606431["ekos/crates/ekl/src/parser.rs"]
    n7eda253187c8504892b2c98483606431 -->|Contains| nbcca247dbb0c526da31620cd7dbe0337
    n13dfbb5c4862500da859066930201fb1["Parser::parse_predicate"]
    n13dfbb5c4862500da859066930201fb1 -->|Calls| nbcca247dbb0c526da31620cd7dbe0337
    ncf978e57771f5071bc4190610efbe4cd["Parser::advance"]
    nbcca247dbb0c526da31620cd7dbe0337 -->|Calls| ncf978e57771f5071bc4190610efbe4cd
    ne00f6daf8ff45f419ac9cdab8bb25782["Parser::peek_pos"]
    nbcca247dbb0c526da31620cd7dbe0337 -->|Calls| ne00f6daf8ff45f419ac9cdab8bb25782
```

## Evidence

_No evidence cited._
