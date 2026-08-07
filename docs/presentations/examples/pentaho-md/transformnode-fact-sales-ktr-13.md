# fact_sales.ktr:13 (TransformNode)

## Properties

| Key | Value |
|---|---|
| `columns` | [] |
| `node_type` | Source |
| `object_name` | dim_sales_person |

## Relationships

### FeedsInto

- → fact_sales.ktr:9 (`71039590-7ed6-5761-a7c7-95fe29d56665`)

## Diagram

```mermaid
graph TD
    n3d941052bd4a5d8c92a548d0d0a4cc75["fact_sales.ktr:13"]
    n710395907ed65761a7c795fe29d56665["fact_sales.ktr:9"]
    n3d941052bd4a5d8c92a548d0d0a4cc75 -->|FeedsInto| n710395907ed65761a7c795fe29d56665
```

## Evidence

- `8f697512-99c7-5eee-9f8d-18bb579150e0` — dim_sales_person (confidence: 1.00)
