# function_to_graph (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← dispatch_one_statement (`23b79c8c-1748-5c88-b609-df3f07bd4779`)
- → query_to_graph (`f816648e-a2ad-50bd-bd73-494fdbbbad49`)
- → append_fragment (`c9869d76-7bdc-567d-a3ec-83128f3ac4cd`)
- → function_body_text (`0ac287c3-3b30-59c8-870a-4ba97857d319`)
- → push (`cc223cbc-9afd-5086-9f88-8c6d484895d1`)

### Contains

- ← ekos/crates/recovery/src/sql_transform_analyzer.rs (`987e3fbb-31ec-576d-b794-7e801336e8c8`)

## Diagram

```mermaid
graph TD
    nfb4a63c136905beeb4246e26e9a46525["function_to_graph"]
    n987e3fbb31ec576db7947e801336e8c8["ekos/crates/recovery/src/sql_transform_analyzer.rs"]
    n987e3fbb31ec576db7947e801336e8c8 -->|Contains| nfb4a63c136905beeb4246e26e9a46525
    n23b79c8c17485c88b609df3f07bd4779["dispatch_one_statement"]
    n23b79c8c17485c88b609df3f07bd4779 -->|Calls| nfb4a63c136905beeb4246e26e9a46525
    nf816648ea2ad50bdbd73494fdbbbad49["query_to_graph"]
    nfb4a63c136905beeb4246e26e9a46525 -->|Calls| nf816648ea2ad50bdbd73494fdbbbad49
    nc9869d767bdc567da3ec83128f3ac4cd["append_fragment"]
    nfb4a63c136905beeb4246e26e9a46525 -->|Calls| nc9869d767bdc567da3ec83128f3ac4cd
    n0ac287c33b3059c8870a4ba97857d319["function_body_text"]
    nfb4a63c136905beeb4246e26e9a46525 -->|Calls| n0ac287c33b3059c8870a4ba97857d319
    ncc223cbc9afd50869f888c6d484895d1["push"]
    nfb4a63c136905beeb4246e26e9a46525 -->|Calls| ncc223cbc9afd50869f888c6d484895d1
```

## Evidence

_No evidence cited._
