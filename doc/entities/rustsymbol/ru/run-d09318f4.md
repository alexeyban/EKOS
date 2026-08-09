# run (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- → prune_snapshots (`f9f60185-26ce-5298-b387-17a129857d00`)
- → save_fingerprints (`b7ee0369-14d2-5ac7-a8b9-893ebb6644d7`)
- → load_fingerprints (`ab00e332-2b4e-50ad-9ea7-b0178ac4cd6b`)

### Contains

- ← ekos/crates/cli/src/commands/build.rs (`306def2e-bd5a-5784-9453-692c119e8d43`)

## Diagram

```mermaid
graph TD
    nd09318f4bb3c5be79348151887565314["run"]
    n306def2ebd5a57849453692c119e8d43["ekos/crates/cli/src/commands/build.rs"]
    n306def2ebd5a57849453692c119e8d43 -->|Contains| nd09318f4bb3c5be79348151887565314
    nf9f6018526ce5298b38717a129857d00["prune_snapshots"]
    nd09318f4bb3c5be79348151887565314 -->|Calls| nf9f6018526ce5298b38717a129857d00
    nb7ee036914d25ac7a8b9893ebb6644d7["save_fingerprints"]
    nd09318f4bb3c5be79348151887565314 -->|Calls| nb7ee036914d25ac7a8b9893ebb6644d7
    nab00e3322b4e50ad9ea7b0178ac4cd6b["load_fingerprints"]
    nd09318f4bb3c5be79348151887565314 -->|Calls| nab00e3322b4e50ad9ea7b0178ac4cd6b
```

## Evidence

_No evidence cited._
