# clean_removes_artifacts_not_ledger (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- → setup_workspace (`f8f102ad-bfb4-5854-8fb6-2200db5a7daf`)
- → load_config (`d1e71ee3-4e17-5050-81bd-5dd6a8cafafe`)

### Contains

- ← ekos/crates/cli/tests/skeleton.rs (`c39b7026-a223-5e82-b5c8-7ed254f6ba84`)

## Diagram

```mermaid
graph TD
    nc8408593f6d85b7083013485d093719e["clean_removes_artifacts_not_ledger"]
    nc39b7026a2235e82b5c87ed254f6ba84["ekos/crates/cli/tests/skeleton.rs"]
    nc39b7026a2235e82b5c87ed254f6ba84 -->|Contains| nc8408593f6d85b7083013485d093719e
    nf8f102adbfb458548fb62200db5a7daf["setup_workspace"]
    nc8408593f6d85b7083013485d093719e -->|Calls| nf8f102adbfb458548fb62200db5a7daf
    nd1e71ee34e17505081bd5dd6a8cafafe["load_config"]
    nc8408593f6d85b7083013485d093719e -->|Calls| nd1e71ee34e17505081bd5dd6a8cafafe
```

## Evidence

_No evidence cited._
