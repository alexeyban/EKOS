# query_to_graph (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← dispatch_one_statement (`23b79c8c-1748-5c88-b609-df3f07bd4779`)
- → select_to_graph (`cb80b4b3-9be9-5aae-828e-f0d4f3ec4336`)
- ← procedure_body_to_graph (`7ec44c96-a38e-5bf5-8c0f-3683d9b61662`)
- ← function_to_graph (`fb4a63c1-3690-5bee-b424-6e26e9a46525`)

### Contains

- ← ekos/crates/recovery/src/sql_transform_analyzer.rs (`987e3fbb-31ec-576d-b794-7e801336e8c8`)

## Diagram

```mermaid
graph TD
    nf816648ea2ad50bdbd73494fdbbbad49["query_to_graph"]
    n987e3fbb31ec576db7947e801336e8c8["ekos/crates/recovery/src/sql_transform_analyzer.rs"]
    n987e3fbb31ec576db7947e801336e8c8 -->|Contains| nf816648ea2ad50bdbd73494fdbbbad49
    n23b79c8c17485c88b609df3f07bd4779["dispatch_one_statement"]
    n23b79c8c17485c88b609df3f07bd4779 -->|Calls| nf816648ea2ad50bdbd73494fdbbbad49
    ncb80b4b39be95aae828ef0d4f3ec4336["select_to_graph"]
    nf816648ea2ad50bdbd73494fdbbbad49 -->|Calls| ncb80b4b39be95aae828ef0d4f3ec4336
    n7ec44c96a38e5bf58c0f3683d9b61662["procedure_body_to_graph"]
    n7ec44c96a38e5bf58c0f3683d9b61662 -->|Calls| nf816648ea2ad50bdbd73494fdbbbad49
    nfb4a63c136905beeb4246e26e9a46525["function_to_graph"]
    nfb4a63c136905beeb4246e26e9a46525 -->|Calls| nf816648ea2ad50bdbd73494fdbbbad49
```

## Evidence

_No evidence cited._
