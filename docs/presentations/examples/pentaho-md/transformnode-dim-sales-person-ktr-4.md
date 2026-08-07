# dim_sales_person.ktr:4 (TransformNode)

## Properties

| Key | Value |
|---|---|
| `columns` | [] |
| `node_type` | Source |
| `object_name` | HumanResources.Employee |

## Relationships

### FeedsInto

- → dim_sales_person.ktr:3 (`30af066c-2584-50ab-b1d2-e8b9a8bba955`)

## Diagram

```mermaid
graph TD
    nbe2f1841af6a5f809520f8d20f0cf6c6["dim_sales_person.ktr:4"]
    n30af066c258450abb1d2e8b9a8bba955["dim_sales_person.ktr:3"]
    nbe2f1841af6a5f809520f8d20f0cf6c6 -->|FeedsInto| n30af066c258450abb1d2e8b9a8bba955
```

## Evidence

- `2435097e-cfd9-51e4-bedc-291c24b3837c` — HumanResources.Employee (confidence: 1.00)
