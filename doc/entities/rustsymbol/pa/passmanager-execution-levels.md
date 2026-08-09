# PassManager::execution_levels (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → PassManager::is_empty (`35af3903-5819-5ec3-9396-11a8a343a41c`)
- → PassManager::check_unique_names (`523e017f-a17a-520e-925d-295f0abfb465`)
- → PassManager::len (`621f7d1a-9bff-502d-9e32-1f7cbefd68e8`)
- ← PassManager::run_all_parallel (`f64251fc-07c7-5863-b006-0419e9c2f655`)

### Contains

- ← ekos/crates/compiler-core/src/pass.rs (`5189d0a2-3e2b-529e-adbf-3984a77be404`)

## Diagram

```mermaid
graph TD
    n5b7ff059a0b154d791b16fd5a3bbfe85["PassManager::execution_levels"]
    n5189d0a23e2b529eadbf3984a77be404["ekos/crates/compiler-core/src/pass.rs"]
    n5189d0a23e2b529eadbf3984a77be404 -->|Contains| n5b7ff059a0b154d791b16fd5a3bbfe85
    n35af390358195ec3939611a8a343a41c["PassManager::is_empty"]
    n5b7ff059a0b154d791b16fd5a3bbfe85 -->|Calls| n35af390358195ec3939611a8a343a41c
    n523e017fa17a520e925d295f0abfb465["PassManager::check_unique_names"]
    n5b7ff059a0b154d791b16fd5a3bbfe85 -->|Calls| n523e017fa17a520e925d295f0abfb465
    n621f7d1a9bff502d9e321f7cbefd68e8["PassManager::len"]
    n5b7ff059a0b154d791b16fd5a3bbfe85 -->|Calls| n621f7d1a9bff502d9e321f7cbefd68e8
    nf64251fc07c75863b0060419e9c2f655["PassManager::run_all_parallel"]
    nf64251fc07c75863b0060419e9c2f655 -->|Calls| n5b7ff059a0b154d791b16fd5a3bbfe85
```

## Evidence

_No evidence cited._
