# generate_curated (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← generate (`9628a7cf-316d-5400-8261-6b2216ee01f1`)
- → write_page (`6672efa9-4ce8-5c9c-a096-3c74f2963490`)

### Contains

- ← ekos/crates/cli/src/commands/docs.rs (`5503d15b-112f-541f-8189-8be05a060beb`)

## Diagram

```mermaid
graph TD
    n5a70d7a9bb4c59dca7cd00be6ab7f553["generate_curated"]
    n5503d15b112f541f81898be05a060beb["ekos/crates/cli/src/commands/docs.rs"]
    n5503d15b112f541f81898be05a060beb -->|Contains| n5a70d7a9bb4c59dca7cd00be6ab7f553
    n9628a7cf316d540082616b2216ee01f1["generate"]
    n9628a7cf316d540082616b2216ee01f1 -->|Calls| n5a70d7a9bb4c59dca7cd00be6ab7f553
    n6672efa94ce85c9ca0963c74f2963490["write_page"]
    n5a70d7a9bb4c59dca7cd00be6ab7f553 -->|Calls| n6672efa94ce85c9ca0963c74f2963490
```

## Evidence

_No evidence cited._
