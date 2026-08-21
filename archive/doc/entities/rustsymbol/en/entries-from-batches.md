# entries_from_batches (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- → IndexEntry::from_fact (`2fd9d8a2-1c89-5478-8e80-95a3253931e9`)
- ← FactIndexes::build_from_batches (`01b36534-35a3-5736-8a1e-c727d7f48136`)

### Contains

- ← ekos/crates/ledger/src/index.rs (`a43fa387-2cac-5166-bfc2-ae4c965fc2ac`)

## Diagram

```mermaid
graph TD
    n1a7fc575224c562ea9babce6552c54e4["entries_from_batches"]
    na43fa3872cac5166bfc2ae4c965fc2ac["ekos/crates/ledger/src/index.rs"]
    na43fa3872cac5166bfc2ae4c965fc2ac -->|Contains| n1a7fc575224c562ea9babce6552c54e4
    n2fd9d8a21c8954788e8095a3253931e9["IndexEntry::from_fact"]
    n1a7fc575224c562ea9babce6552c54e4 -->|Calls| n2fd9d8a21c8954788e8095a3253931e9
    n01b3653435a357368a1ec727d7f48136["FactIndexes::build_from_batches"]
    n01b3653435a357368a1ec727d7f48136 -->|Calls| n1a7fc575224c562ea9babce6552c54e4
```

## Evidence

_No evidence cited._
