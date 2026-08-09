# ekos_kir::KirRelationship (RustModule)

## Properties

_No compiled properties._

## Relationships

### DependsOn

- ← ekos/crates/recovery/src/python_analyzer.rs (`196ca3fd-e85c-5ab9-a142-50f63dc586b9`)
- ← ekos/crates/recovery/src/github_analyzer.rs (`09117129-0fa3-5361-a92f-97212fff36cb`)
- ← ekos/crates/recovery/src/git_analyzer.rs (`f908320d-aaa8-525a-9521-e6581d42da30`)
- ← ekos/crates/recovery/src/crypto_analyzer.rs (`c5652a0f-42a3-5e1c-82d4-0cf4d37fab34`)
- ← ekos/crates/semantic/src/transform_ir.rs (`b4fdd24c-8184-5879-9136-f0a70208955e`)
- ← benchmark/benches/runtime_load_neighborhood.rs (`ea98c002-3a2b-5dd6-9aee-01db9fa9bde1`)
- ← ekos/crates/docs-gen/src/lib.rs (`6ca4fba0-18e5-59b7-9876-7dae72e2ce0e`)
- ← ekos/crates/ledger/src/lib.rs (`21938f45-b767-5933-9e71-12e15ff53eb1`)
- ← ekos/crates/ledger/src/fact_ledger.rs (`eadcb59e-818f-5d1e-af87-ff29aba11423`)
- ← ekos/crates/recovery/src/sql_analyzer.rs (`cd768c5e-1640-51c3-b6ec-cb7ac78ade6d`)
- ← ekos/crates/dbt-gen/src/lib.rs (`20d45ed0-c411-592d-8034-6486682f898c`)
- ← ekos/crates/recovery/src/local_docs_analyzer.rs (`d4eeb831-bfb8-592d-ad11-0e13adad2090`)
- ← ekos/crates/cli/src/commands/identity.rs (`f6e3418b-d664-536b-8a69-b723a534ff1a`)
- ← ekos/crates/recovery/src/dependency_analyzer.rs (`9baa718a-d61d-554c-a791-7798003ba6c4`)
- ← ekos/crates/recovery/src/rust_analyzer.rs (`50cde56d-1f82-53a6-bc71-b5b2f7c711bc`)
- ← ekos/crates/semantic/src/lib.rs (`54021a72-4846-550c-960f-e63303e4d103`)
- ← ekos/crates/ekl/src/interpreter.rs (`9c2cb6e4-ee09-503f-8cf5-ccfaf23ecd79`)
- ← ekos/crates/cli/src/commands/commit.rs (`f48ae11b-a9a7-54f0-8cc6-a192b1641436`)
- ← ekos/crates/recovery/src/confluence_analyzer.rs (`d1b7d840-ae82-5a26-b381-06fb944d4e3c`)
- ← ekos/crates/cli/src/commands/docs.rs (`5503d15b-112f-541f-8189-8be05a060beb`)
- ← ekos/crates/recovery/src/document_semantics_analyzer.rs (`62e92526-f096-5ed2-bc72-0bdae8703aa3`)
- ← ekos/crates/runtime/src/lib.rs (`7d8cf6bd-fac1-56d9-a742-4db7a455ab7c`)
- ← benchmark/benches/semantic_compiler.rs (`95222458-b6a4-5b9c-bcc9-0e2e1909eb44`)
- ← ekos/crates/recovery/src/crate_topology_analyzer.rs (`83764758-1115-54df-bb75-4a49e7334245`)

## Diagram

