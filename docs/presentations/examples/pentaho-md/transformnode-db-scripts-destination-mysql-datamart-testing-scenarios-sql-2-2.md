# DB Scripts/Destination MySQL/datamart.testing.scenarios.sql#2:2 (TransformNode)

## Properties

| Key | Value |
|---|---|
| `join_kind` | Inner |
| `keys` | [["s.dim_product_id","p.dim_product_id"]] |
| `left` | 0 |
| `node_type` | Join |
| `right` | 1 |

## Relationships

### FeedsInto

- → DB Scripts/Destination MySQL/datamart.testing.scenarios.sql#2:3 (`0a079edb-2efb-5bec-ac83-8f4c8bd3eb25`)
- ← DB Scripts/Destination MySQL/datamart.testing.scenarios.sql#2:0 (`421db59f-415a-5f93-9de2-138253399948`)
- ← DB Scripts/Destination MySQL/datamart.testing.scenarios.sql#2:1 (`3d1e41c2-d8b2-550d-b111-52ba283c0637`)

## Diagram

```mermaid
graph TD
    nd95e468e14635b139e437b53bcb6e0e8["DB Scripts/Destination MySQL/datamart.testing.scenarios.sql#2:2"]
    n0a079edb2efb5becac838f4c8bd3eb25["DB Scripts/Destination MySQL/datamart.testing.scenarios.sql#2:3"]
    nd95e468e14635b139e437b53bcb6e0e8 -->|FeedsInto| n0a079edb2efb5becac838f4c8bd3eb25
    n421db59f415a5f939de2138253399948["DB Scripts/Destination MySQL/datamart.testing.scenarios.sql#2:0"]
    n421db59f415a5f939de2138253399948 -->|FeedsInto| nd95e468e14635b139e437b53bcb6e0e8
    n3d1e41c2d8b2550db11152ba283c0637["DB Scripts/Destination MySQL/datamart.testing.scenarios.sql#2:1"]
    n3d1e41c2d8b2550db11152ba283c0637 -->|FeedsInto| nd95e468e14635b139e437b53bcb6e0e8
```

## Evidence

- `e9f204da-8522-5ed5-a442-dda05dcb3d6e` — Inner JOIN ON [("s.dim_product_id", "p.dim_product_id")] (confidence: 1.00)
