# bench_storage (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- → populated_ledger (`e4c4b382-b7a8-519e-a960-5b6bcfa5ea64`)
- → ledger_file_bytes (`40be71f6-5a12-5539-aced-28e4a7a4a361`)

### Contains

- ← benchmark/benches/storage_compaction.rs (`7c6dcc8e-035b-5a69-89a6-a45e962f93d8`)

## Diagram

```mermaid
graph TD
    n862cdfdbc2a75c97bfdcc5116b2d9737["bench_storage"]
    n7c6dcc8e035b5a6989a6a45e962f93d8["benchmark/benches/storage_compaction.rs"]
    n7c6dcc8e035b5a6989a6a45e962f93d8 -->|Contains| n862cdfdbc2a75c97bfdcc5116b2d9737
    ne4c4b382b7a8519ea9605b6bcfa5ea64["populated_ledger"]
    n862cdfdbc2a75c97bfdcc5116b2d9737 -->|Calls| ne4c4b382b7a8519ea9605b6bcfa5ea64
    n40be71f65a125539aced28e4a7a4a361["ledger_file_bytes"]
    n862cdfdbc2a75c97bfdcc5116b2d9737 -->|Calls| n40be71f65a125539aced28e4a7a4a361
```

## Evidence

_No evidence cited._
