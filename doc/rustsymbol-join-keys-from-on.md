# join_keys_from_on (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- → keyword_arg (`d64243b2-605e-5af2-9373-b2f6483c166a`)
- → string_constant (`de2abe1c-32b1-5b40-8fe7-3fcfe125a6e5`)
- ← calls_to_nodes (`73d9612b-4af4-5e0f-b10f-ddfc27015e27`)

### Contains

- ← ekos/crates/recovery/src/python_analyzer.rs (`196ca3fd-e85c-5ab9-a142-50f63dc586b9`)

## Diagram

```mermaid
graph TD
    n340e949592d156c4adb1a881b61f5e07["join_keys_from_on"]
    n196ca3fde85c5ab9a14250f63dc586b9["ekos/crates/recovery/src/python_analyzer.rs"]
    n196ca3fde85c5ab9a14250f63dc586b9 -->|Contains| n340e949592d156c4adb1a881b61f5e07
    nd64243b2605e5af29373b2f6483c166a["keyword_arg"]
    n340e949592d156c4adb1a881b61f5e07 -->|Calls| nd64243b2605e5af29373b2f6483c166a
    nde2abe1c32b15b408fe73fcfe125a6e5["string_constant"]
    n340e949592d156c4adb1a881b61f5e07 -->|Calls| nde2abe1c32b15b408fe73fcfe125a6e5
    n73d9612b4af45e0fb10fddfc27015e27["calls_to_nodes"]
    n73d9612b4af45e0fb10fddfc27015e27 -->|Calls| n340e949592d156c4adb1a881b61f5e07
```

## Evidence

_No evidence cited._
