# thiserror::Error (RustModule)

## Properties

_No compiled properties._

## Relationships

### DependsOn

- ← ekos/plugins/github/src/lib.rs (`85954c06-2c57-57ee-8a60-8a92c239ab70`)
- ← ekos/crates/compiler-core/src/pass.rs (`5189d0a2-3e2b-529e-adbf-3984a77be404`)
- ← ekos/crates/artifact/src/store.rs (`d997f78d-b111-570e-b530-510e98c14df8`)
- ← ekos/plugins/salesforce/src/lib.rs (`62e147c2-5a91-534c-98ac-6557560db8f6`)
- ← ekos/crates/ledger/src/segment/mod.rs (`80a64e0a-b0ac-51df-b3be-128cddd1cc83`)
- ← ekos/plugins/confluence/src/lib.rs (`b78f02e6-8e4d-58de-abb4-ed29b9688c5f`)
- ← ekos/crates/compiler-core/src/compiler.rs (`3b7209fe-32c4-588e-b8b5-5fa2165ff88b`)
- ← ekos/crates/recovery/src/llm.rs (`61d089be-a24a-5745-8bca-67d1d16373ca`)
- ← ekos/crates/ledger/src/lib.rs (`21938f45-b767-5933-9e71-12e15ff53eb1`)
- ← ekos/crates/marketing/src/publisher.rs (`3fab908f-efe6-5e1f-940b-5a16e3b7c774`)
- ← ekos/plugins/sap/src/lib.rs (`8ce136d7-2eb9-53fd-90c2-7d0d00aeb27d`)
- ← ekos/plugins/fabric/src/lib.rs (`ea3988ed-2565-56db-b0cf-09b6d4594525`)
- ← ekos/crates/marketing/src/tweet.rs (`3372ee6e-2a1d-50b2-a3ae-b17eb421301b`)
- ← ekos/crates/ekl/src/interpreter.rs (`9c2cb6e4-ee09-503f-8cf5-ccfaf23ecd79`)
- ← ekos/crates/runtime/src/ai.rs (`e85e734d-ef58-5185-835a-34896d2da3f1`)
- ← ekos/plugins/crypto/src/lib.rs (`83728c61-df4d-5ad6-81c7-5bf50ff761fb`)
- ← ekos/crates/ledger/src/fact.rs (`7837f9fa-7178-5151-a068-e75361336c37`)
- ← ekos/crates/marketing/src/store.rs (`87860e47-e5db-5450-a233-7e7fe0c46d89`)
- ← ekos/crates/observation-sdk/src/lib.rs (`66ce958b-7250-5ec2-954e-eacf8f64aae0`)
- ← ekos/plugins/oracle/src/lib.rs (`affbb6cf-64eb-5dd5-805d-01cf2bd08c7f`)
- ← ekos/plugins/snowflake/src/lib.rs (`1f09ad6b-972d-56d7-9597-8641b250abce`)
- ← ekos/crates/marketing/src/devlog.rs (`4ca01e5b-d21d-5312-ac99-c6aa65d7d8d0`)
- ← ekos/plugins/localdocs/src/lib.rs (`1ed81bd1-9be1-5cda-9158-9ad4f1980e3d`)
- ← ekos/crates/runtime/src/lib.rs (`7d8cf6bd-fac1-56d9-a742-4db7a455ab7c`)

## Diagram

