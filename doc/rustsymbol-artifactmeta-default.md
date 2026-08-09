# ArtifactMeta::default (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → ArtifactMeta::new (`6db558a3-50a3-5ee5-ab5b-f5a80e5f5e63`)
- ← ObservationArtifact::new (`d3b06dcb-6b00-578c-bb03-2ed93735d399`)
- ← KnowledgeArtifact::new (`6a696ea8-bbb4-57e2-8ac4-cd7e7cf5c452`)
- ← EvidenceArtifact::new (`4b6ad777-17f7-5a0d-9b47-e2a5fc581d02`)
- ← DiagnosticArtifact::new (`3529a593-008c-5745-a14d-524906d6cbd2`)
- ← IndexArtifact::new (`08ac6aa9-19bb-50b5-9291-fc045349dd29`)

### Contains

- ← ekos/crates/artifact/src/lib.rs (`918532b1-7390-5128-8de5-faf4f7a91daf`)

## Diagram

```mermaid
graph TD
    n75608e6dce35532197aff8b9d4f486b2["ArtifactMeta::default"]
    n918532b1739051288de5faf4f7a91daf["ekos/crates/artifact/src/lib.rs"]
    n918532b1739051288de5faf4f7a91daf -->|Contains| n75608e6dce35532197aff8b9d4f486b2
    n6db558a350a35ee5ab5bf5a80e5f5e63["ArtifactMeta::new"]
    n75608e6dce35532197aff8b9d4f486b2 -->|Calls| n6db558a350a35ee5ab5bf5a80e5f5e63
    nd3b06dcb6b00578cbb032ed93735d399["ObservationArtifact::new"]
    nd3b06dcb6b00578cbb032ed93735d399 -->|Calls| n75608e6dce35532197aff8b9d4f486b2
    n6a696ea8bbb457e28ac4cd7e7cf5c452["KnowledgeArtifact::new"]
    n6a696ea8bbb457e28ac4cd7e7cf5c452 -->|Calls| n75608e6dce35532197aff8b9d4f486b2
    n4b6ad77717f75a0d9b47e2a5fc581d02["EvidenceArtifact::new"]
    n4b6ad77717f75a0d9b47e2a5fc581d02 -->|Calls| n75608e6dce35532197aff8b9d4f486b2
    n3529a593008c5745a14d524906d6cbd2["DiagnosticArtifact::new"]
    n3529a593008c5745a14d524906d6cbd2 -->|Calls| n75608e6dce35532197aff8b9d4f486b2
    n08ac6aa919bb50b59291fc045349dd29["IndexArtifact::new"]
    n08ac6aa919bb50b59291fc045349dd29 -->|Calls| n75608e6dce35532197aff8b9d4f486b2
```

## Evidence

_No evidence cited._
