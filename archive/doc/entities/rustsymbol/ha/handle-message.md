# handle_message (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← run (`6891f75c-d8b9-56a3-9f17-a6ae5a79a3b7`)
- → ok_response (`75923916-379c-5b98-a9b4-c4b9264c6d2d`)
- → error_response (`6e38d862-2134-5ff9-917b-9c7c56f49a8e`)
- → initialize_result (`45c28e0c-51c3-541d-899d-fdef70b6efb8`)
- → tools_call (`52440250-4db8-5d19-8d69-bf98b9de4bd1`)

### Contains

- ← ekos/crates/cli/src/commands/mcp.rs (`ff76bb73-9285-5295-927c-5cb33a5bbc25`)

## Diagram

```mermaid
graph TD
    n98d3a1757e2f5ae182edc0036ee6a6f5["handle_message"]
    nff76bb7392855295927c5cb33a5bbc25["ekos/crates/cli/src/commands/mcp.rs"]
    nff76bb7392855295927c5cb33a5bbc25 -->|Contains| n98d3a1757e2f5ae182edc0036ee6a6f5
    n6891f75cd8b956a39f17a6ae5a79a3b7["run"]
    n6891f75cd8b956a39f17a6ae5a79a3b7 -->|Calls| n98d3a1757e2f5ae182edc0036ee6a6f5
    n75923916379c5b98a9b4c4b9264c6d2d["ok_response"]
    n98d3a1757e2f5ae182edc0036ee6a6f5 -->|Calls| n75923916379c5b98a9b4c4b9264c6d2d
    n6e38d86221345ff9917b9c7c56f49a8e["error_response"]
    n98d3a1757e2f5ae182edc0036ee6a6f5 -->|Calls| n6e38d86221345ff9917b9c7c56f49a8e
    n45c28e0c51c3541d899dfdef70b6efb8["initialize_result"]
    n98d3a1757e2f5ae182edc0036ee6a6f5 -->|Calls| n45c28e0c51c3541d899dfdef70b6efb8
    n524402504db85d198d69bf98b9de4bd1["tools_call"]
    n98d3a1757e2f5ae182edc0036ee6a6f5 -->|Calls| n524402504db85d198d69bf98b9de4bd1
```

## Evidence

_No evidence cited._
