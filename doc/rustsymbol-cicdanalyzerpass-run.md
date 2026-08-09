# CicdAnalyzerPass::run (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → pipeline_kir_id (`630ff87b-5416-525d-b949-e50b8c6ac173`)
- → extract_triggers (`be0f7bcd-58dd-5d24-8de9-6a2e0f2bc9ec`)
- → extract_jobs (`ffa32f72-651f-58b6-8100-f32b4d3997aa`)

### Contains

- ← ekos/crates/recovery/src/cicd_analyzer.rs (`b6532a21-993c-5d28-8d99-891c30d70063`)

## Diagram

```mermaid
graph TD
    n1f592577a349504ebf9b846299c8c7c8["CicdAnalyzerPass::run"]
    nb6532a21993c5d288d99891c30d70063["ekos/crates/recovery/src/cicd_analyzer.rs"]
    nb6532a21993c5d288d99891c30d70063 -->|Contains| n1f592577a349504ebf9b846299c8c7c8
    n630ff87b5416525db949e50b8c6ac173["pipeline_kir_id"]
    n1f592577a349504ebf9b846299c8c7c8 -->|Calls| n630ff87b5416525db949e50b8c6ac173
    nbe0f7bcd58dd5d248de96a2e0f2bc9ec["extract_triggers"]
    n1f592577a349504ebf9b846299c8c7c8 -->|Calls| nbe0f7bcd58dd5d248de96a2e0f2bc9ec
    nffa32f72651f58b68100f32b4d3997aa["extract_jobs"]
    n1f592577a349504ebf9b846299c8c7c8 -->|Calls| nffa32f72651f58b68100f32b4d3997aa
```

## Evidence

_No evidence cited._
