# dim_customer.ktr:7 (TransformNode)

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

- → dim_customer.ktr:9 (`13a1cc5a-f528-5e37-aa60-650e1632c222`)
- ← dim_customer.ktr:4 (`65f972fc-cf69-5b31-b267-600dd103faef`)
- ← dim_customer.ktr:6 (`f059165c-f525-5050-92d6-a43c94e62f9d`)

## Diagram

```mermaid
graph TD
    n91a74c3f86b85ea3989e5d6d577864fd["dim_customer.ktr:7"]
    n13a1cc5af5285e37aa60650e1632c222["dim_customer.ktr:9"]
    n91a74c3f86b85ea3989e5d6d577864fd -->|FeedsInto| n13a1cc5af5285e37aa60650e1632c222
    n65f972fccf695b31b267600dd103faef["dim_customer.ktr:4"]
    n65f972fccf695b31b267600dd103faef -->|FeedsInto| n91a74c3f86b85ea3989e5d6d577864fd
    nf059165cf525505092d6a43c94e62f9d["dim_customer.ktr:6"]
    nf059165cf525505092d6a43c94e62f9d -->|FeedsInto| n91a74c3f86b85ea3989e5d6d577864fd
```

## Evidence

- `5922e60b-7aa9-5cc4-bd95-16d970660829` — Left JOIN ON [] (confidence: 1.00)
