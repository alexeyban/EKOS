# fact_sales.ktr:6 (TransformNode)

## Properties

| Key | Value |
|---|---|
| `columns` | [] |
| `node_type` | Source |
| `object_name` | Sales.SalesOrderDetail |

## Relationships

### FeedsInto

- → fact_sales.ktr:8 (`aa78f67a-b68c-54eb-9ca8-6621c38a74d2`)

## Diagram

```mermaid
graph TD
    nc03772510bee537c801c875029ca73c3["fact_sales.ktr:6"]
    naa78f67ab68c54eb9ca86621c38a74d2["fact_sales.ktr:8"]
    nc03772510bee537c801c875029ca73c3 -->|FeedsInto| naa78f67ab68c54eb9ca86621c38a74d2
```

## Evidence

- `ef6dbadb-cf3f-5722-ade6-6ff552273ee5` — Sales.SalesOrderDetail (confidence: 1.00)
