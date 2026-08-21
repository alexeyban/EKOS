# anyhow::Result (RustModule)

## Properties

_No compiled properties._

## Relationships

### DependsOn

- ← ekos/crates/cli/src/commands/store.rs (`ce2ed217-2b42-5760-9d2e-e2ca1574a517`)
- ← ekos/crates/cli/src/commands/compile.rs (`187a8810-a032-5178-ac4c-33a24e5cc42a`)
- ← ekos/crates/cli/src/commands/ledger.rs (`00bf5c8a-7198-5df3-a6eb-5bf22bc8ddcb`)
- ← ekos/crates/cli/src/commands/ekl.rs (`bc65014d-7e0a-5fdc-8a2b-fde4edce2935`)
- ← ekos/crates/cli/src/commands/artifact.rs (`06db81a6-f2c1-538b-bfce-452cf905f733`)
- ← tests/integration/tests/integration.rs (`23e9eb43-c8df-52b5-a581-f2efba7085ea`)
- ← ekos/crates/cli/src/commands/init.rs (`4ed5ca6d-a35f-5cde-a080-af09a742cfb9`)
- ← ekos/crates/cli/src/commands/dbt.rs (`d3579ceb-9751-53ad-b6be-693f17509a70`)
- ← ekos/crates/cli/src/commands/build.rs (`306def2e-bd5a-5784-9453-692c119e8d43`)
- ← ekos/crates/cli/src/commands/docs.rs (`5503d15b-112f-541f-8189-8be05a060beb`)
- ← ekos/crates/cli/src/commands/resolve.rs (`6b6902bf-7bb5-59f8-a210-ce0acd18d7ec`)
- ← ekos/crates/cli/src/commands/branch.rs (`8ae8543c-ebb4-545a-b5fe-5735e3953e88`)
- ← ekos/crates/cli/src/commands/doctor.rs (`117003db-05ca-5009-9ea3-90c845aff5f4`)
- ← ekos/crates/cli/src/commands/identity.rs (`f6e3418b-d664-536b-8a69-b723a534ff1a`)
- ← ekos/crates/cli/src/commands/diff.rs (`162a5e91-5e60-5951-9a1c-14c60cd6109b`)
- ← ekos/crates/cli/src/commands/query.rs (`76b10d14-834f-5bcb-8858-f46092b1989c`)
- ← ekos/crates/cli/src/commands/clean.rs (`0ccb7d47-8c4d-5547-bbd5-24f7701fb4e7`)
- ← ekos/crates/cli/src/commands/ask.rs (`e7e75efa-39cb-5182-b056-2fd16dfdc739`)
- ← ekos/crates/cli/src/commands/recover.rs (`7e02bcf9-a7b4-5099-8255-130d9ef401bb`)
- ← ekos/crates/cli/src/commands/commit.rs (`f48ae11b-a9a7-54f0-8cc6-a192b1641436`)
- ← ekos/crates/cli/src/commands/mcp.rs (`ff76bb73-9285-5295-927c-5cb33a5bbc25`)
- ← ekos/crates/cli/src/commands/marketing.rs (`e4550c2d-5dcf-5779-b25d-ac86e4019342`)
- ← ekos/crates/cli/src/bin/ekos.rs (`67ea4c4e-5e03-5c3a-8066-adf7aaed8a3e`)

## Diagram

