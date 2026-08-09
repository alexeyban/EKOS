# manifest_path (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← should_recompute (`bc8bc6d9-4904-570c-a032-19abf84603ab`)
- ← record_manifest (`67114ad5-cda0-5d80-9444-0bf9047d0ecc`)

### Contains

- ← ekos/crates/compiler-core/src/cache.rs (`01ec80b2-6c80-5000-979c-acb288ff920a`)

## Diagram

```mermaid
graph TD
    naa7a9c528f6750f6a1eb13a372078457["manifest_path"]
    n01ec80b26c805000979cacb288ff920a["ekos/crates/compiler-core/src/cache.rs"]
    n01ec80b26c805000979cacb288ff920a -->|Contains| naa7a9c528f6750f6a1eb13a372078457
    nbc8bc6d94904570ca03219abf84603ab["should_recompute"]
    nbc8bc6d94904570ca03219abf84603ab -->|Calls| naa7a9c528f6750f6a1eb13a372078457
    n67114ad5cda05d8094440bf9047d0ecc["record_manifest"]
    n67114ad5cda05d8094440bf9047d0ecc -->|Calls| naa7a9c528f6750f6a1eb13a372078457
```

## Evidence

_No evidence cited._
