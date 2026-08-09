# query_object_returns_known_file (RustSymbol)

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
    n41345ca495295fa7994e937f12f58595["query_object_returns_known_file"]
    nc39b7026a2235e82b5c87ed254f6ba84["ekos/crates/cli/tests/skeleton.rs"]
    nc39b7026a2235e82b5c87ed254f6ba84 -->|Contains| n41345ca495295fa7994e937f12f58595
    nd1e71ee34e17505081bd5dd6a8cafafe["load_config"]
    n41345ca495295fa7994e937f12f58595 -->|Calls| nd1e71ee34e17505081bd5dd6a8cafafe
    nf8f102adbfb458548fb62200db5a7daf["setup_workspace"]
    n41345ca495295fa7994e937f12f58595 -->|Calls| nf8f102adbfb458548fb62200db5a7daf
```

## Evidence

_No evidence cited._
