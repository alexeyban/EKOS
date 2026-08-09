# merge (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- → open_branch (`bc49dd2c-e377-511c-b06c-9550959f7e15`)
- → branch_path (`9275fae1-e652-5cb3-a56c-d0e45a28067e`)

### Contains

- ← ekos/crates/cli/src/commands/branch.rs (`8ae8543c-ebb4-545a-b5fe-5735e3953e88`)

## Diagram

```mermaid
graph TD
    n419d1f8f21a4549e9c0fd83a9c18265c["merge"]
    n8ae8543cebb4545ab5fe5735e3953e88["ekos/crates/cli/src/commands/branch.rs"]
    n8ae8543cebb4545ab5fe5735e3953e88 -->|Contains| n419d1f8f21a4549e9c0fd83a9c18265c
    nbc49dd2ce377511cb06c9550959f7e15["open_branch"]
    n419d1f8f21a4549e9c0fd83a9c18265c -->|Calls| nbc49dd2ce377511cb06c9550959f7e15
    n9275fae1e6525cb3a56cd0e45a28067e["branch_path"]
    n419d1f8f21a4549e9c0fd83a9c18265c -->|Calls| n9275fae1e6525cb3a56cd0e45a28067e
```

## Evidence

_No evidence cited._
