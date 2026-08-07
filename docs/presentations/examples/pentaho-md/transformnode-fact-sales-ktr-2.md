# fact_sales.ktr:2 (TransformNode)

## Properties

| Key | Value |
|---|---|
| `node_type` | Unmapped |
| `raw` |  |
| `reason` | Calculator step has no recognizable calculated fields |

## Relationships

### FeedsInto

- → fact_sales.ktr:0 (`bd436d40-4053-57f4-ac77-b4ffe37edae4`)
- ← fact_sales.ktr:8 (`aa78f67a-b68c-54eb-9ca8-6621c38a74d2`)

## Diagram

```mermaid
graph TD
    n0c1ebe1c815954839e8d8a04c7c91a2e["fact_sales.ktr:2"]
    nbd436d40405357f4ac77b4ffe37edae4["fact_sales.ktr:0"]
    n0c1ebe1c815954839e8d8a04c7c91a2e -->|FeedsInto| nbd436d40405357f4ac77b4ffe37edae4
    naa78f67ab68c54eb9ca86621c38a74d2["fact_sales.ktr:8"]
    naa78f67ab68c54eb9ca86621c38a74d2 -->|FeedsInto| n0c1ebe1c815954839e8d8a04c7c91a2e
```

## Evidence

- `7dbe17c0-570f-5fa1-8317-47e3fa7fe9f3` —  (confidence: 1.00)
