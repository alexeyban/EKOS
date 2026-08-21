# append_fragment (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← procedure_body_to_graph (`7ec44c96-a38e-5bf5-8c0f-3683d9b61662`)
- ← function_to_graph (`fb4a63c1-3690-5bee-b424-6e26e9a46525`)

### Contains

- ← ekos/crates/recovery/src/sql_transform_analyzer.rs (`987e3fbb-31ec-576d-b794-7e801336e8c8`)

## Diagram

```mermaid
graph TD
    nc9869d767bdc567da3ec83128f3ac4cd["append_fragment"]
    n987e3fbb31ec576db7947e801336e8c8["ekos/crates/recovery/src/sql_transform_analyzer.rs"]
    n987e3fbb31ec576db7947e801336e8c8 -->|Contains| nc9869d767bdc567da3ec83128f3ac4cd
    n7ec44c96a38e5bf58c0f3683d9b61662["procedure_body_to_graph"]
    n7ec44c96a38e5bf58c0f3683d9b61662 -->|Calls| nc9869d767bdc567da3ec83128f3ac4cd
    nfb4a63c136905beeb4246e26e9a46525["function_to_graph"]
    nfb4a63c136905beeb4246e26e9a46525 -->|Calls| nc9869d767bdc567da3ec83128f3ac4cd
```

## Evidence

_No evidence cited._
