# header_block (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← EmailParser::parse (`f784bab1-c5b1-5155-b44d-018b4df1cf66`)
- → render_address (`b31cf62e-d2d3-5487-a66b-c9b553b78284`)

### Contains

- ← ekos/plugins/localdocs/src/email.rs (`fbdd8fb1-8ac2-5e5d-b95b-c1d0451eae6e`)

## Diagram

```mermaid
graph TD
    n5e7be91d4d375e3eb04133ece5b9675c["header_block"]
    nfbdd8fb18ac25e5db95bc1d0451eae6e["ekos/plugins/localdocs/src/email.rs"]
    nfbdd8fb18ac25e5db95bc1d0451eae6e -->|Contains| n5e7be91d4d375e3eb04133ece5b9675c
    nf784bab1c5b15155b44d018b4df1cf66["EmailParser::parse"]
    nf784bab1c5b15155b44d018b4df1cf66 -->|Calls| n5e7be91d4d375e3eb04133ece5b9675c
    nb31cf62ed2d35487a66bc9b553b78284["render_address"]
    n5e7be91d4d375e3eb04133ece5b9675c -->|Calls| nb31cf62ed2d35487a66bc9b553b78284
```

## Evidence

_No evidence cited._
