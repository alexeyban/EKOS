# std::sync::Arc (RustModule)

## Properties

_No compiled properties._

## Relationships

### DependsOn

- ← ekos/plugins/github/src/lib.rs (`85954c06-2c57-57ee-8a60-8a92c239ab70`)
- ← ekos/crates/recovery/src/python_analyzer.rs (`196ca3fd-e85c-5ab9-a142-50f63dc586b9`)
- ← ekos/crates/cli/src/commands/compile.rs (`187a8810-a032-5178-ac4c-33a24e5cc42a`)
- ← ekos/crates/compiler-core/src/pass.rs (`5189d0a2-3e2b-529e-adbf-3984a77be404`)
- ← ekos/plugins/salesforce/src/lib.rs (`62e147c2-5a91-534c-98ac-6557560db8f6`)
- ← ekos/plugins/confluence/src/lib.rs (`b78f02e6-8e4d-58de-abb4-ed29b9688c5f`)
- ← ekos/crates/compiler-core/src/compiler.rs (`3b7209fe-32c4-588e-b8b5-5fa2165ff88b`)
- ← ekos/crates/recovery/src/pentaho_analyzer.rs (`ce3d2f1b-e1c6-55d7-92bc-c1b69f76dbca`)
- ← ekos/crates/cli/src/commands/build.rs (`306def2e-bd5a-5784-9453-692c119e8d43`)
- ← ekos/crates/recovery/src/sql_analyzer.rs (`cd768c5e-1640-51c3-b6ec-cb7ac78ade6d`)
- ← ekos/plugins/sap/src/lib.rs (`8ce136d7-2eb9-53fd-90c2-7d0d00aeb27d`)
- ← ekos/plugins/fabric/src/lib.rs (`ea3988ed-2565-56db-b0cf-09b6d4594525`)
- ← ekos/crates/recovery/src/rust_analyzer.rs (`50cde56d-1f82-53a6-bc71-b5b2f7c711bc`)
- ← ekos/crates/runtime/src/ai.rs (`e85e734d-ef58-5185-835a-34896d2da3f1`)
- ← ekos/plugins/crypto/src/lib.rs (`83728c61-df4d-5ad6-81c7-5bf50ff761fb`)
- ← ekos/crates/recovery/src/sql_transform_analyzer.rs (`987e3fbb-31ec-576d-b794-7e801336e8c8`)
- ← ekos/plugins/oracle/src/lib.rs (`affbb6cf-64eb-5dd5-805d-01cf2bd08c7f`)
- ← ekos/crates/cli/src/commands/recover.rs (`7e02bcf9-a7b4-5099-8255-130d9ef401bb`)
- ← ekos/plugins/snowflake/src/lib.rs (`1f09ad6b-972d-56d7-9597-8641b250abce`)
- ← ekos/crates/cli/src/commands/marketing.rs (`e4550c2d-5dcf-5779-b25d-ac86e4019342`)
- ← ekos/plugins/localdocs/src/lib.rs (`1ed81bd1-9be1-5cda-9158-9ad4f1980e3d`)
- ← ekos/crates/recovery/src/document_semantics_analyzer.rs (`62e92526-f096-5ed2-bc72-0bdae8703aa3`)

## Diagram

