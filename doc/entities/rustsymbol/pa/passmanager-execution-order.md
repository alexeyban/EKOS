# PassManager::execution_order (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → PassManager::len (`621f7d1a-9bff-502d-9e32-1f7cbefd68e8`)
- → PassManager::check_unique_names (`523e017f-a17a-520e-925d-295f0abfb465`)
- ← PassManager::run_all (`3e578779-ba09-580a-9d6d-8319115114de`)

### Contains

- ← ekos/crates/compiler-core/src/pass.rs (`5189d0a2-3e2b-529e-adbf-3984a77be404`)

## Diagram

```mermaid
graph TD
    n293e83a5d93e54cf957a3cc539a369fe["PassManager::execution_order"]
    n5189d0a23e2b529eadbf3984a77be404["ekos/crates/compiler-core/src/pass.rs"]
    n5189d0a23e2b529eadbf3984a77be404 -->|Contains| n293e83a5d93e54cf957a3cc539a369fe
    n621f7d1a9bff502d9e321f7cbefd68e8["PassManager::len"]
    n293e83a5d93e54cf957a3cc539a369fe -->|Calls| n621f7d1a9bff502d9e321f7cbefd68e8
    n523e017fa17a520e925d295f0abfb465["PassManager::check_unique_names"]
    n293e83a5d93e54cf957a3cc539a369fe -->|Calls| n523e017fa17a520e925d295f0abfb465
    n3e578779ba09580a9d6d8319115114de["PassManager::run_all"]
    n3e578779ba09580a9d6d8319115114de -->|Calls| n293e83a5d93e54cf957a3cc539a369fe
```

## Evidence

_No evidence cited._
