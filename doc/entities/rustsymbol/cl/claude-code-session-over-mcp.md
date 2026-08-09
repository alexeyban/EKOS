# claude_code_session_over_mcp (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- → setup_workspace (`60facea1-f085-5fb6-9750-725533dae17f`)
- → load_config (`467babc2-5a36-5968-88d4-8cd2204b2f95`)
- → call_tool (`79df7d9c-60b9-5b0c-82a0-8fea3e18a0a0`)

### Contains

- ← ekos/crates/cli/tests/mcp_session.rs (`201efc61-073b-5bcd-a5a8-a6c476333729`)

## Diagram

```mermaid
graph TD
    na0f174d3a1ae5c3886aa9920ae2beb45["claude_code_session_over_mcp"]
    n201efc61073b5bcda5a8a6c476333729["ekos/crates/cli/tests/mcp_session.rs"]
    n201efc61073b5bcda5a8a6c476333729 -->|Contains| na0f174d3a1ae5c3886aa9920ae2beb45
    n60facea1f0855fb69750725533dae17f["setup_workspace"]
    na0f174d3a1ae5c3886aa9920ae2beb45 -->|Calls| n60facea1f0855fb69750725533dae17f
    n467babc25a36596888d48cd2204b2f95["load_config"]
    na0f174d3a1ae5c3886aa9920ae2beb45 -->|Calls| n467babc25a36596888d48cd2204b2f95
    n79df7d9c60b95b0c82a08fea3e18a0a0["call_tool"]
    na0f174d3a1ae5c3886aa9920ae2beb45 -->|Calls| n79df7d9c60b95b0c82a08fea3e18a0a0
```

## Evidence

_No evidence cited._
