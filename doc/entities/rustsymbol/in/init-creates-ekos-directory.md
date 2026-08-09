# init_creates_ekos_directory (RustSymbol)

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
    n83a765f710045a659fda0e393a6a043b["init_creates_ekos_directory"]
    nc39b7026a2235e82b5c87ed254f6ba84["ekos/crates/cli/tests/skeleton.rs"]
    nc39b7026a2235e82b5c87ed254f6ba84 -->|Contains| n83a765f710045a659fda0e393a6a043b
    nf8f102adbfb458548fb62200db5a7daf["setup_workspace"]
    n83a765f710045a659fda0e393a6a043b -->|Calls| nf8f102adbfb458548fb62200db5a7daf
    nd1e71ee34e17505081bd5dd6a8cafafe["load_config"]
    n83a765f710045a659fda0e393a6a043b -->|Calls| nd1e71ee34e17505081bd5dd6a8cafafe
```

## Evidence

_No evidence cited._
