# EmailParser::parse (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → header_block (`5e7be91d-4d37-5e3e-b041-33ece5b9675c`)
- → body_text (`02700955-771f-5404-a469-e30ff1f3274f`)
- → EmailParser::parse (`f784bab1-c5b1-5155-b44d-018b4df1cf66`)

### Contains

- ← ekos/plugins/localdocs/src/email.rs (`fbdd8fb1-8ac2-5e5d-b95b-c1d0451eae6e`)

## Diagram

```mermaid
graph TD
    nf784bab1c5b15155b44d018b4df1cf66["EmailParser::parse"]
    nfbdd8fb18ac25e5db95bc1d0451eae6e["ekos/plugins/localdocs/src/email.rs"]
    nfbdd8fb18ac25e5db95bc1d0451eae6e -->|Contains| nf784bab1c5b15155b44d018b4df1cf66
    n5e7be91d4d375e3eb04133ece5b9675c["header_block"]
    nf784bab1c5b15155b44d018b4df1cf66 -->|Calls| n5e7be91d4d375e3eb04133ece5b9675c
    n02700955771f5404a469e30ff1f3274f["body_text"]
    nf784bab1c5b15155b44d018b4df1cf66 -->|Calls| n02700955771f5404a469e30ff1f3274f
    nf784bab1c5b15155b44d018b4df1cf66 -->|Calls| nf784bab1c5b15155b44d018b4df1cf66
```

## Evidence

_No evidence cited._
