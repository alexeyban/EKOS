# build_observes_files_and_writes_ledger (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- → load_config (`d1e71ee3-4e17-5050-81bd-5dd6a8cafafe`)
- → setup_workspace (`f8f102ad-bfb4-5854-8fb6-2200db5a7daf`)

### Contains

- ← ekos/crates/cli/tests/skeleton.rs (`c39b7026-a223-5e82-b5c8-7ed254f6ba84`)

## Diagram

```mermaid
graph TD
    n8381b3cfb4ab5e24b22325b1e3647ee2["build_observes_files_and_writes_ledger"]
    nc39b7026a2235e82b5c87ed254f6ba84["ekos/crates/cli/tests/skeleton.rs"]
    nc39b7026a2235e82b5c87ed254f6ba84 -->|Contains| n8381b3cfb4ab5e24b22325b1e3647ee2
    nd1e71ee34e17505081bd5dd6a8cafafe["load_config"]
    n8381b3cfb4ab5e24b22325b1e3647ee2 -->|Calls| nd1e71ee34e17505081bd5dd6a8cafafe
    nf8f102adbfb458548fb62200db5a7daf["setup_workspace"]
    n8381b3cfb4ab5e24b22325b1e3647ee2 -->|Calls| nf8f102adbfb458548fb62200db5a7daf
```

## Evidence

_No evidence cited._
