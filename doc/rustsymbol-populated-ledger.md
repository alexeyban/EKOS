# populated_ledger (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- → realistic_object (`0bf4f5af-e90c-51c5-87ef-f5bd09bac74b`)
- ← bench_storage (`862cdfdb-c2a7-5c97-bfdc-c5116b2d9737`)

### Contains

- ← benchmark/benches/storage_compaction.rs (`7c6dcc8e-035b-5a69-89a6-a45e962f93d8`)

## Diagram

```mermaid
graph TD
    ne4c4b382b7a8519ea9605b6bcfa5ea64["populated_ledger"]
    n7c6dcc8e035b5a6989a6a45e962f93d8["benchmark/benches/storage_compaction.rs"]
    n7c6dcc8e035b5a6989a6a45e962f93d8 -->|Contains| ne4c4b382b7a8519ea9605b6bcfa5ea64
    n0bf4f5afe90c51c587eff5bd09bac74b["realistic_object"]
    ne4c4b382b7a8519ea9605b6bcfa5ea64 -->|Calls| n0bf4f5afe90c51c587eff5bd09bac74b
    n862cdfdbc2a75c97bfdcc5116b2d9737["bench_storage"]
    n862cdfdbc2a75c97bfdcc5116b2d9737 -->|Calls| ne4c4b382b7a8519ea9605b6bcfa5ea64
```

## Evidence

_No evidence cited._