```mermaid
graph TD
    n24c9b1b50fcc57c5a21060f98e10aef0["std::sync::Arc"]
    n85954c062c5757ee8a608a92c239ab70["ekos/plugins/github/src/lib.rs"]
    n85954c062c5757ee8a608a92c239ab70 -->|DependsOn| n24c9b1b50fcc57c5a21060f98e10aef0
    n196ca3fde85c5ab9a14250f63dc586b9["ekos/crates/recovery/src/python_analyzer.rs"]
    n196ca3fde85c5ab9a14250f63dc586b9 -->|DependsOn| n24c9b1b50fcc57c5a21060f98e10aef0
    n187a8810a0325178ac4c33a24e5cc42a["ekos/crates/cli/src/commands/compile.rs"]
    n187a8810a0325178ac4c33a24e5cc42a -->|DependsOn| n24c9b1b50fcc57c5a21060f98e10aef0
    n5189d0a23e2b529eadbf3984a77be404["ekos/crates/compiler-core/src/pass.rs"]
    n5189d0a23e2b529eadbf3984a77be404 -->|DependsOn| n24c9b1b50fcc57c5a21060f98e10aef0
    n62e147c25a91534c98ac6557560db8f6["ekos/plugins/salesforce/src/lib.rs"]
    n62e147c25a91534c98ac6557560db8f6 -->|DependsOn| n24c9b1b50fcc57c5a21060f98e10aef0
    nb78f02e68e4d58deabb4ed29b9688c5f["ekos/plugins/confluence/src/lib.rs"]
    nb78f02e68e4d58deabb4ed29b9688c5f -->|DependsOn| n24c9b1b50fcc57c5a21060f98e10aef0
    n3b7209fe32c4588eb8b55fa2165ff88b["ekos/crates/compiler-core/src/compiler.rs"]
    n3b7209fe32c4588eb8b55fa2165ff88b -->|DependsOn| n24c9b1b50fcc57c5a21060f98e10aef0
    nce3d2f1be1c655d792bcc1b69f76dbca["ekos/crates/recovery/src/pentaho_analyzer.rs"]
    nce3d2f1be1c655d792bcc1b69f76dbca -->|DependsOn| n24c9b1b50fcc57c5a21060f98e10aef0
    n306def2ebd5a57849453692c119e8d43["ekos/crates/cli/src/commands/build.rs"]
    n306def2ebd5a57849453692c119e8d43 -->|DependsOn| n24c9b1b50fcc57c5a21060f98e10aef0
    ncd768c5e164051c3b6eccb7ac78ade6d["ekos/crates/recovery/src/sql_analyzer.rs"]
    ncd768c5e164051c3b6eccb7ac78ade6d -->|DependsOn| n24c9b1b50fcc57c5a21060f98e10aef0
    n8ce136d72eb953fd90c27d0d00aeb27d["ekos/plugins/sap/src/lib.rs"]
    n8ce136d72eb953fd90c27d0d00aeb27d -->|DependsOn| n24c9b1b50fcc57c5a21060f98e10aef0
    nea3988ed256556dbb0cf09b6d4594525["ekos/plugins/fabric/src/lib.rs"]
    nea3988ed256556dbb0cf09b6d4594525 -->|DependsOn| n24c9b1b50fcc57c5a21060f98e10aef0
    n50cde56d1f8253a6bc71b5b2f7c711bc["ekos/crates/recovery/src/rust_analyzer.rs"]
    n50cde56d1f8253a6bc71b5b2f7c711bc -->|DependsOn| n24c9b1b50fcc57c5a21060f98e10aef0
    ne85e734def585185835a34896d2da3f1["ekos/crates/runtime/src/ai.rs"]
    ne85e734def585185835a34896d2da3f1 -->|DependsOn| n24c9b1b50fcc57c5a21060f98e10aef0
    n83728c61df4d5ad681c75bf50ff761fb["ekos/plugins/crypto/src/lib.rs"]
    n83728c61df4d5ad681c75bf50ff761fb -->|DependsOn| n24c9b1b50fcc57c5a21060f98e10aef0
    n987e3fbb31ec576db7947e801336e8c8["ekos/crates/recovery/src/sql_transform_analyzer.rs"]
    n987e3fbb31ec576db7947e801336e8c8 -->|DependsOn| n24c9b1b50fcc57c5a21060f98e10aef0
    naffbb6cf64eb5dd5805d01cf2bd08c7f["ekos/plugins/oracle/src/lib.rs"]
    naffbb6cf64eb5dd5805d01cf2bd08c7f -->|DependsOn| n24c9b1b50fcc57c5a21060f98e10aef0
    n7e02bcf9a7b450998255130d9ef401bb["ekos/crates/cli/src/commands/recover.rs"]
    n7e02bcf9a7b450998255130d9ef401bb -->|DependsOn| n24c9b1b50fcc57c5a21060f98e10aef0
    n1f09ad6b972d56d795978641b250abce["ekos/plugins/snowflake/src/lib.rs"]
    n1f09ad6b972d56d795978641b250abce -->|DependsOn| n24c9b1b50fcc57c5a21060f98e10aef0
    ne4550c2d5dcf5779b25dac86e4019342["ekos/crates/cli/src/commands/marketing.rs"]
    ne4550c2d5dcf5779b25dac86e4019342 -->|DependsOn| n24c9b1b50fcc57c5a21060f98e10aef0
    n1ed81bd19be15cda91589ad4f1980e3d["ekos/plugins/localdocs/src/lib.rs"]
    n1ed81bd19be15cda91589ad4f1980e3d -->|DependsOn| n24c9b1b50fcc57c5a21060f98e10aef0
    n62e92526f0965ed2bc720bdae8703aa3["ekos/crates/recovery/src/document_semantics_analyzer.rs"]
    n62e92526f0965ed2bc720bdae8703aa3 -->|DependsOn| n24c9b1b50fcc57c5a21060f98e10aef0
```

## Evidence

_No evidence cited._
