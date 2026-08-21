# Parser::peek_pos (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- ← Parser::expect_keyword (`251e54ef-3f28-5954-9014-ee9ae388c8ba`)
- ← Parser::expect_ident (`b6c351ae-b268-5230-a6df-6a4cfd1a8f80`)
- ← Parser::expect_string (`6e72e754-f9b7-56b0-acdf-067abda4af8a`)
- ← Parser::expect_num (`cff299ba-b65d-5113-92a4-a72f46de71a1`)
- ← Parser::parse_query (`4147b7b9-9d99-5be9-b91f-42752c6f6561`)
- ← Parser::parse_entity (`a27888f1-6c0b-53ed-a4a8-c95d9fd15c0d`)
- ← Parser::parse_op (`e087e932-24ce-558e-98f1-852fd478017f`)
- ← Parser::parse_literal (`bcca247d-bb0c-526d-a316-20cd7dbe0337`)

### Contains

- ← ekos/crates/ekl/src/parser.rs (`7eda2531-87c8-5048-92b2-c98483606431`)

## Diagram

```mermaid
graph TD
    ne00f6daf8ff45f419ac9cdab8bb25782["Parser::peek_pos"]
    n7eda253187c8504892b2c98483606431["ekos/crates/ekl/src/parser.rs"]
    n7eda253187c8504892b2c98483606431 -->|Contains| ne00f6daf8ff45f419ac9cdab8bb25782
    n251e54ef3f2859549014ee9ae388c8ba["Parser::expect_keyword"]
    n251e54ef3f2859549014ee9ae388c8ba -->|Calls| ne00f6daf8ff45f419ac9cdab8bb25782
    nb6c351aeb2685230a6df6a4cfd1a8f80["Parser::expect_ident"]
    nb6c351aeb2685230a6df6a4cfd1a8f80 -->|Calls| ne00f6daf8ff45f419ac9cdab8bb25782
    n6e72e754f9b756b0acdf067abda4af8a["Parser::expect_string"]
    n6e72e754f9b756b0acdf067abda4af8a -->|Calls| ne00f6daf8ff45f419ac9cdab8bb25782
    ncff299bab65d511392a4a72f46de71a1["Parser::expect_num"]
    ncff299bab65d511392a4a72f46de71a1 -->|Calls| ne00f6daf8ff45f419ac9cdab8bb25782
    n4147b7b99d995be9b91f42752c6f6561["Parser::parse_query"]
    n4147b7b99d995be9b91f42752c6f6561 -->|Calls| ne00f6daf8ff45f419ac9cdab8bb25782
    na27888f16c0b53eda4a8c95d9fd15c0d["Parser::parse_entity"]
    na27888f16c0b53eda4a8c95d9fd15c0d -->|Calls| ne00f6daf8ff45f419ac9cdab8bb25782
    ne087e93224ce558e98f1852fd478017f["Parser::parse_op"]
    ne087e93224ce558e98f1852fd478017f -->|Calls| ne00f6daf8ff45f419ac9cdab8bb25782
    nbcca247dbb0c526da31620cd7dbe0337["Parser::parse_literal"]
    nbcca247dbb0c526da31620cd7dbe0337 -->|Calls| ne00f6daf8ff45f419ac9cdab8bb25782
```

## Evidence

_No evidence cited._
