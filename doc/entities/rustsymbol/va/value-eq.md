# value_eq (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- → value_as_f64 (`8613a3d6-5583-53a9-907c-c715f59b736e`)
- → value_to_string (`2b431c16-e299-5ca5-b60a-18aac4ca949f`)
- ← eval_predicate (`db424b5e-3b63-590d-8e63-197b48efa89a`)

### Contains

- ← ekos/crates/ekl/src/interpreter.rs (`9c2cb6e4-ee09-503f-8cf5-ccfaf23ecd79`)

## Diagram

```mermaid
graph TD
    n92f877896991562bb87673c822722296["value_eq"]
    n9c2cb6e4ee09503f8cf5ccfaf23ecd79["ekos/crates/ekl/src/interpreter.rs"]
    n9c2cb6e4ee09503f8cf5ccfaf23ecd79 -->|Contains| n92f877896991562bb87673c822722296
    n8613a3d6558353a9907cc715f59b736e["value_as_f64"]
    n92f877896991562bb87673c822722296 -->|Calls| n8613a3d6558353a9907cc715f59b736e
    n2b431c16e2995ca5b60a18aac4ca949f["value_to_string"]
    n92f877896991562bb87673c822722296 -->|Calls| n2b431c16e2995ca5b60a18aac4ca949f
    ndb424b5e3b63590d8e63197b48efa89a["eval_predicate"]
    ndb424b5e3b63590d8e63197b48efa89a -->|Calls| n92f877896991562bb87673c822722296
```

## Evidence

_No evidence cited._
