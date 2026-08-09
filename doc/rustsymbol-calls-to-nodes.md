# calls_to_nodes (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← try_recognize_chain_statement (`1c28e117-91f6-543a-9528-4a070d7a4528`)
- → source_slice (`2af471f9-745a-5e89-9438-e13d5973a397`)
- → join_keys_from_on (`340e9495-92d1-56c4-adb1-a881b61f5e07`)
- → positional_string_arg (`c82e0ed6-c517-5eb4-8987-4cf896e0b597`)
- → join_kind_from_how (`50c6eef6-d1fe-5900-af7a-31a4ef604197`)

### Contains

- ← ekos/crates/recovery/src/python_analyzer.rs (`196ca3fd-e85c-5ab9-a142-50f63dc586b9`)

## Diagram

```mermaid
graph TD
    n73d9612b4af45e0fb10fddfc27015e27["calls_to_nodes"]
    n196ca3fde85c5ab9a14250f63dc586b9["ekos/crates/recovery/src/python_analyzer.rs"]
    n196ca3fde85c5ab9a14250f63dc586b9 -->|Contains| n73d9612b4af45e0fb10fddfc27015e27
    n1c28e11791f6543a95284a070d7a4528["try_recognize_chain_statement"]
    n1c28e11791f6543a95284a070d7a4528 -->|Calls| n73d9612b4af45e0fb10fddfc27015e27
    n2af471f9745a5e899438e13d5973a397["source_slice"]
    n73d9612b4af45e0fb10fddfc27015e27 -->|Calls| n2af471f9745a5e899438e13d5973a397
    n340e949592d156c4adb1a881b61f5e07["join_keys_from_on"]
    n73d9612b4af45e0fb10fddfc27015e27 -->|Calls| n340e949592d156c4adb1a881b61f5e07
    nc82e0ed6c5175eb489874cf896e0b597["positional_string_arg"]
    n73d9612b4af45e0fb10fddfc27015e27 -->|Calls| nc82e0ed6c5175eb489874cf896e0b597
    n50c6eef6d1fe5900af7a31a4ef604197["join_kind_from_how"]
    n73d9612b4af45e0fb10fddfc27015e27 -->|Calls| n50c6eef6d1fe5900af7a31a4ef604197
```

## Evidence

_No evidence cited._
