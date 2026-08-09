# northwind_pipeline_end_to_end (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- → run_pipeline (`6bb7df90-2b21-50fa-9dc1-ddde4307ef92`)
- → fixtures_dir (`5fe11438-f684-5c87-b602-8ed880979203`)

### Contains

- ← tests/integration/tests/integration.rs (`23e9eb43-c8df-52b5-a581-f2efba7085ea`)

## Diagram

```mermaid
graph TD
    n2cb9f53ef55a510fab721dcea04bc856["northwind_pipeline_end_to_end"]
    n23e9eb43c8df52b5a581f2efba7085ea["tests/integration/tests/integration.rs"]
    n23e9eb43c8df52b5a581f2efba7085ea -->|Contains| n2cb9f53ef55a510fab721dcea04bc856
    n6bb7df902b2150fa9dc1ddde4307ef92["run_pipeline"]
    n2cb9f53ef55a510fab721dcea04bc856 -->|Calls| n6bb7df902b2150fa9dc1ddde4307ef92
    n5fe11438f6845c87b6028ed880979203["fixtures_dir"]
    n2cb9f53ef55a510fab721dcea04bc856 -->|Calls| n5fe11438f6845c87b6028ed880979203
```

## Evidence

_No evidence cited._
