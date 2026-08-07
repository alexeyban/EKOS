# DB Scripts/Destination MySQL/datamart.testing.scenarios.sql#3:0 (TransformNode)

## Properties

| Key | Value |
|---|---|
| `columns` | [] |
| `node_type` | Source |
| `object_name` | fact_purchases |

## Relationships

### FeedsInto

- → DB Scripts/Destination MySQL/datamart.testing.scenarios.sql#3:2 (`0c5e40a1-01ac-522b-aebf-7cfab887c9c7`)

## Diagram

```mermaid
graph TD
    n1ad8cca65cea572287f37507db631292["DB Scripts/Destination MySQL/datamart.testing.scenarios.sql#3:0"]
    n0c5e40a101ac522baebf7cfab887c9c7["DB Scripts/Destination MySQL/datamart.testing.scenarios.sql#3:2"]
    n1ad8cca65cea572287f37507db631292 -->|FeedsInto| n0c5e40a101ac522baebf7cfab887c9c7
```

## Evidence

- `01ee44b8-41d5-5ce3-a928-97c5a24b08a7` — fact_purchases (confidence: 1.00)
