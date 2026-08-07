# dim_date.ktr:6 (TransformNode)

## Properties

| Key | Value |
|---|---|
| `excerpt` | global = 'Y' |
| `node_type` | Filter |

## Relationships

### FeedsInto

- → dim_date.ktr:4 (`75430eb0-f4fe-5215-ad6c-1c4734756c60`)
- ← dim_date.ktr:7 (`fe55ff87-f4fc-57b1-9fa8-7d4d3398f1a7`)

## Diagram

```mermaid
graph TD
    n6058cd8d9cd65654a6489d1249282de0["dim_date.ktr:6"]
    n75430eb0f4fe5215ad6c1c4734756c60["dim_date.ktr:4"]
    n6058cd8d9cd65654a6489d1249282de0 -->|FeedsInto| n75430eb0f4fe5215ad6c1c4734756c60
    nfe55ff87f4fc57b19fa87d4d3398f1a7["dim_date.ktr:7"]
    nfe55ff87f4fc57b19fa87d4d3398f1a7 -->|FeedsInto| n6058cd8d9cd65654a6489d1249282de0
```

## Evidence

- `d226e73d-d0ff-5d70-8a10-b8343ccc89e9` — global = 'Y' (confidence: 1.00)
