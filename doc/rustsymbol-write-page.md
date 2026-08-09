# write_page (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← generate (`9628a7cf-316d-5400-8261-6b2216ee01f1`)
- ← generate_curated (`5a70d7a9-bb4c-59dc-a7cd-00be6ab7f553`)

### Contains

- ← ekos/crates/cli/src/commands/docs.rs (`5503d15b-112f-541f-8189-8be05a060beb`)

## Diagram

```mermaid
graph TD
    n6672efa94ce85c9ca0963c74f2963490["write_page"]
    n5503d15b112f541f81898be05a060beb["ekos/crates/cli/src/commands/docs.rs"]
    n5503d15b112f541f81898be05a060beb -->|Contains| n6672efa94ce85c9ca0963c74f2963490
    n9628a7cf316d540082616b2216ee01f1["generate"]
    n9628a7cf316d540082616b2216ee01f1 -->|Calls| n6672efa94ce85c9ca0963c74f2963490
    n5a70d7a9bb4c59dca7cd00be6ab7f553["generate_curated"]
    n5a70d7a9bb4c59dca7cd00be6ab7f553 -->|Calls| n6672efa94ce85c9ca0963c74f2963490
```

## Evidence

_No evidence cited._
