# call_tool (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← tools_call (`52440250-4db8-5d19-8d69-bf98b9de4bd1`)
- → explain_node (`0ebfd1d7-051f-56b4-9112-e5305fb697e3`)
- → required_str (`34ebe6e3-6729-5e55-a0e8-f85730f2eb99`)
- → required_id (`d8d4e4a8-6e9d-5b6d-a699-fe2d97a8071c`)
- → transformation_chain (`fc9051fd-c6d9-5f74-9646-5a2260a739f1`)

### Contains

- ← ekos/crates/cli/src/commands/mcp.rs (`ff76bb73-9285-5295-927c-5cb33a5bbc25`)

## Diagram

```mermaid
graph TD
    n0e48bbfe32495440a944a03fcd474757["call_tool"]
    nff76bb7392855295927c5cb33a5bbc25["ekos/crates/cli/src/commands/mcp.rs"]
    nff76bb7392855295927c5cb33a5bbc25 -->|Contains| n0e48bbfe32495440a944a03fcd474757
    n524402504db85d198d69bf98b9de4bd1["tools_call"]
    n524402504db85d198d69bf98b9de4bd1 -->|Calls| n0e48bbfe32495440a944a03fcd474757
    n0ebfd1d7051f56b49112e5305fb697e3["explain_node"]
    n0e48bbfe32495440a944a03fcd474757 -->|Calls| n0ebfd1d7051f56b49112e5305fb697e3
    n34ebe6e367295e55a0e8f85730f2eb99["required_str"]
    n0e48bbfe32495440a944a03fcd474757 -->|Calls| n34ebe6e367295e55a0e8f85730f2eb99
    nd8d4e4a86e9d5b6da699fe2d97a8071c["required_id"]
    n0e48bbfe32495440a944a03fcd474757 -->|Calls| nd8d4e4a86e9d5b6da699fe2d97a8071c
    nfc9051fdc6d95f7496465a2260a739f1["transformation_chain"]
    n0e48bbfe32495440a944a03fcd474757 -->|Calls| nfc9051fdc6d95f7496465a2260a739f1
```

## Evidence

_No evidence cited._
