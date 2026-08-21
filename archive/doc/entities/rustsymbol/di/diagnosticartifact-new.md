# DiagnosticArtifact::new (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → ArtifactMeta::default (`75608e6d-ce35-5321-97af-f8b9d4f486b2`)
- → compute_content_id (`68328ad4-597d-5947-afb0-8d5a0804e3dd`)

### Contains

- ← ekos/crates/artifact/src/lib.rs (`918532b1-7390-5128-8de5-faf4f7a91daf`)

## Diagram

```mermaid
graph TD
    n3529a593008c5745a14d524906d6cbd2["DiagnosticArtifact::new"]
    n918532b1739051288de5faf4f7a91daf["ekos/crates/artifact/src/lib.rs"]
    n918532b1739051288de5faf4f7a91daf -->|Contains| n3529a593008c5745a14d524906d6cbd2
    n75608e6dce35532197aff8b9d4f486b2["ArtifactMeta::default"]
    n3529a593008c5745a14d524906d6cbd2 -->|Calls| n75608e6dce35532197aff8b9d4f486b2
    n68328ad4597d5947afb08d5a0804e3dd["compute_content_id"]
    n3529a593008c5745a14d524906d6cbd2 -->|Calls| n68328ad4597d5947afb08d5a0804e3dd
```

## Evidence

_No evidence cited._
