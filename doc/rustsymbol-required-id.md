# required_id (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← call_tool (`0e48bbfe-3249-5440-a944-a03fcd474757`)
- → required_str (`34ebe6e3-6729-5e55-a0e8-f85730f2eb99`)

### Contains

- ← ekos/crates/cli/src/commands/mcp.rs (`ff76bb73-9285-5295-927c-5cb33a5bbc25`)

## Diagram

```mermaid
graph TD
    nd8d4e4a86e9d5b6da699fe2d97a8071c["required_id"]
    nff76bb7392855295927c5cb33a5bbc25["ekos/crates/cli/src/commands/mcp.rs"]
    nff76bb7392855295927c5cb33a5bbc25 -->|Contains| nd8d4e4a86e9d5b6da699fe2d97a8071c
    n0e48bbfe32495440a944a03fcd474757["call_tool"]
    n0e48bbfe32495440a944a03fcd474757 -->|Calls| nd8d4e4a86e9d5b6da699fe2d97a8071c
    n34ebe6e367295e55a0e8f85730f2eb99["required_str"]
    nd8d4e4a86e9d5b6da699fe2d97a8071c -->|Calls| n34ebe6e367295e55a0e8f85730f2eb99
```

## Evidence

_No evidence cited._
