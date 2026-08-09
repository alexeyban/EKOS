# chunk_text (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← TextParser::parse (`d8a4bbd2-9d47-53c2-8746-6e817d24d7f5`)
- → split_to_budget (`d4c45ee3-3c6e-5747-9943-a5b3df1108ee`)

### Contains

- ← ekos/plugins/localdocs/src/text.rs (`fb7cfbed-8381-5078-b9d6-bc8b68af4e7d`)

## Diagram

```mermaid
graph TD
    n2975a72f8664580389dde5ab5a6ad1e8["chunk_text"]
    nfb7cfbed83815078b9d6bc8b68af4e7d["ekos/plugins/localdocs/src/text.rs"]
    nfb7cfbed83815078b9d6bc8b68af4e7d -->|Contains| n2975a72f8664580389dde5ab5a6ad1e8
    nd8a4bbd29d4753c287466e817d24d7f5["TextParser::parse"]
    nd8a4bbd29d4753c287466e817d24d7f5 -->|Calls| n2975a72f8664580389dde5ab5a6ad1e8
    nd4c45ee33c6e57479943a5b3df1108ee["split_to_budget"]
    n2975a72f8664580389dde5ab5a6ad1e8 -->|Calls| nd4c45ee33c6e57479943a5b3df1108ee
```

## Evidence

_No evidence cited._
