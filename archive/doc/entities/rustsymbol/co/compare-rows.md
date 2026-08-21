# compare_rows (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← EklInterpreter::execute (`feb95d3d-5916-525d-86e3-ad4cee4ff906`)
- → value_to_string (`2b431c16-e299-5ca5-b60a-18aac4ca949f`)

### Contains

- ← ekos/crates/ekl/src/interpreter.rs (`9c2cb6e4-ee09-503f-8cf5-ccfaf23ecd79`)

## Diagram

```mermaid
graph TD
    n9166302681cb5c5e912c7ffe203d4ed6["compare_rows"]
    n9c2cb6e4ee09503f8cf5ccfaf23ecd79["ekos/crates/ekl/src/interpreter.rs"]
    n9c2cb6e4ee09503f8cf5ccfaf23ecd79 -->|Contains| n9166302681cb5c5e912c7ffe203d4ed6
    nfeb95d3d5916525d86e3ad4cee4ff906["EklInterpreter::execute"]
    nfeb95d3d5916525d86e3ad4cee4ff906 -->|Calls| n9166302681cb5c5e912c7ffe203d4ed6
    n2b431c16e2995ca5b60a18aac4ca949f["value_to_string"]
    n9166302681cb5c5e912c7ffe203d4ed6 -->|Calls| n2b431c16e2995ca5b60a18aac4ca949f
```

## Evidence

_No evidence cited._
