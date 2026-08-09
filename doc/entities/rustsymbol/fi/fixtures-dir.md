# fixtures_dir (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← ecommerce_pipeline_end_to_end (`a57c4f64-6fb8-58be-aa29-7c411ae6f5ad`)
- ← northwind_pipeline_end_to_end (`2cb9f53e-f55a-510f-ab72-1dcea04bc856`)
- ← odoo_git_fixture_pipeline_end_to_end (`35626c56-43f6-547a-8944-e9bd07e4852a`)

### Contains

- ← tests/integration/tests/integration.rs (`23e9eb43-c8df-52b5-a581-f2efba7085ea`)

## Diagram

```mermaid
graph TD
    n5fe11438f6845c87b6028ed880979203["fixtures_dir"]
    n23e9eb43c8df52b5a581f2efba7085ea["tests/integration/tests/integration.rs"]
    n23e9eb43c8df52b5a581f2efba7085ea -->|Contains| n5fe11438f6845c87b6028ed880979203
    na57c4f646fb858beaa297c411ae6f5ad["ecommerce_pipeline_end_to_end"]
    na57c4f646fb858beaa297c411ae6f5ad -->|Calls| n5fe11438f6845c87b6028ed880979203
    n2cb9f53ef55a510fab721dcea04bc856["northwind_pipeline_end_to_end"]
    n2cb9f53ef55a510fab721dcea04bc856 -->|Calls| n5fe11438f6845c87b6028ed880979203
    n35626c5643f6547a8944e9bd07e4852a["odoo_git_fixture_pipeline_end_to_end"]
    n35626c5643f6547a8944e9bd07e4852a -->|Calls| n5fe11438f6845c87b6028ed880979203
```

## Evidence

_No evidence cited._