```mermaid
graph TD
    n584318baec365ab8a4bfdef21bc540fe["anyhow::Result"]
    nce2ed2172b4257609d2ee2ca1574a517["ekos/crates/cli/src/commands/store.rs"]
    nce2ed2172b4257609d2ee2ca1574a517 -->|DependsOn| n584318baec365ab8a4bfdef21bc540fe
    n187a8810a0325178ac4c33a24e5cc42a["ekos/crates/cli/src/commands/compile.rs"]
    n187a8810a0325178ac4c33a24e5cc42a -->|DependsOn| n584318baec365ab8a4bfdef21bc540fe
    n00bf5c8a71985df3a6eb5bf22bc8ddcb["ekos/crates/cli/src/commands/ledger.rs"]
    n00bf5c8a71985df3a6eb5bf22bc8ddcb -->|DependsOn| n584318baec365ab8a4bfdef21bc540fe
    nbc65014d7e0a5fdc8a2bfde4edce2935["ekos/crates/cli/src/commands/ekl.rs"]
    nbc65014d7e0a5fdc8a2bfde4edce2935 -->|DependsOn| n584318baec365ab8a4bfdef21bc540fe
    n06db81a6f2c1538bbfce452cf905f733["ekos/crates/cli/src/commands/artifact.rs"]
    n06db81a6f2c1538bbfce452cf905f733 -->|DependsOn| n584318baec365ab8a4bfdef21bc540fe
    n23e9eb43c8df52b5a581f2efba7085ea["tests/integration/tests/integration.rs"]
    n23e9eb43c8df52b5a581f2efba7085ea -->|DependsOn| n584318baec365ab8a4bfdef21bc540fe
    n4ed5ca6da35f5cdea080af09a742cfb9["ekos/crates/cli/src/commands/init.rs"]
    n4ed5ca6da35f5cdea080af09a742cfb9 -->|DependsOn| n584318baec365ab8a4bfdef21bc540fe
    nd3579ceb975153adb6be693f17509a70["ekos/crates/cli/src/commands/dbt.rs"]
    nd3579ceb975153adb6be693f17509a70 -->|DependsOn| n584318baec365ab8a4bfdef21bc540fe
    n306def2ebd5a57849453692c119e8d43["ekos/crates/cli/src/commands/build.rs"]
    n306def2ebd5a57849453692c119e8d43 -->|DependsOn| n584318baec365ab8a4bfdef21bc540fe
    n5503d15b112f541f81898be05a060beb["ekos/crates/cli/src/commands/docs.rs"]
    n5503d15b112f541f81898be05a060beb -->|DependsOn| n584318baec365ab8a4bfdef21bc540fe
    n6b6902bf7bb559f8a210ce0acd18d7ec["ekos/crates/cli/src/commands/resolve.rs"]
    n6b6902bf7bb559f8a210ce0acd18d7ec -->|DependsOn| n584318baec365ab8a4bfdef21bc540fe
    n8ae8543cebb4545ab5fe5735e3953e88["ekos/crates/cli/src/commands/branch.rs"]
    n8ae8543cebb4545ab5fe5735e3953e88 -->|DependsOn| n584318baec365ab8a4bfdef21bc540fe
    n117003db05ca50099ea390c845aff5f4["ekos/crates/cli/src/commands/doctor.rs"]
    n117003db05ca50099ea390c845aff5f4 -->|DependsOn| n584318baec365ab8a4bfdef21bc540fe
    nf6e3418bd664536b8a69b723a534ff1a["ekos/crates/cli/src/commands/identity.rs"]
    nf6e3418bd664536b8a69b723a534ff1a -->|DependsOn| n584318baec365ab8a4bfdef21bc540fe
    n162a5e915e6059519a1c14c60cd6109b["ekos/crates/cli/src/commands/diff.rs"]
    n162a5e915e6059519a1c14c60cd6109b -->|DependsOn| n584318baec365ab8a4bfdef21bc540fe
    n76b10d14834f5bcb8858f46092b1989c["ekos/crates/cli/src/commands/query.rs"]
    n76b10d14834f5bcb8858f46092b1989c -->|DependsOn| n584318baec365ab8a4bfdef21bc540fe
    n0ccb7d478c4d5547bbd524f7701fb4e7["ekos/crates/cli/src/commands/clean.rs"]
    n0ccb7d478c4d5547bbd524f7701fb4e7 -->|DependsOn| n584318baec365ab8a4bfdef21bc540fe
    ne7e75efa39cb5182b0562fd16dfdc739["ekos/crates/cli/src/commands/ask.rs"]
    ne7e75efa39cb5182b0562fd16dfdc739 -->|DependsOn| n584318baec365ab8a4bfdef21bc540fe
    n7e02bcf9a7b450998255130d9ef401bb["ekos/crates/cli/src/commands/recover.rs"]
    n7e02bcf9a7b450998255130d9ef401bb -->|DependsOn| n584318baec365ab8a4bfdef21bc540fe
    nf48ae11ba9a754f08cc6a192b1641436["ekos/crates/cli/src/commands/commit.rs"]
    nf48ae11ba9a754f08cc6a192b1641436 -->|DependsOn| n584318baec365ab8a4bfdef21bc540fe
    nff76bb7392855295927c5cb33a5bbc25["ekos/crates/cli/src/commands/mcp.rs"]
    nff76bb7392855295927c5cb33a5bbc25 -->|DependsOn| n584318baec365ab8a4bfdef21bc540fe
    ne4550c2d5dcf5779b25dac86e4019342["ekos/crates/cli/src/commands/marketing.rs"]
    ne4550c2d5dcf5779b25dac86e4019342 -->|DependsOn| n584318baec365ab8a4bfdef21bc540fe
    n67ea4c4e5e035c3a8066adf7aaed8a3e["ekos/crates/cli/src/bin/ekos.rs"]
    n67ea4c4e5e035c3a8066adf7aaed8a3e -->|DependsOn| n584318baec365ab8a4bfdef21bc540fe
```

## Evidence

_No evidence cited._
