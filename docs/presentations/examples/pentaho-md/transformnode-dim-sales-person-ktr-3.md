# dim_sales_person.ktr:3 (TransformNode)

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

- → dim_sales_person.ktr:1 (`805f3826-5f37-5e0e-955c-94101dd972dc`)
- ← dim_sales_person.ktr:2 (`e2f19a19-f1ce-55d6-bdfa-c60c9cd35e5c`)
- ← dim_sales_person.ktr:4 (`be2f1841-af6a-5f80-9520-f8d20f0cf6c6`)

## Diagram

```mermaid
graph TD
    n30af066c258450abb1d2e8b9a8bba955["dim_sales_person.ktr:3"]
    n805f38265f375e0e955c94101dd972dc["dim_sales_person.ktr:1"]
    n30af066c258450abb1d2e8b9a8bba955 -->|FeedsInto| n805f38265f375e0e955c94101dd972dc
    ne2f19a19f1ce55d6bdfac60c9cd35e5c["dim_sales_person.ktr:2"]
    ne2f19a19f1ce55d6bdfac60c9cd35e5c -->|FeedsInto| n30af066c258450abb1d2e8b9a8bba955
    nbe2f1841af6a5f809520f8d20f0cf6c6["dim_sales_person.ktr:4"]
    nbe2f1841af6a5f809520f8d20f0cf6c6 -->|FeedsInto| n30af066c258450abb1d2e8b9a8bba955
```

## Evidence

- `59f0a65d-6638-52bd-9db8-fac3e729f8f1` — Left JOIN ON [] (confidence: 1.00)
