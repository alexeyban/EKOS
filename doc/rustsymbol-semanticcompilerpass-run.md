# SemanticCompilerPass::run (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → CkModel::validate (`de0b1feb-df36-5b3d-89ad-69c32f6bb7c2`)
- → build_ckm (`2629dd98-2de4-577c-9729-5674e1bef671`)
- → merge_graphs (`af354743-f94c-501c-8907-0a51ccdae017`)
- → apply_merges (`0fe2ab6b-10b6-50bf-be74-c6ec8ad47104`)

### Contains

- ← ekos/crates/semantic/src/lib.rs (`54021a72-4846-550c-960f-e63303e4d103`)

## Diagram

```mermaid
graph TD
    nc6428a28088e5899b6d1f734e97ebbbe["SemanticCompilerPass::run"]
    n54021a724846550c960fe63303e4d103["ekos/crates/semantic/src/lib.rs"]
    n54021a724846550c960fe63303e4d103 -->|Contains| nc6428a28088e5899b6d1f734e97ebbbe
    nde0b1febdf365b3d89ad69c32f6bb7c2["CkModel::validate"]
    nc6428a28088e5899b6d1f734e97ebbbe -->|Calls| nde0b1febdf365b3d89ad69c32f6bb7c2
    n2629dd982de4577c97295674e1bef671["build_ckm"]
    nc6428a28088e5899b6d1f734e97ebbbe -->|Calls| n2629dd982de4577c97295674e1bef671
    naf354743f94c501c89070a51ccdae017["merge_graphs"]
    nc6428a28088e5899b6d1f734e97ebbbe -->|Calls| naf354743f94c501c89070a51ccdae017
    n0fe2ab6b10b650bfbe74c6ec8ad47104["apply_merges"]
    nc6428a28088e5899b6d1f734e97ebbbe -->|Calls| n0fe2ab6b10b650bfbe74c6ec8ad47104
```

## Evidence

_No evidence cited._
