# serde::Serialize (RustModule)

## Properties

_No compiled properties._

## Relationships

### DependsOn

- ← ekos/plugins/github/src/lib.rs (`85954c06-2c57-57ee-8a60-8a92c239ab70`)
- ← ekos/plugins/salesforce/src/lib.rs (`62e147c2-5a91-534c-98ac-6557560db8f6`)
- ← ekos/crates/ledger/src/segment/mod.rs (`80a64e0a-b0ac-51df-b3be-128cddd1cc83`)
- ← ekos/plugins/confluence/src/lib.rs (`b78f02e6-8e4d-58de-abb4-ed29b9688c5f`)
- ← ekos/crates/kir/src/lib.rs (`078a10d5-8141-5e74-b89d-7120ec1be4f8`)
- ← ekos/crates/semantic/src/transform_ir.rs (`b4fdd24c-8184-5879-9136-f0a70208955e`)
- ← ekos/crates/recovery/src/llm.rs (`61d089be-a24a-5745-8bca-67d1d16373ca`)
- ← ekos/crates/ledger/src/lib.rs (`21938f45-b767-5933-9e71-12e15ff53eb1`)
- ← ekos/crates/identity/src/cross_system.rs (`44d130a8-ca02-506d-bc1a-21b037fb492c`)
- ← ekos/plugins/sap/src/lib.rs (`8ce136d7-2eb9-53fd-90c2-7d0d00aeb27d`)
- ← ekos/crates/recovery/src/ollama.rs (`952b3eaf-406f-5c22-b538-f5c2d5fbe2f9`)
- ← ekos/plugins/fabric/src/lib.rs (`ea3988ed-2565-56db-b0cf-09b6d4594525`)
- ← ekos/crates/semantic/src/lib.rs (`54021a72-4846-550c-960f-e63303e4d103`)
- ← ekos/crates/identity/src/lib.rs (`c958282a-6d42-50ab-9cf9-8533976f0820`)
- ← ekos/crates/compiler-core/src/cache.rs (`01ec80b2-6c80-5000-979c-acb288ff920a`)
- ← ekos/crates/runtime/src/ai.rs (`e85e734d-ef58-5185-835a-34896d2da3f1`)
- ← ekos/plugins/crypto/src/lib.rs (`83728c61-df4d-5ad6-81c7-5bf50ff761fb`)
- ← ekos/crates/recovery/src/anthropic.rs (`2b1d458b-2cbb-5b8a-9932-9c15c981a99e`)
- ← ekos/crates/ledger/src/fact.rs (`7837f9fa-7178-5151-a068-e75361336c37`)
- ← ekos/crates/compiler-core/src/config.rs (`d0747f34-25e1-5426-80eb-a54fffcac598`)
- ← ekos/crates/ledger/src/index.rs (`a43fa387-2cac-5166-bfc2-ae4c965fc2ac`)
- ← ekos/crates/marketing/src/store.rs (`87860e47-e5db-5450-a233-7e7fe0c46d89`)
- ← ekos/plugins/oracle/src/lib.rs (`affbb6cf-64eb-5dd5-805d-01cf2bd08c7f`)
- ← ekos/crates/common/src/compress.rs (`99637da4-0489-5fca-ba15-b1144f48c3cc`)
- ← ekos/plugins/snowflake/src/lib.rs (`1f09ad6b-972d-56d7-9597-8641b250abce`)
- ← ekos/crates/compiler-core/src/diagnostics.rs (`e5b5b2e0-3763-5cb8-a914-f113bb9e3ac4`)
- ← ekos/crates/artifact/src/lib.rs (`918532b1-7390-5128-8de5-faf4f7a91daf`)
- ← ekos/crates/runtime/src/lib.rs (`7d8cf6bd-fac1-56d9-a742-4db7a455ab7c`)

## Diagram