```mermaid
graph TD
    nb15859146ac4553b81ec56f595c2e6ee["thiserror::Error"]
    n85954c062c5757ee8a608a92c239ab70["ekos/plugins/github/src/lib.rs"]
    n85954c062c5757ee8a608a92c239ab70 -->|DependsOn| nb15859146ac4553b81ec56f595c2e6ee
    n5189d0a23e2b529eadbf3984a77be404["ekos/crates/compiler-core/src/pass.rs"]
    n5189d0a23e2b529eadbf3984a77be404 -->|DependsOn| nb15859146ac4553b81ec56f595c2e6ee
    nd997f78db111570eb530510e98c14df8["ekos/crates/artifact/src/store.rs"]
    nd997f78db111570eb530510e98c14df8 -->|DependsOn| nb15859146ac4553b81ec56f595c2e6ee
    n62e147c25a91534c98ac6557560db8f6["ekos/plugins/salesforce/src/lib.rs"]
    n62e147c25a91534c98ac6557560db8f6 -->|DependsOn| nb15859146ac4553b81ec56f595c2e6ee
    n80a64e0ab0ac51dfb3be128cddd1cc83["ekos/crates/ledger/src/segment/mod.rs"]
    n80a64e0ab0ac51dfb3be128cddd1cc83 -->|DependsOn| nb15859146ac4553b81ec56f595c2e6ee
    nb78f02e68e4d58deabb4ed29b9688c5f["ekos/plugins/confluence/src/lib.rs"]
    nb78f02e68e4d58deabb4ed29b9688c5f -->|DependsOn| nb15859146ac4553b81ec56f595c2e6ee
    n3b7209fe32c4588eb8b55fa2165ff88b["ekos/crates/compiler-core/src/compiler.rs"]
    n3b7209fe32c4588eb8b55fa2165ff88b -->|DependsOn| nb15859146ac4553b81ec56f595c2e6ee
    n61d089bea24a57458bca67d1d16373ca["ekos/crates/recovery/src/llm.rs"]
    n61d089bea24a57458bca67d1d16373ca -->|DependsOn| nb15859146ac4553b81ec56f595c2e6ee
    n21938f45b76759339e7112e15ff53eb1["ekos/crates/ledger/src/lib.rs"]
    n21938f45b76759339e7112e15ff53eb1 -->|DependsOn| nb15859146ac4553b81ec56f595c2e6ee
    n3fab908fefe65e1f940b5a16e3b7c774["ekos/crates/marketing/src/publisher.rs"]
    n3fab908fefe65e1f940b5a16e3b7c774 -->|DependsOn| nb15859146ac4553b81ec56f595c2e6ee
    n8ce136d72eb953fd90c27d0d00aeb27d["ekos/plugins/sap/src/lib.rs"]
    n8ce136d72eb953fd90c27d0d00aeb27d -->|DependsOn| nb15859146ac4553b81ec56f595c2e6ee
    nea3988ed256556dbb0cf09b6d4594525["ekos/plugins/fabric/src/lib.rs"]
    nea3988ed256556dbb0cf09b6d4594525 -->|DependsOn| nb15859146ac4553b81ec56f595c2e6ee
    n3372ee6e2a1d50b2a3aeb17eb421301b["ekos/crates/marketing/src/tweet.rs"]
    n3372ee6e2a1d50b2a3aeb17eb421301b -->|DependsOn| nb15859146ac4553b81ec56f595c2e6ee
    n9c2cb6e4ee09503f8cf5ccfaf23ecd79["ekos/crates/ekl/src/interpreter.rs"]
    n9c2cb6e4ee09503f8cf5ccfaf23ecd79 -->|DependsOn| nb15859146ac4553b81ec56f595c2e6ee
    ne85e734def585185835a34896d2da3f1["ekos/crates/runtime/src/ai.rs"]
    ne85e734def585185835a34896d2da3f1 -->|DependsOn| nb15859146ac4553b81ec56f595c2e6ee
    n83728c61df4d5ad681c75bf50ff761fb["ekos/plugins/crypto/src/lib.rs"]
    n83728c61df4d5ad681c75bf50ff761fb -->|DependsOn| nb15859146ac4553b81ec56f595c2e6ee
    n7837f9fa71785151a068e75361336c37["ekos/crates/ledger/src/fact.rs"]
    n7837f9fa71785151a068e75361336c37 -->|DependsOn| nb15859146ac4553b81ec56f595c2e6ee
    n87860e47e5db5450a2337e7fe0c46d89["ekos/crates/marketing/src/store.rs"]
    n87860e47e5db5450a2337e7fe0c46d89 -->|DependsOn| nb15859146ac4553b81ec56f595c2e6ee
    n66ce958b72505ec2954eeacf8f64aae0["ekos/crates/observation-sdk/src/lib.rs"]
    n66ce958b72505ec2954eeacf8f64aae0 -->|DependsOn| nb15859146ac4553b81ec56f595c2e6ee
    naffbb6cf64eb5dd5805d01cf2bd08c7f["ekos/plugins/oracle/src/lib.rs"]
    naffbb6cf64eb5dd5805d01cf2bd08c7f -->|DependsOn| nb15859146ac4553b81ec56f595c2e6ee
    n1f09ad6b972d56d795978641b250abce["ekos/plugins/snowflake/src/lib.rs"]
    n1f09ad6b972d56d795978641b250abce -->|DependsOn| nb15859146ac4553b81ec56f595c2e6ee
    n4ca01e5bd21d5312ac99c6aa65d7d8d0["ekos/crates/marketing/src/devlog.rs"]
    n4ca01e5bd21d5312ac99c6aa65d7d8d0 -->|DependsOn| nb15859146ac4553b81ec56f595c2e6ee
    n1ed81bd19be15cda91589ad4f1980e3d["ekos/plugins/localdocs/src/lib.rs"]
    n1ed81bd19be15cda91589ad4f1980e3d -->|DependsOn| nb15859146ac4553b81ec56f595c2e6ee
    n7d8cf6bdfac156d9a7424db7a455ab7c["ekos/crates/runtime/src/lib.rs"]
    n7d8cf6bdfac156d9a7424db7a455ab7c -->|DependsOn| nb15859146ac4553b81ec56f595c2e6ee
```

## Evidence

_No evidence cited._
