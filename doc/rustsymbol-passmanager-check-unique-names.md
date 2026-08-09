# PassManager::check_unique_names (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → PassManager::len (`621f7d1a-9bff-502d-9e32-1f7cbefd68e8`)
- ← PassManager::execution_order (`293e83a5-d93e-54cf-957a-3cc539a369fe`)
- ← PassManager::execution_levels (`5b7ff059-a0b1-54d7-91b1-6fd5a3bbfe85`)

### Contains

- ← ekos/crates/compiler-core/src/pass.rs (`5189d0a2-3e2b-529e-adbf-3984a77be404`)

## Diagram

```mermaid
graph TD
    n523e017fa17a520e925d295f0abfb465["PassManager::check_unique_names"]
    n5189d0a23e2b529eadbf3984a77be404["ekos/crates/compiler-core/src/pass.rs"]
    n5189d0a23e2b529eadbf3984a77be404 -->|Contains| n523e017fa17a520e925d295f0abfb465
    n621f7d1a9bff502d9e321f7cbefd68e8["PassManager::len"]
    n523e017fa17a520e925d295f0abfb465 -->|Calls| n621f7d1a9bff502d9e321f7cbefd68e8
    n293e83a5d93e54cf957a3cc539a369fe["PassManager::execution_order"]
    n293e83a5d93e54cf957a3cc539a369fe -->|Calls| n523e017fa17a520e925d295f0abfb465
    n5b7ff059a0b154d791b16fd5a3bbfe85["PassManager::execution_levels"]
    n5b7ff059a0b154d791b16fd5a3bbfe85 -->|Calls| n523e017fa17a520e925d295f0abfb465
```

## Evidence

_No evidence cited._
