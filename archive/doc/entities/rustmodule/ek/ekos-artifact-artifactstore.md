# ekos_artifact::ArtifactStore (RustModule)

## Properties

_No compiled properties._

## Relationships

### DependsOn

- ← ekos/crates/cli/src/commands/compile.rs (`187a8810-a032-5178-ac4c-33a24e5cc42a`)
- ← ekos/crates/compiler-core/src/pass.rs (`5189d0a2-3e2b-529e-adbf-3984a77be404`)
- ← ekos/crates/cli/src/commands/build.rs (`306def2e-bd5a-5784-9453-692c119e8d43`)
- ← ekos/crates/cli/src/commands/resolve.rs (`6b6902bf-7bb5-59f8-a210-ce0acd18d7ec`)
- ← ekos/crates/cli/src/commands/recover.rs (`7e02bcf9-a7b4-5099-8255-130d9ef401bb`)

## Diagram

```mermaid
graph TD
    n8e9de7b567f45d98a49cae490aadb32e["ekos_artifact::ArtifactStore"]
    n187a8810a0325178ac4c33a24e5cc42a["ekos/crates/cli/src/commands/compile.rs"]
    n187a8810a0325178ac4c33a24e5cc42a -->|DependsOn| n8e9de7b567f45d98a49cae490aadb32e
    n5189d0a23e2b529eadbf3984a77be404["ekos/crates/compiler-core/src/pass.rs"]
    n5189d0a23e2b529eadbf3984a77be404 -->|DependsOn| n8e9de7b567f45d98a49cae490aadb32e
    n306def2ebd5a57849453692c119e8d43["ekos/crates/cli/src/commands/build.rs"]
    n306def2ebd5a57849453692c119e8d43 -->|DependsOn| n8e9de7b567f45d98a49cae490aadb32e
    n6b6902bf7bb559f8a210ce0acd18d7ec["ekos/crates/cli/src/commands/resolve.rs"]
    n6b6902bf7bb559f8a210ce0acd18d7ec -->|DependsOn| n8e9de7b567f45d98a49cae490aadb32e
    n7e02bcf9a7b450998255130d9ef401bb["ekos/crates/cli/src/commands/recover.rs"]
    n7e02bcf9a7b450998255130d9ef401bb -->|DependsOn| n8e9de7b567f45d98a49cae490aadb32e
```

## Evidence

_No evidence cited._
