# parse_python_file (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← PythonAnalyzerPass::run (`b3870c36-783f-55a9-88e9-eb68c0023f88`)
- → walk_top_level_statement (`36a795d0-3788-5433-a369-2571af84e340`)

### Contains

- ← ekos/crates/recovery/src/python_analyzer.rs (`196ca3fd-e85c-5ab9-a142-50f63dc586b9`)

## Diagram

```mermaid
graph TD
    n72ff02a8b7ba5481b85dc5b408ab50e4["parse_python_file"]
    n196ca3fde85c5ab9a14250f63dc586b9["ekos/crates/recovery/src/python_analyzer.rs"]
    n196ca3fde85c5ab9a14250f63dc586b9 -->|Contains| n72ff02a8b7ba5481b85dc5b408ab50e4
    nb3870c36783f55a988e9eb68c0023f88["PythonAnalyzerPass::run"]
    nb3870c36783f55a988e9eb68c0023f88 -->|Calls| n72ff02a8b7ba5481b85dc5b408ab50e4
    n36a795d037885433a3692571af84e340["walk_top_level_statement"]
    n72ff02a8b7ba5481b85dc5b408ab50e4 -->|Calls| n36a795d037885433a3692571af84e340
```

## Evidence

_No evidence cited._
