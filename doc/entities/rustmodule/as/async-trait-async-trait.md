# async_trait::async_trait (RustModule)

## Properties

_No compiled properties._

## Relationships

### DependsOn

- ← ekos/plugins/github/src/lib.rs (`85954c06-2c57-57ee-8a60-8a92c239ab70`)
- ← ekos/crates/recovery/src/python_analyzer.rs (`196ca3fd-e85c-5ab9-a142-50f63dc586b9`)
- ← ekos/crates/recovery/src/github_analyzer.rs (`09117129-0fa3-5361-a92f-97212fff36cb`)
- ← ekos/crates/compiler-core/src/pass.rs (`5189d0a2-3e2b-529e-adbf-3984a77be404`)
- ← ekos/crates/recovery/src/git_analyzer.rs (`f908320d-aaa8-525a-9521-e6581d42da30`)
- ← ekos/crates/recovery/src/crypto_analyzer.rs (`c5652a0f-42a3-5e1c-82d4-0cf4d37fab34`)
- ← ekos/plugins/salesforce/src/lib.rs (`62e147c2-5a91-534c-98ac-6557560db8f6`)
- ← ekos/plugins/confluence/src/lib.rs (`b78f02e6-8e4d-58de-abb4-ed29b9688c5f`)
- ← ekos/crates/recovery/src/pentaho_analyzer.rs (`ce3d2f1b-e1c6-55d7-92bc-c1b69f76dbca`)
- ← ekos/crates/recovery/src/llm.rs (`61d089be-a24a-5745-8bca-67d1d16373ca`)
- ← ekos/crates/marketing/src/publisher.rs (`3fab908f-efe6-5e1f-940b-5a16e3b7c774`)
- ← ekos/crates/recovery/src/sql_analyzer.rs (`cd768c5e-1640-51c3-b6ec-cb7ac78ade6d`)
- ← ekos/plugins/file/src/lib.rs (`7cdc5253-6617-5e49-81c7-9e227a76c44b`)
- ← ekos/plugins/pentaho/src/lib.rs (`cebff2b5-8142-5508-b8ad-01561647c1cc`)
- ← ekos/crates/recovery/src/local_docs_analyzer.rs (`d4eeb831-bfb8-592d-ad11-0e13adad2090`)
- ← ekos/crates/recovery/src/dependency_analyzer.rs (`9baa718a-d61d-554c-a791-7798003ba6c4`)
- ← ekos/plugins/sap/src/lib.rs (`8ce136d7-2eb9-53fd-90c2-7d0d00aeb27d`)
- ← ekos/crates/recovery/src/ollama.rs (`952b3eaf-406f-5c22-b538-f5c2d5fbe2f9`)
- ← ekos/plugins/fabric/src/lib.rs (`ea3988ed-2565-56db-b0cf-09b6d4594525`)
- ← ekos/plugins/python/src/lib.rs (`a3158b70-7e78-5501-8593-4a22c3e9c266`)
- ← ekos/crates/recovery/src/rust_analyzer.rs (`50cde56d-1f82-53a6-bc71-b5b2f7c711bc`)
- ← ekos/crates/semantic/src/lib.rs (`54021a72-4846-550c-960f-e63303e4d103`)
- ← ekos/plugins/crypto/src/lib.rs (`83728c61-df4d-5ad6-81c7-5bf50ff761fb`)
- ← ekos/crates/recovery/src/anthropic.rs (`2b1d458b-2cbb-5b8a-9932-9c15c981a99e`)
- ← ekos/crates/observation-sdk/src/lib.rs (`66ce958b-7250-5ec2-954e-eacf8f64aae0`)
- ← ekos/crates/recovery/src/sql_transform_analyzer.rs (`987e3fbb-31ec-576d-b794-7e801336e8c8`)
- ← ekos/plugins/oracle/src/lib.rs (`affbb6cf-64eb-5dd5-805d-01cf2bd08c7f`)
- ← ekos/plugins/git/src/lib.rs (`8941bcba-6474-5c7b-af9e-97dc4f4f7a13`)
- ← ekos/plugins/snowflake/src/lib.rs (`1f09ad6b-972d-56d7-9597-8641b250abce`)
- ← ekos/plugins/rust/src/lib.rs (`1a06302f-f373-5d4e-9592-3d99844910e8`)
- ← ekos/crates/recovery/src/cache.rs (`0b06681a-4e07-5e02-a8d6-433ccf4aadc4`)
- ← ekos/crates/recovery/src/confluence_analyzer.rs (`d1b7d840-ae82-5a26-b381-06fb944d4e3c`)
- ← ekos/plugins/localdocs/src/lib.rs (`1ed81bd1-9be1-5cda-9158-9ad4f1980e3d`)
- ← ekos/crates/recovery/src/document_semantics_analyzer.rs (`62e92526-f096-5ed2-bc72-0bdae8703aa3`)
- ← ekos/crates/recovery/src/crate_topology_analyzer.rs (`83764758-1115-54df-bb75-4a49e7334245`)
- ← ekos/crates/recovery/src/cicd_analyzer.rs (`b6532a21-993c-5d28-8d99-891c30d70063`)

