# ekos_kir::KirEvidence (RustModule)

## Properties

_No compiled properties._

## Relationships

### DependsOn

- ← ekos/crates/recovery/src/python_analyzer.rs (`196ca3fd-e85c-5ab9-a142-50f63dc586b9`)
- ← ekos/crates/recovery/src/github_analyzer.rs (`09117129-0fa3-5361-a92f-97212fff36cb`)
- ← ekos/crates/recovery/src/git_analyzer.rs (`f908320d-aaa8-525a-9521-e6581d42da30`)
- ← ekos/crates/recovery/src/crypto_analyzer.rs (`c5652a0f-42a3-5e1c-82d4-0cf4d37fab34`)
- ← ekos/crates/semantic/src/transform_ir.rs (`b4fdd24c-8184-5879-9136-f0a70208955e`)
- ← ekos/crates/docs-gen/src/lib.rs (`6ca4fba0-18e5-59b7-9876-7dae72e2ce0e`)
- ← ekos/crates/ledger/src/lib.rs (`21938f45-b767-5933-9e71-12e15ff53eb1`)
- ← ekos/crates/ledger/src/fact_ledger.rs (`eadcb59e-818f-5d1e-af87-ff29aba11423`)
- ← ekos/crates/cli/src/commands/build.rs (`306def2e-bd5a-5784-9453-692c119e8d43`)
- ← ekos/crates/recovery/src/sql_analyzer.rs (`cd768c5e-1640-51c3-b6ec-cb7ac78ade6d`)
- ← ekos/crates/recovery/src/local_docs_analyzer.rs (`d4eeb831-bfb8-592d-ad11-0e13adad2090`)
- ← ekos/crates/cli/src/commands/identity.rs (`f6e3418b-d664-536b-8a69-b723a534ff1a`)
- ← ekos/crates/recovery/src/dependency_analyzer.rs (`9baa718a-d61d-554c-a791-7798003ba6c4`)
- ← ekos/crates/cli/src/commands/commit.rs (`f48ae11b-a9a7-54f0-8cc6-a192b1641436`)
- ← ekos/crates/recovery/src/confluence_analyzer.rs (`d1b7d840-ae82-5a26-b381-06fb944d4e3c`)
- ← ekos/crates/artifact/src/lib.rs (`918532b1-7390-5128-8de5-faf4f7a91daf`)
- ← ekos/crates/recovery/src/document_semantics_analyzer.rs (`62e92526-f096-5ed2-bc72-0bdae8703aa3`)
- ← ekos/crates/runtime/src/lib.rs (`7d8cf6bd-fac1-56d9-a742-4db7a455ab7c`)
- ← ekos/crates/recovery/src/crate_topology_analyzer.rs (`83764758-1115-54df-bb75-4a49e7334245`)
- ← ekos/crates/recovery/src/cicd_analyzer.rs (`b6532a21-993c-5d28-8d99-891c30d70063`)

## Diagram

