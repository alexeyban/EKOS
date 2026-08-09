# tools_call (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← handle_message (`98d3a175-7e2f-5ae1-82ed-c0036ee6a6f5`)
- → call_tool (`0e48bbfe-3249-5440-a944-a03fcd474757`)

### Contains

- ← ekos/crates/cli/src/commands/mcp.rs (`ff76bb73-9285-5295-927c-5cb33a5bbc25`)

## Diagram

```mermaid
graph TD
    n524402504db85d198d69bf98b9de4bd1["tools_call"]
    nff76bb7392855295927c5cb33a5bbc25["ekos/crates/cli/src/commands/mcp.rs"]
    nff76bb7392855295927c5cb33a5bbc25 -->|Contains| n524402504db85d198d69bf98b9de4bd1
    n98d3a1757e2f5ae182edc0036ee6a6f5["handle_message"]
    n98d3a1757e2f5ae182edc0036ee6a6f5 -->|Calls| n524402504db85d198d69bf98b9de4bd1
    n0e48bbfe32495440a944a03fcd474757["call_tool"]
    n524402504db85d198d69bf98b9de4bd1 -->|Calls| n0e48bbfe32495440a944a03fcd474757
```

## Evidence

_No evidence cited._
