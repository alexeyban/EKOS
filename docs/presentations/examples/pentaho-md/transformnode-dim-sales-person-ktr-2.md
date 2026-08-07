# dim_sales_person.ktr:2 (TransformNode)

## Properties

| Key | Value |
|---|---|
| `join_kind` | Left |
| `keys` | [] |
| `left` | 0 |
| `node_type` | Join |
| `right` | 0 |

## Relationships

### FeedsInto

- → dim_sales_person.ktr:3 (`30af066c-2584-50ab-b1d2-e8b9a8bba955`)
- ← dim_sales_person.ktr:0 (`d3d9a08d-a353-51f5-8945-91da2a42146d`)
- ← dim_sales_person.ktr:5 (`3f22db9f-58c3-53a7-8ba5-7dcdb5388b76`)

## Diagram

```mermaid
graph TD
    ne2f19a19f1ce55d6bdfac60c9cd35e5c["dim_sales_person.ktr:2"]
    n30af066c258450abb1d2e8b9a8bba955["dim_sales_person.ktr:3"]
    ne2f19a19f1ce55d6bdfac60c9cd35e5c -->|FeedsInto| n30af066c258450abb1d2e8b9a8bba955
    nd3d9a08da35351f5894591da2a42146d["dim_sales_person.ktr:0"]
    nd3d9a08da35351f5894591da2a42146d -->|FeedsInto| ne2f19a19f1ce55d6bdfac60c9cd35e5c
    n3f22db9f58c353a78ba57dcdb5388b76["dim_sales_person.ktr:5"]
    n3f22db9f58c353a78ba57dcdb5388b76 -->|FeedsInto| ne2f19a19f1ce55d6bdfac60c9cd35e5c
```

## Evidence

- `ce462a91-e30d-5749-974f-75937cf6ad79` — Left JOIN ON [] (confidence: 1.00)