```mermaid
graph TD
    n84add3e75caa502fbd0094301bcac26b["ekos_kir::KirRelationship"]
    n196ca3fde85c5ab9a14250f63dc586b9["ekos/crates/recovery/src/python_analyzer.rs"]
    n196ca3fde85c5ab9a14250f63dc586b9 -->|DependsOn| n84add3e75caa502fbd0094301bcac26b
    n091171290fa35361a92f97212fff36cb["ekos/crates/recovery/src/github_analyzer.rs"]
    n091171290fa35361a92f97212fff36cb -->|DependsOn| n84add3e75caa502fbd0094301bcac26b
    nf908320daaa8525a9521e6581d42da30["ekos/crates/recovery/src/git_analyzer.rs"]
    nf908320daaa8525a9521e6581d42da30 -->|DependsOn| n84add3e75caa502fbd0094301bcac26b
    nc5652a0f42a35e1c82d40cf4d37fab34["ekos/crates/recovery/src/crypto_analyzer.rs"]
    nc5652a0f42a35e1c82d40cf4d37fab34 -->|DependsOn| n84add3e75caa502fbd0094301bcac26b
    nb4fdd24c818458799136f0a70208955e["ekos/crates/semantic/src/transform_ir.rs"]
    nb4fdd24c818458799136f0a70208955e -->|DependsOn| n84add3e75caa502fbd0094301bcac26b
    nea98c0023a2b5dd69aee01db9fa9bde1["benchmark/benches/runtime_load_neighborhood.rs"]
    nea98c0023a2b5dd69aee01db9fa9bde1 -->|DependsOn| n84add3e75caa502fbd0094301bcac26b
    n6ca4fba018e559b798767dae72e2ce0e["ekos/crates/docs-gen/src/lib.rs"]
    n6ca4fba018e559b798767dae72e2ce0e -->|DependsOn| n84add3e75caa502fbd0094301bcac26b
    n21938f45b76759339e7112e15ff53eb1["ekos/crates/ledger/src/lib.rs"]
    n21938f45b76759339e7112e15ff53eb1 -->|DependsOn| n84add3e75caa502fbd0094301bcac26b
    neadcb59e818f5d1eaf87ff29aba11423["ekos/crates/ledger/src/fact_ledger.rs"]
    neadcb59e818f5d1eaf87ff29aba11423 -->|DependsOn| n84add3e75caa502fbd0094301bcac26b
    ncd768c5e164051c3b6eccb7ac78ade6d["ekos/crates/recovery/src/sql_analyzer.rs"]
    ncd768c5e164051c3b6eccb7ac78ade6d -->|DependsOn| n84add3e75caa502fbd0094301bcac26b
    n20d45ed0c411592d80346486682f898c["ekos/crates/dbt-gen/src/lib.rs"]
    n20d45ed0c411592d80346486682f898c -->|DependsOn| n84add3e75caa502fbd0094301bcac26b
    nd4eeb831bfb8592dad110e13adad2090["ekos/crates/recovery/src/local_docs_analyzer.rs"]
    nd4eeb831bfb8592dad110e13adad2090 -->|DependsOn| n84add3e75caa502fbd0094301bcac26b
    nf6e3418bd664536b8a69b723a534ff1a["ekos/crates/cli/src/commands/identity.rs"]
    nf6e3418bd664536b8a69b723a534ff1a -->|DependsOn| n84add3e75caa502fbd0094301bcac26b
    n9baa718ad61d554ca7917798003ba6c4["ekos/crates/recovery/src/dependency_analyzer.rs"]
    n9baa718ad61d554ca7917798003ba6c4 -->|DependsOn| n84add3e75caa502fbd0094301bcac26b
    n50cde56d1f8253a6bc71b5b2f7c711bc["ekos/crates/recovery/src/rust_analyzer.rs"]
    n50cde56d1f8253a6bc71b5b2f7c711bc -->|DependsOn| n84add3e75caa502fbd0094301bcac26b
    n54021a724846550c960fe63303e4d103["ekos/crates/semantic/src/lib.rs"]
    n54021a724846550c960fe63303e4d103 -->|DependsOn| n84add3e75caa502fbd0094301bcac26b
    n9c2cb6e4ee09503f8cf5ccfaf23ecd79["ekos/crates/ekl/src/interpreter.rs"]
    n9c2cb6e4ee09503f8cf5ccfaf23ecd79 -->|DependsOn| n84add3e75caa502fbd0094301bcac26b
    nf48ae11ba9a754f08cc6a192b1641436["ekos/crates/cli/src/commands/commit.rs"]
    nf48ae11ba9a754f08cc6a192b1641436 -->|DependsOn| n84add3e75caa502fbd0094301bcac26b
    nd1b7d840ae825a26b38106fb944d4e3c["ekos/crates/recovery/src/confluence_analyzer.rs"]
    nd1b7d840ae825a26b38106fb944d4e3c -->|DependsOn| n84add3e75caa502fbd0094301bcac26b
    n5503d15b112f541f81898be05a060beb["ekos/crates/cli/src/commands/docs.rs"]
    n5503d15b112f541f81898be05a060beb -->|DependsOn| n84add3e75caa502fbd0094301bcac26b
    n62e92526f0965ed2bc720bdae8703aa3["ekos/crates/recovery/src/document_semantics_analyzer.rs"]
    n62e92526f0965ed2bc720bdae8703aa3 -->|DependsOn| n84add3e75caa502fbd0094301bcac26b
    n7d8cf6bdfac156d9a7424db7a455ab7c["ekos/crates/runtime/src/lib.rs"]
    n7d8cf6bdfac156d9a7424db7a455ab7c -->|DependsOn| n84add3e75caa502fbd0094301bcac26b
    n95222458b6a45b9cbcc90e2e1909eb44["benchmark/benches/semantic_compiler.rs"]
    n95222458b6a45b9cbcc90e2e1909eb44 -->|DependsOn| n84add3e75caa502fbd0094301bcac26b
    n83764758111554dfbb754a49e7334245["ekos/crates/recovery/src/crate_topology_analyzer.rs"]
    n83764758111554dfbb754a49e7334245 -->|DependsOn| n84add3e75caa502fbd0094301bcac26b
```

## Evidence

_No evidence cited._
