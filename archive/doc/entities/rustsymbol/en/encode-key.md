# encode_key (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- → value_order_key (`9a9ea627-8891-52e2-a6a7-1ade17a48fa6`)
- → push_pos (`0a415a8e-cd91-5d4b-915a-2b234ac64d67`)
- → push_escaped (`479cc024-8a4a-58db-9137-6a0b043be5e8`)
- ← project (`775a5c11-b65e-541d-a531-4ca75476a2b6`)
- ← FactIndexes::scan (`a6eecec9-893d-552b-9d21-a9ca35b1c87d`)

### Contains

- ← ekos/crates/ledger/src/index.rs (`a43fa387-2cac-5166-bfc2-ae4c965fc2ac`)

## Diagram

```mermaid
graph TD
    n5f3049a28e6555109ee3ab92690f254a["encode_key"]
    na43fa3872cac5166bfc2ae4c965fc2ac["ekos/crates/ledger/src/index.rs"]
    na43fa3872cac5166bfc2ae4c965fc2ac -->|Contains| n5f3049a28e6555109ee3ab92690f254a
    n9a9ea627889152e2a6a71ade17a48fa6["value_order_key"]
    n5f3049a28e6555109ee3ab92690f254a -->|Calls| n9a9ea627889152e2a6a71ade17a48fa6
    n0a415a8ecd915d4b915a2b234ac64d67["push_pos"]
    n5f3049a28e6555109ee3ab92690f254a -->|Calls| n0a415a8ecd915d4b915a2b234ac64d67
    n479cc0248a4a58db91376a0b043be5e8["push_escaped"]
    n5f3049a28e6555109ee3ab92690f254a -->|Calls| n479cc0248a4a58db91376a0b043be5e8
    n775a5c11b65e541da5314ca75476a2b6["project"]
    n775a5c11b65e541da5314ca75476a2b6 -->|Calls| n5f3049a28e6555109ee3ab92690f254a
    na6eecec9893d552b9d21a9ca35b1c87d["FactIndexes::scan"]
    na6eecec9893d552b9d21a9ca35b1c87d -->|Calls| n5f3049a28e6555109ee3ab92690f254a
```

## Evidence

_No evidence cited._
