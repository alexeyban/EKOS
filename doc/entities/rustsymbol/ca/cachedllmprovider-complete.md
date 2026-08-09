# CachedLlmProvider::complete (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → cache_path (`220594e1-9d9d-53fc-be8d-24c63db12040`)
- → CachedLlmProvider::model_name (`2d44742b-555e-5c97-898b-b162cdb09a8d`)
- → cache_key (`d78c6cd6-1f31-51d0-a15f-292fb28c8614`)
- → CachedLlmProvider::complete (`a17b9530-2256-592a-8e82-2db4eb42b103`)

### Contains

- ← ekos/crates/recovery/src/cache.rs (`0b06681a-4e07-5e02-a8d6-433ccf4aadc4`)

## Diagram

```mermaid
graph TD
    na17b95302256592a8e822db4eb42b103["CachedLlmProvider::complete"]
    n0b06681a4e075e02a8d6433ccf4aadc4["ekos/crates/recovery/src/cache.rs"]
    n0b06681a4e075e02a8d6433ccf4aadc4 -->|Contains| na17b95302256592a8e822db4eb42b103
    n220594e19d9d53fcbe8d24c63db12040["cache_path"]
    na17b95302256592a8e822db4eb42b103 -->|Calls| n220594e19d9d53fcbe8d24c63db12040
    n2d44742b555e5c97898bb162cdb09a8d["CachedLlmProvider::model_name"]
    na17b95302256592a8e822db4eb42b103 -->|Calls| n2d44742b555e5c97898bb162cdb09a8d
    nd78c6cd61f3151d0a15f292fb28c8614["cache_key"]
    na17b95302256592a8e822db4eb42b103 -->|Calls| nd78c6cd61f3151d0a15f292fb28c8614
    na17b95302256592a8e822db4eb42b103 -->|Calls| na17b95302256592a8e822db4eb42b103
```

## Evidence

_No evidence cited._
