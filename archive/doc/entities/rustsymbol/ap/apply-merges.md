# apply_merges (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- → dedup_relationships (`cef5cb16-665a-5648-bfaa-23750b333ccf`)
- ← SemanticCompilerPass::run (`c6428a28-088e-5899-b6d1-f734e97ebbbe`)

### Contains

- ← ekos/crates/semantic/src/lib.rs (`54021a72-4846-550c-960f-e63303e4d103`)

## Diagram

```mermaid
graph TD
    n0fe2ab6b10b650bfbe74c6ec8ad47104["apply_merges"]
    n54021a724846550c960fe63303e4d103["ekos/crates/semantic/src/lib.rs"]
    n54021a724846550c960fe63303e4d103 -->|Contains| n0fe2ab6b10b650bfbe74c6ec8ad47104
    ncef5cb16665a5648bfaa23750b333ccf["dedup_relationships"]
    n0fe2ab6b10b650bfbe74c6ec8ad47104 -->|Calls| ncef5cb16665a5648bfaa23750b333ccf
    nc6428a28088e5899b6d1f734e97ebbbe["SemanticCompilerPass::run"]
    nc6428a28088e5899b6d1f734e97ebbbe -->|Calls| n0fe2ab6b10b650bfbe74c6ec8ad47104
```

## Evidence

_No evidence cited._
