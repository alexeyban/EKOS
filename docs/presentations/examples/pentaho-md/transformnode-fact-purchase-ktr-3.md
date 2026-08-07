# fact_purchase.ktr:3 (TransformNode)

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

- → fact_purchase.ktr:2 (`38c748f9-9595-56a1-97fd-6bbd010e579f`)
- ← fact_purchase.ktr:4 (`aa055a34-9576-5f4e-9d38-382816f185f9`)
- ← fact_purchase.ktr:5 (`f2f850a9-507a-57b8-991b-8a881da3a48b`)

## Diagram

```mermaid
graph TD
    n57e3eb816e915f8496c26095e5f42e04["fact_purchase.ktr:3"]
    n38c748f9959556a197fd6bbd010e579f["fact_purchase.ktr:2"]
    n57e3eb816e915f8496c26095e5f42e04 -->|FeedsInto| n38c748f9959556a197fd6bbd010e579f
    naa055a3495765f4e9d38382816f185f9["fact_purchase.ktr:4"]
    naa055a3495765f4e9d38382816f185f9 -->|FeedsInto| n57e3eb816e915f8496c26095e5f42e04
    nf2f850a9507a57b8991b8a881da3a48b["fact_purchase.ktr:5"]
    nf2f850a9507a57b8991b8a881da3a48b -->|FeedsInto| n57e3eb816e915f8496c26095e5f42e04
```

## Evidence

- `169298c8-b52c-5e06-9846-6553bcbfbbd8` — Left JOIN ON [] (confidence: 1.00)
