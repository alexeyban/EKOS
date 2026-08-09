# Parser::parse_op (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- ← Parser::parse_predicate (`13dfbb5c-4862-500d-a859-066930201fb1`)
- → Parser::peek_pos (`e00f6daf-8ff4-5f41-9ac9-cdab8bb25782`)
- → Parser::advance (`cf978e57-771f-5071-bc41-90610efbe4cd`)

### Contains

- ← ekos/crates/ekl/src/parser.rs (`7eda2531-87c8-5048-92b2-c98483606431`)

## Diagram

```mermaid
graph TD
    ne087e93224ce558e98f1852fd478017f["Parser::parse_op"]
    n7eda253187c8504892b2c98483606431["ekos/crates/ekl/src/parser.rs"]
    n7eda253187c8504892b2c98483606431 -->|Contains| ne087e93224ce558e98f1852fd478017f
    n13dfbb5c4862500da859066930201fb1["Parser::parse_predicate"]
    n13dfbb5c4862500da859066930201fb1 -->|Calls| ne087e93224ce558e98f1852fd478017f
    ne00f6daf8ff45f419ac9cdab8bb25782["Parser::peek_pos"]
    ne087e93224ce558e98f1852fd478017f -->|Calls| ne00f6daf8ff45f419ac9cdab8bb25782
    ncf978e57771f5071bc4190610efbe4cd["Parser::advance"]
    ne087e93224ce558e98f1852fd478017f -->|Calls| ncf978e57771f5071bc4190610efbe4cd
```

## Evidence

_No evidence cited._