```mermaid
graph TD
    n8bfbda903848539fbfdd932ad7f515ae["ekos_kir::KirEvidence"]
    n196ca3fde85c5ab9a14250f63dc586b9["ekos/crates/recovery/src/python_analyzer.rs"]
    n196ca3fde85c5ab9a14250f63dc586b9 -->|DependsOn| n8bfbda903848539fbfdd932ad7f515ae
    n091171290fa35361a92f97212fff36cb["ekos/crates/recovery/src/github_analyzer.rs"]
    n091171290fa35361a92f97212fff36cb -->|DependsOn| n8bfbda903848539fbfdd932ad7f515ae
    nf908320daaa8525a9521e6581d42da30["ekos/crates/recovery/src/git_analyzer.rs"]
    nf908320daaa8525a9521e6581d42da30 -->|DependsOn| n8bfbda903848539fbfdd932ad7f515ae
    nc5652a0f42a35e1c82d40cf4d37fab34["ekos/crates/recovery/src/crypto_analyzer.rs"]
    nc5652a0f42a35e1c82d40cf4d37fab34 -->|DependsOn| n8bfbda903848539fbfdd932ad7f515ae
    nb4fdd24c818458799136f0a70208955e["ekos/crates/semantic/src/transform_ir.rs"]
    nb4fdd24c818458799136f0a70208955e -->|DependsOn| n8bfbda903848539fbfdd932ad7f515ae
    n6ca4fba018e559b798767dae72e2ce0e["ekos/crates/docs-gen/src/lib.rs"]
    n6ca4fba018e559b798767dae72e2ce0e -->|DependsOn| n8bfbda903848539fbfdd932ad7f515ae
    n21938f45b76759339e7112e15ff53eb1["ekos/crates/ledger/src/lib.rs"]
    n21938f45b76759339e7112e15ff53eb1 -->|DependsOn| n8bfbda903848539fbfdd932ad7f515ae
    neadcb59e818f5d1eaf87ff29aba11423["ekos/crates/ledger/src/fact_ledger.rs"]
    neadcb59e818f5d1eaf87ff29aba11423 -->|DependsOn| n8bfbda903848539fbfdd932ad7f515ae
    n306def2ebd5a57849453692c119e8d43["ekos/crates/cli/src/commands/build.rs"]
    n306def2ebd5a57849453692c119e8d43 -->|DependsOn| n8bfbda903848539fbfdd932ad7f515ae
    ncd768c5e164051c3b6eccb7ac78ade6d["ekos/crates/recovery/src/sql_analyzer.rs"]
    ncd768c5e164051c3b6eccb7ac78ade6d -->|DependsOn| n8bfbda903848539fbfdd932ad7f515ae
    nd4eeb831bfb8592dad110e13adad2090["ekos/crates/recovery/src/local_docs_analyzer.rs"]
    nd4eeb831bfb8592dad110e13adad2090 -->|DependsOn| n8bfbda903848539fbfdd932ad7f515ae
    nf6e3418bd664536b8a69b723a534ff1a["ekos/crates/cli/src/commands/identity.rs"]
    nf6e3418bd664536b8a69b723a534ff1a -->|DependsOn| n8bfbda903848539fbfdd932ad7f515ae
    n9baa718ad61d554ca7917798003ba6c4["ekos/crates/recovery/src/dependency_analyzer.rs"]
    n9baa718ad61d554ca7917798003ba6c4 -->|DependsOn| n8bfbda903848539fbfdd932ad7f515ae
    nf48ae11ba9a754f08cc6a192b1641436["ekos/crates/cli/src/commands/commit.rs"]
    nf48ae11ba9a754f08cc6a192b1641436 -->|DependsOn| n8bfbda903848539fbfdd932ad7f515ae
    nd1b7d840ae825a26b38106fb944d4e3c["ekos/crates/recovery/src/confluence_analyzer.rs"]
    nd1b7d840ae825a26b38106fb944d4e3c -->|DependsOn| n8bfbda903848539fbfdd932ad7f515ae
    n918532b1739051288de5faf4f7a91daf["ekos/crates/artifact/src/lib.rs"]
    n918532b1739051288de5faf4f7a91daf -->|DependsOn| n8bfbda903848539fbfdd932ad7f515ae
    n62e92526f0965ed2bc720bdae8703aa3["ekos/crates/recovery/src/document_semantics_analyzer.rs"]
    n62e92526f0965ed2bc720bdae8703aa3 -->|DependsOn| n8bfbda903848539fbfdd932ad7f515ae
    n7d8cf6bdfac156d9a7424db7a455ab7c["ekos/crates/runtime/src/lib.rs"]
    n7d8cf6bdfac156d9a7424db7a455ab7c -->|DependsOn| n8bfbda903848539fbfdd932ad7f515ae
    n83764758111554dfbb754a49e7334245["ekos/crates/recovery/src/crate_topology_analyzer.rs"]
    n83764758111554dfbb754a49e7334245 -->|DependsOn| n8bfbda903848539fbfdd932ad7f515ae
    nb6532a21993c5d288d99891c30d70063["ekos/crates/recovery/src/cicd_analyzer.rs"]
    nb6532a21993c5d288d99891c30d70063 -->|DependsOn| n8bfbda903848539fbfdd932ad7f515ae
```

## Evidence

_No evidence cited._