## Diagram

```mermaid
graph TD
    nb0102a5de1f05f0cb37b410f88809410["async_trait::async_trait"]
    n85954c062c5757ee8a608a92c239ab70["ekos/plugins/github/src/lib.rs"]
    n85954c062c5757ee8a608a92c239ab70 -->|DependsOn| nb0102a5de1f05f0cb37b410f88809410
    n196ca3fde85c5ab9a14250f63dc586b9["ekos/crates/recovery/src/python_analyzer.rs"]
    n196ca3fde85c5ab9a14250f63dc586b9 -->|DependsOn| nb0102a5de1f05f0cb37b410f88809410
    n091171290fa35361a92f97212fff36cb["ekos/crates/recovery/src/github_analyzer.rs"]
    n091171290fa35361a92f97212fff36cb -->|DependsOn| nb0102a5de1f05f0cb37b410f88809410
    n5189d0a23e2b529eadbf3984a77be404["ekos/crates/compiler-core/src/pass.rs"]
    n5189d0a23e2b529eadbf3984a77be404 -->|DependsOn| nb0102a5de1f05f0cb37b410f88809410
    nf908320daaa8525a9521e6581d42da30["ekos/crates/recovery/src/git_analyzer.rs"]
    nf908320daaa8525a9521e6581d42da30 -->|DependsOn| nb0102a5de1f05f0cb37b410f88809410
    nc5652a0f42a35e1c82d40cf4d37fab34["ekos/crates/recovery/src/crypto_analyzer.rs"]
    nc5652a0f42a35e1c82d40cf4d37fab34 -->|DependsOn| nb0102a5de1f05f0cb37b410f88809410
    n62e147c25a91534c98ac6557560db8f6["ekos/plugins/salesforce/src/lib.rs"]
    n62e147c25a91534c98ac6557560db8f6 -->|DependsOn| nb0102a5de1f05f0cb37b410f88809410
    nb78f02e68e4d58deabb4ed29b9688c5f["ekos/plugins/confluence/src/lib.rs"]
    nb78f02e68e4d58deabb4ed29b9688c5f -->|DependsOn| nb0102a5de1f05f0cb37b410f88809410
    nce3d2f1be1c655d792bcc1b69f76dbca["ekos/crates/recovery/src/pentaho_analyzer.rs"]
    nce3d2f1be1c655d792bcc1b69f76dbca -->|DependsOn| nb0102a5de1f05f0cb37b410f88809410
    n61d089bea24a57458bca67d1d16373ca["ekos/crates/recovery/src/llm.rs"]
    n61d089bea24a57458bca67d1d16373ca -->|DependsOn| nb0102a5de1f05f0cb37b410f88809410
    n3fab908fefe65e1f940b5a16e3b7c774["ekos/crates/marketing/src/publisher.rs"]
    n3fab908fefe65e1f940b5a16e3b7c774 -->|DependsOn| nb0102a5de1f05f0cb37b410f88809410
    ncd768c5e164051c3b6eccb7ac78ade6d["ekos/crates/recovery/src/sql_analyzer.rs"]
    ncd768c5e164051c3b6eccb7ac78ade6d -->|DependsOn| nb0102a5de1f05f0cb37b410f88809410
    n7cdc525366175e4981c79e227a76c44b["ekos/plugins/file/src/lib.rs"]
    n7cdc525366175e4981c79e227a76c44b -->|DependsOn| nb0102a5de1f05f0cb37b410f88809410
    ncebff2b581425508b8ad01561647c1cc["ekos/plugins/pentaho/src/lib.rs"]
    ncebff2b581425508b8ad01561647c1cc -->|DependsOn| nb0102a5de1f05f0cb37b410f88809410
    nd4eeb831bfb8592dad110e13adad2090["ekos/crates/recovery/src/local_docs_analyzer.rs"]
    nd4eeb831bfb8592dad110e13adad2090 -->|DependsOn| nb0102a5de1f05f0cb37b410f88809410
    n9baa718ad61d554ca7917798003ba6c4["ekos/crates/recovery/src/dependency_analyzer.rs"]
    n9baa718ad61d554ca7917798003ba6c4 -->|DependsOn| nb0102a5de1f05f0cb37b410f88809410
    n8ce136d72eb953fd90c27d0d00aeb27d["ekos/plugins/sap/src/lib.rs"]
    n8ce136d72eb953fd90c27d0d00aeb27d -->|DependsOn| nb0102a5de1f05f0cb37b410f88809410
    n952b3eaf406f5c22b538f5c2d5fbe2f9["ekos/crates/recovery/src/ollama.rs"]
    n952b3eaf406f5c22b538f5c2d5fbe2f9 -->|DependsOn| nb0102a5de1f05f0cb37b410f88809410
    nea3988ed256556dbb0cf09b6d4594525["ekos/plugins/fabric/src/lib.rs"]
    nea3988ed256556dbb0cf09b6d4594525 -->|DependsOn| nb0102a5de1f05f0cb37b410f88809410
    na3158b707e78550185934a22c3e9c266["ekos/plugins/python/src/lib.rs"]
    na3158b707e78550185934a22c3e9c266 -->|DependsOn| nb0102a5de1f05f0cb37b410f88809410
    n50cde56d1f8253a6bc71b5b2f7c711bc["ekos/crates/recovery/src/rust_analyzer.rs"]
    n50cde56d1f8253a6bc71b5b2f7c711bc -->|DependsOn| nb0102a5de1f05f0cb37b410f88809410
    n54021a724846550c960fe63303e4d103["ekos/crates/semantic/src/lib.rs"]
    n54021a724846550c960fe63303e4d103 -->|DependsOn| nb0102a5de1f05f0cb37b410f88809410
    n83728c61df4d5ad681c75bf50ff761fb["ekos/plugins/crypto/src/lib.rs"]
    n83728c61df4d5ad681c75bf50ff761fb -->|DependsOn| nb0102a5de1f05f0cb37b410f88809410
    n2b1d458b2cbb5b8a99329c15c981a99e["ekos/crates/recovery/src/anthropic.rs"]
    n2b1d458b2cbb5b8a99329c15c981a99e -->|DependsOn| nb0102a5de1f05f0cb37b410f88809410
    n66ce958b72505ec2954eeacf8f64aae0["ekos/crates/observation-sdk/src/lib.rs"]
    n66ce958b72505ec2954eeacf8f64aae0 -->|DependsOn| nb0102a5de1f05f0cb37b410f88809410
    n987e3fbb31ec576db7947e801336e8c8["ekos/crates/recovery/src/sql_transform_analyzer.rs"]
    n987e3fbb31ec576db7947e801336e8c8 -->|DependsOn| nb0102a5de1f05f0cb37b410f88809410
    naffbb6cf64eb5dd5805d01cf2bd08c7f["ekos/plugins/oracle/src/lib.rs"]
    naffbb6cf64eb5dd5805d01cf2bd08c7f -->|DependsOn| nb0102a5de1f05f0cb37b410f88809410
    n8941bcba64745c7baf9e97dc4f4f7a13["ekos/plugins/git/src/lib.rs"]
    n8941bcba64745c7baf9e97dc4f4f7a13 -->|DependsOn| nb0102a5de1f05f0cb37b410f88809410
    n1f09ad6b972d56d795978641b250abce["ekos/plugins/snowflake/src/lib.rs"]
    n1f09ad6b972d56d795978641b250abce -->|DependsOn| nb0102a5de1f05f0cb37b410f88809410
    n1a06302ff3735d4e95923d99844910e8["ekos/plugins/rust/src/lib.rs"]
    n1a06302ff3735d4e95923d99844910e8 -->|DependsOn| nb0102a5de1f05f0cb37b410f88809410
    n0b06681a4e075e02a8d6433ccf4aadc4["ekos/crates/recovery/src/cache.rs"]
    n0b06681a4e075e02a8d6433ccf4aadc4 -->|DependsOn| nb0102a5de1f05f0cb37b410f88809410
    nd1b7d840ae825a26b38106fb944d4e3c["ekos/crates/recovery/src/confluence_analyzer.rs"]
    nd1b7d840ae825a26b38106fb944d4e3c -->|DependsOn| nb0102a5de1f05f0cb37b410f88809410
    n1ed81bd19be15cda91589ad4f1980e3d["ekos/plugins/localdocs/src/lib.rs"]
    n1ed81bd19be15cda91589ad4f1980e3d -->|DependsOn| nb0102a5de1f05f0cb37b410f88809410
    n62e92526f0965ed2bc720bdae8703aa3["ekos/crates/recovery/src/document_semantics_analyzer.rs"]
    n62e92526f0965ed2bc720bdae8703aa3 -->|DependsOn| nb0102a5de1f05f0cb37b410f88809410
    n83764758111554dfbb754a49e7334245["ekos/crates/recovery/src/crate_topology_analyzer.rs"]
    n83764758111554dfbb754a49e7334245 -->|DependsOn| nb0102a5de1f05f0cb37b410f88809410
    nb6532a21993c5d288d99891c30d70063["ekos/crates/recovery/src/cicd_analyzer.rs"]
    nb6532a21993c5d288d99891c30d70063 -->|DependsOn| nb0102a5de1f05f0cb37b410f88809410
```

## Evidence

_No evidence cited._