```mermaid
graph TD
    n02602034970e54a986c7bd69593368ee["serde::Serialize"]
    n85954c062c5757ee8a608a92c239ab70["ekos/plugins/github/src/lib.rs"]
    n85954c062c5757ee8a608a92c239ab70 -->|DependsOn| n02602034970e54a986c7bd69593368ee
    n62e147c25a91534c98ac6557560db8f6["ekos/plugins/salesforce/src/lib.rs"]
    n62e147c25a91534c98ac6557560db8f6 -->|DependsOn| n02602034970e54a986c7bd69593368ee
    n80a64e0ab0ac51dfb3be128cddd1cc83["ekos/crates/ledger/src/segment/mod.rs"]
    n80a64e0ab0ac51dfb3be128cddd1cc83 -->|DependsOn| n02602034970e54a986c7bd69593368ee
    nb78f02e68e4d58deabb4ed29b9688c5f["ekos/plugins/confluence/src/lib.rs"]
    nb78f02e68e4d58deabb4ed29b9688c5f -->|DependsOn| n02602034970e54a986c7bd69593368ee
    n078a10d581415e74b89d7120ec1be4f8["ekos/crates/kir/src/lib.rs"]
    n078a10d581415e74b89d7120ec1be4f8 -->|DependsOn| n02602034970e54a986c7bd69593368ee
    nb4fdd24c818458799136f0a70208955e["ekos/crates/semantic/src/transform_ir.rs"]
    nb4fdd24c818458799136f0a70208955e -->|DependsOn| n02602034970e54a986c7bd69593368ee
    n61d089bea24a57458bca67d1d16373ca["ekos/crates/recovery/src/llm.rs"]
    n61d089bea24a57458bca67d1d16373ca -->|DependsOn| n02602034970e54a986c7bd69593368ee
    n21938f45b76759339e7112e15ff53eb1["ekos/crates/ledger/src/lib.rs"]
    n21938f45b76759339e7112e15ff53eb1 -->|DependsOn| n02602034970e54a986c7bd69593368ee
    n44d130a8ca02506dbc1a21b037fb492c["ekos/crates/identity/src/cross_system.rs"]
    n44d130a8ca02506dbc1a21b037fb492c -->|DependsOn| n02602034970e54a986c7bd69593368ee
    n8ce136d72eb953fd90c27d0d00aeb27d["ekos/plugins/sap/src/lib.rs"]
    n8ce136d72eb953fd90c27d0d00aeb27d -->|DependsOn| n02602034970e54a986c7bd69593368ee
    n952b3eaf406f5c22b538f5c2d5fbe2f9["ekos/crates/recovery/src/ollama.rs"]
    n952b3eaf406f5c22b538f5c2d5fbe2f9 -->|DependsOn| n02602034970e54a986c7bd69593368ee
    nea3988ed256556dbb0cf09b6d4594525["ekos/plugins/fabric/src/lib.rs"]
    nea3988ed256556dbb0cf09b6d4594525 -->|DependsOn| n02602034970e54a986c7bd69593368ee
    n54021a724846550c960fe63303e4d103["ekos/crates/semantic/src/lib.rs"]
    n54021a724846550c960fe63303e4d103 -->|DependsOn| n02602034970e54a986c7bd69593368ee
    nc958282a6d4250ab9cf98533976f0820["ekos/crates/identity/src/lib.rs"]
    nc958282a6d4250ab9cf98533976f0820 -->|DependsOn| n02602034970e54a986c7bd69593368ee
    n01ec80b26c805000979cacb288ff920a["ekos/crates/compiler-core/src/cache.rs"]
    n01ec80b26c805000979cacb288ff920a -->|DependsOn| n02602034970e54a986c7bd69593368ee
    ne85e734def585185835a34896d2da3f1["ekos/crates/runtime/src/ai.rs"]
    ne85e734def585185835a34896d2da3f1 -->|DependsOn| n02602034970e54a986c7bd69593368ee
    n83728c61df4d5ad681c75bf50ff761fb["ekos/plugins/crypto/src/lib.rs"]
    n83728c61df4d5ad681c75bf50ff761fb -->|DependsOn| n02602034970e54a986c7bd69593368ee
    n2b1d458b2cbb5b8a99329c15c981a99e["ekos/crates/recovery/src/anthropic.rs"]
    n2b1d458b2cbb5b8a99329c15c981a99e -->|DependsOn| n02602034970e54a986c7bd69593368ee
    n7837f9fa71785151a068e75361336c37["ekos/crates/ledger/src/fact.rs"]
    n7837f9fa71785151a068e75361336c37 -->|DependsOn| n02602034970e54a986c7bd69593368ee
    nd0747f3425e1542680eba54fffcac598["ekos/crates/compiler-core/src/config.rs"]
    nd0747f3425e1542680eba54fffcac598 -->|DependsOn| n02602034970e54a986c7bd69593368ee
    na43fa3872cac5166bfc2ae4c965fc2ac["ekos/crates/ledger/src/index.rs"]
    na43fa3872cac5166bfc2ae4c965fc2ac -->|DependsOn| n02602034970e54a986c7bd69593368ee
    n87860e47e5db5450a2337e7fe0c46d89["ekos/crates/marketing/src/store.rs"]
    n87860e47e5db5450a2337e7fe0c46d89 -->|DependsOn| n02602034970e54a986c7bd69593368ee
    naffbb6cf64eb5dd5805d01cf2bd08c7f["ekos/plugins/oracle/src/lib.rs"]
    naffbb6cf64eb5dd5805d01cf2bd08c7f -->|DependsOn| n02602034970e54a986c7bd69593368ee
    n99637da404895fcaba15b1144f48c3cc["ekos/crates/common/src/compress.rs"]
    n99637da404895fcaba15b1144f48c3cc -->|DependsOn| n02602034970e54a986c7bd69593368ee
    n1f09ad6b972d56d795978641b250abce["ekos/plugins/snowflake/src/lib.rs"]
    n1f09ad6b972d56d795978641b250abce -->|DependsOn| n02602034970e54a986c7bd69593368ee
    ne5b5b2e037635cb8a914f113bb9e3ac4["ekos/crates/compiler-core/src/diagnostics.rs"]
    ne5b5b2e037635cb8a914f113bb9e3ac4 -->|DependsOn| n02602034970e54a986c7bd69593368ee
    n918532b1739051288de5faf4f7a91daf["ekos/crates/artifact/src/lib.rs"]
    n918532b1739051288de5faf4f7a91daf -->|DependsOn| n02602034970e54a986c7bd69593368ee
    n7d8cf6bdfac156d9a7424db7a455ab7c["ekos/crates/runtime/src/lib.rs"]
    n7d8cf6bdfac156d9a7424db7a455ab7c -->|DependsOn| n02602034970e54a986c7bd69593368ee
```

## Evidence

_No evidence cited._
