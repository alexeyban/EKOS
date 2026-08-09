# ArtifactId::compute (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → canonicalize (`08d35be0-096d-5e3c-94d4-424966d024d9`)
- ← compute_content_id (`68328ad4-597d-5947-afb0-8d5a0804e3dd`)

### Contains

- ← ekos/crates/artifact/src/lib.rs (`918532b1-7390-5128-8de5-faf4f7a91daf`)

## Diagram

```mermaid
graph TD
    n0e9e62a4010053e3bc58f7c408482937["ArtifactId::compute"]
    n918532b1739051288de5faf4f7a91daf["ekos/crates/artifact/src/lib.rs"]
    n918532b1739051288de5faf4f7a91daf -->|Contains| n0e9e62a4010053e3bc58f7c408482937
    n08d35be0096d5e3c94d4424966d024d9["canonicalize"]
    n0e9e62a4010053e3bc58f7c408482937 -->|Calls| n08d35be0096d5e3c94d4424966d024d9
    n68328ad4597d5947afb08d5a0804e3dd["compute_content_id"]
    n68328ad4597d5947afb08d5a0804e3dd -->|Calls| n0e9e62a4010053e3bc58f7c408482937
```

## Evidence

_No evidence cited._
