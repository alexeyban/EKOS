# value_as_f64 (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← value_eq (`92f87789-6991-562b-b876-73c822722296`)
- ← eval_predicate (`db424b5e-3b63-590d-8e63-197b48efa89a`)

### Contains

- ← ekos/crates/ekl/src/interpreter.rs (`9c2cb6e4-ee09-503f-8cf5-ccfaf23ecd79`)

## Diagram

```mermaid
graph TD
    n8613a3d6558353a9907cc715f59b736e["value_as_f64"]
    n9c2cb6e4ee09503f8cf5ccfaf23ecd79["ekos/crates/ekl/src/interpreter.rs"]
    n9c2cb6e4ee09503f8cf5ccfaf23ecd79 -->|Contains| n8613a3d6558353a9907cc715f59b736e
    n92f877896991562bb87673c822722296["value_eq"]
    n92f877896991562bb87673c822722296 -->|Calls| n8613a3d6558353a9907cc715f59b736e
    ndb424b5e3b63590d8e63197b48efa89a["eval_predicate"]
    ndb424b5e3b63590d8e63197b48efa89a -->|Calls| n8613a3d6558353a9907cc715f59b736e
```

## Evidence

_No evidence cited._
