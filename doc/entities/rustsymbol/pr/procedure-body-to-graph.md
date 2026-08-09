# procedure_body_to_graph (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← dispatch_one_statement (`23b79c8c-1748-5c88-b609-df3f07bd4779`)
- → query_to_graph (`f816648e-a2ad-50bd-bd73-494fdbbbad49`)
- → append_fragment (`c9869d76-7bdc-567d-a3ec-83128f3ac4cd`)
- → push (`cc223cbc-9afd-5086-9f88-8c6d484895d1`)

### Contains

- ← ekos/crates/recovery/src/sql_transform_analyzer.rs (`987e3fbb-31ec-576d-b794-7e801336e8c8`)

## Diagram

```mermaid
graph TD
    n7ec44c96a38e5bf58c0f3683d9b61662["procedure_body_to_graph"]
    n987e3fbb31ec576db7947e801336e8c8["ekos/crates/recovery/src/sql_transform_analyzer.rs"]
    n987e3fbb31ec576db7947e801336e8c8 -->|Contains| n7ec44c96a38e5bf58c0f3683d9b61662
    n23b79c8c17485c88b609df3f07bd4779["dispatch_one_statement"]
    n23b79c8c17485c88b609df3f07bd4779 -->|Calls| n7ec44c96a38e5bf58c0f3683d9b61662
    nf816648ea2ad50bdbd73494fdbbbad49["query_to_graph"]
    n7ec44c96a38e5bf58c0f3683d9b61662 -->|Calls| nf816648ea2ad50bdbd73494fdbbbad49
    nc9869d767bdc567da3ec83128f3ac4cd["append_fragment"]
    n7ec44c96a38e5bf58c0f3683d9b61662 -->|Calls| nc9869d767bdc567da3ec83128f3ac4cd
    ncc223cbc9afd50869f888c6d484895d1["push"]
    n7ec44c96a38e5bf58c0f3683d9b61662 -->|Calls| ncc223cbc9afd50869f888c6d484895d1
```

## Evidence

_No evidence cited._
