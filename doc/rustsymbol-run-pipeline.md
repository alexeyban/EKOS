# run_pipeline (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← ecommerce_pipeline_end_to_end (`a57c4f64-6fb8-58be-aa29-7c411ae6f5ad`)
- ← northwind_pipeline_end_to_end (`2cb9f53e-f55a-510f-ab72-1dcea04bc856`)

### Contains

- ← tests/integration/tests/integration.rs (`23e9eb43-c8df-52b5-a581-f2efba7085ea`)

## Diagram

```mermaid
graph TD
    n6bb7df902b2150fa9dc1ddde4307ef92["run_pipeline"]
    n23e9eb43c8df52b5a581f2efba7085ea["tests/integration/tests/integration.rs"]
    n23e9eb43c8df52b5a581f2efba7085ea -->|Contains| n6bb7df902b2150fa9dc1ddde4307ef92
    na57c4f646fb858beaa297c411ae6f5ad["ecommerce_pipeline_end_to_end"]
    na57c4f646fb858beaa297c411ae6f5ad -->|Calls| n6bb7df902b2150fa9dc1ddde4307ef92
    n2cb9f53ef55a510fab721dcea04bc856["northwind_pipeline_end_to_end"]
    n2cb9f53ef55a510fab721dcea04bc856 -->|Calls| n6bb7df902b2150fa9dc1ddde4307ef92
```

## Evidence

_No evidence cited._
