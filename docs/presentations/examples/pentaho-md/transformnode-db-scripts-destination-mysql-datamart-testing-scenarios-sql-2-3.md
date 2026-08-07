# DB Scripts/Destination MySQL/datamart.testing.scenarios.sql#2:3 (TransformNode)

## Properties

| Key | Value |
|---|---|
| `excerpt` | s.sales_order_id = 51092 AND p.product_id = 737 |
| `node_type` | Filter |

## Relationships

### FeedsInto

- ← DB Scripts/Destination MySQL/datamart.testing.scenarios.sql#2:2 (`d95e468e-1463-5b13-9e43-7b53bcb6e0e8`)

## Diagram

```mermaid
graph TD
    n0a079edb2efb5becac838f4c8bd3eb25["DB Scripts/Destination MySQL/datamart.testing.scenarios.sql#2:3"]
    nd95e468e14635b139e437b53bcb6e0e8["DB Scripts/Destination MySQL/datamart.testing.scenarios.sql#2:2"]
    nd95e468e14635b139e437b53bcb6e0e8 -->|FeedsInto| n0a079edb2efb5becac838f4c8bd3eb25
```

## Evidence

- `3188e8f9-c671-541c-b05a-bbb0339761d1` — s.sales_order_id = 51092 AND p.product_id = 737 (confidence: 1.00)
