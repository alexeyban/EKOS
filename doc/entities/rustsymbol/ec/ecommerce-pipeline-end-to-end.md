# ecommerce_pipeline_end_to_end (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- → copy_dir (`7496161f-1e51-5c7c-bb5a-03d64200bb13`)
- → fixtures_dir (`5fe11438-f684-5c87-b602-8ed880979203`)
- → run_pipeline (`6bb7df90-2b21-50fa-9dc1-ddde4307ef92`)

### Contains

- ← tests/integration/tests/integration.rs (`23e9eb43-c8df-52b5-a581-f2efba7085ea`)

## Diagram

```mermaid
graph TD
    na57c4f646fb858beaa297c411ae6f5ad["ecommerce_pipeline_end_to_end"]
    n23e9eb43c8df52b5a581f2efba7085ea["tests/integration/tests/integration.rs"]
    n23e9eb43c8df52b5a581f2efba7085ea -->|Contains| na57c4f646fb858beaa297c411ae6f5ad
    n7496161f1e515c7cbb5a03d64200bb13["copy_dir"]
    na57c4f646fb858beaa297c411ae6f5ad -->|Calls| n7496161f1e515c7cbb5a03d64200bb13
    n5fe11438f6845c87b6028ed880979203["fixtures_dir"]
    na57c4f646fb858beaa297c411ae6f5ad -->|Calls| n5fe11438f6845c87b6028ed880979203
    n6bb7df902b2150fa9dc1ddde4307ef92["run_pipeline"]
    na57c4f646fb858beaa297c411ae6f5ad -->|Calls| n6bb7df902b2150fa9dc1ddde4307ef92
```

## Evidence

_No evidence cited._
