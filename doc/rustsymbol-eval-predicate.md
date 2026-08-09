# eval_predicate (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← EklInterpreter::execute (`feb95d3d-5916-525d-86e3-ad4cee4ff906`)
- → value_as_f64 (`8613a3d6-5583-53a9-907c-c715f59b736e`)
- → value_to_string (`2b431c16-e299-5ca5-b60a-18aac4ca949f`)
- → literal_as_f64 (`89a4f306-8073-5546-b7d3-c1c55d138bcf`)
- → value_eq (`92f87789-6991-562b-b876-73c822722296`)
- → literal_to_string (`b99e9e61-15dd-5ac5-be47-b6a1955b3f88`)

### Contains

- ← ekos/crates/ekl/src/interpreter.rs (`9c2cb6e4-ee09-503f-8cf5-ccfaf23ecd79`)

## Diagram

```mermaid
graph TD
    ndb424b5e3b63590d8e63197b48efa89a["eval_predicate"]
    n9c2cb6e4ee09503f8cf5ccfaf23ecd79["ekos/crates/ekl/src/interpreter.rs"]
    n9c2cb6e4ee09503f8cf5ccfaf23ecd79 -->|Contains| ndb424b5e3b63590d8e63197b48efa89a
    nfeb95d3d5916525d86e3ad4cee4ff906["EklInterpreter::execute"]
    nfeb95d3d5916525d86e3ad4cee4ff906 -->|Calls| ndb424b5e3b63590d8e63197b48efa89a
    n8613a3d6558353a9907cc715f59b736e["value_as_f64"]
    ndb424b5e3b63590d8e63197b48efa89a -->|Calls| n8613a3d6558353a9907cc715f59b736e
    n2b431c16e2995ca5b60a18aac4ca949f["value_to_string"]
    ndb424b5e3b63590d8e63197b48efa89a -->|Calls| n2b431c16e2995ca5b60a18aac4ca949f
    n89a4f30680735546b7d3c1c55d138bcf["literal_as_f64"]
    ndb424b5e3b63590d8e63197b48efa89a -->|Calls| n89a4f30680735546b7d3c1c55d138bcf
    n92f877896991562bb87673c822722296["value_eq"]
    ndb424b5e3b63590d8e63197b48efa89a -->|Calls| n92f877896991562bb87673c822722296
    nb99e9e6115dd5ac5be47b6a1955b3f88["literal_to_string"]
    ndb424b5e3b63590d8e63197b48efa89a -->|Calls| nb99e9e6115dd5ac5be47b6a1955b3f88
```

## Evidence

_No evidence cited._
