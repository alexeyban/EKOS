# Parser::advance (RustSymbol)

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
- ← Parser::parse_op (`e087e932-24ce-558e-98f1-852fd478017f`)
- ← Parser::parse_literal (`bcca247d-bb0c-526d-a316-20cd7dbe0337`)

### Contains

- ← ekos/crates/ekl/src/parser.rs (`7eda2531-87c8-5048-92b2-c98483606431`)

## Diagram

```mermaid
graph TD
    ncf978e57771f5071bc4190610efbe4cd["Parser::advance"]
    n7eda253187c8504892b2c98483606431["ekos/crates/ekl/src/parser.rs"]
    n7eda253187c8504892b2c98483606431 -->|Contains| ncf978e57771f5071bc4190610efbe4cd
    n251e54ef3f2859549014ee9ae388c8ba["Parser::expect_keyword"]
    n251e54ef3f2859549014ee9ae388c8ba -->|Calls| ncf978e57771f5071bc4190610efbe4cd
    nb6c351aeb2685230a6df6a4cfd1a8f80["Parser::expect_ident"]
    nb6c351aeb2685230a6df6a4cfd1a8f80 -->|Calls| ncf978e57771f5071bc4190610efbe4cd
    n6e72e754f9b756b0acdf067abda4af8a["Parser::expect_string"]
    n6e72e754f9b756b0acdf067abda4af8a -->|Calls| ncf978e57771f5071bc4190610efbe4cd
    ncff299bab65d511392a4a72f46de71a1["Parser::expect_num"]
    ncff299bab65d511392a4a72f46de71a1 -->|Calls| ncf978e57771f5071bc4190610efbe4cd
    n4147b7b99d995be9b91f42752c6f6561["Parser::parse_query"]
    n4147b7b99d995be9b91f42752c6f6561 -->|Calls| ncf978e57771f5071bc4190610efbe4cd
    ne087e93224ce558e98f1852fd478017f["Parser::parse_op"]
    ne087e93224ce558e98f1852fd478017f -->|Calls| ncf978e57771f5071bc4190610efbe4cd
    nbcca247dbb0c526da31620cd7dbe0337["Parser::parse_literal"]
    nbcca247dbb0c526da31620cd7dbe0337 -->|Calls| ncf978e57771f5071bc4190610efbe4cd
```

## Evidence

_No evidence cited._
