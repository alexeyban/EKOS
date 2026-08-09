# branch_path (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← create (`1aeccd3c-ded5-5915-9ccc-bc3f4dd027b6`)
- ← merge (`419d1f8f-21a4-549e-9c0f-d83a9c18265c`)
- ← delete (`0f40d4c6-e61e-5823-888b-1d2fe90124dc`)

### Contains

- ← ekos/crates/cli/src/commands/branch.rs (`8ae8543c-ebb4-545a-b5fe-5735e3953e88`)

## Diagram

```mermaid
graph TD
    n9275fae1e6525cb3a56cd0e45a28067e["branch_path"]
    n8ae8543cebb4545ab5fe5735e3953e88["ekos/crates/cli/src/commands/branch.rs"]
    n8ae8543cebb4545ab5fe5735e3953e88 -->|Contains| n9275fae1e6525cb3a56cd0e45a28067e
    n1aeccd3cded559159cccbc3f4dd027b6["create"]
    n1aeccd3cded559159cccbc3f4dd027b6 -->|Calls| n9275fae1e6525cb3a56cd0e45a28067e
    n419d1f8f21a4549e9c0fd83a9c18265c["merge"]
    n419d1f8f21a4549e9c0fd83a9c18265c -->|Calls| n9275fae1e6525cb3a56cd0e45a28067e
    n0f40d4c6e61e5823888b1d2fe90124dc["delete"]
    n0f40d4c6e61e5823888b1d2fe90124dc -->|Calls| n9275fae1e6525cb3a56cd0e45a28067e
```

## Evidence

_No evidence cited._
