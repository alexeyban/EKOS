# ekos_kir::ObjectKind (RustModule)

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
- ← ekos/crates/cli/src/commands/build.rs (`306def2e-bd5a-5784-9453-692c119e8d43`)
- ← ekos/crates/recovery/src/sql_analyzer.rs (`cd768c5e-1640-51c3-b6ec-cb7ac78ade6d`)
- ← ekos/crates/dbt-gen/src/lib.rs (`20d45ed0-c411-592d-8034-6486682f898c`)
- ← ekos/crates/recovery/src/local_docs_analyzer.rs (`d4eeb831-bfb8-592d-ad11-0e13adad2090`)
- ← ekos/crates/identity/src/cross_system.rs (`44d130a8-ca02-506d-bc1a-21b037fb492c`)
- ← benchmark/benches/identity_resolver.rs (`98a76aee-0267-55e8-941f-d3a106eb2053`)
- ← ekos/crates/recovery/src/dependency_analyzer.rs (`9baa718a-d61d-554c-a791-7798003ba6c4`)
- ← benchmark/benches/segment_store.rs (`d038c7b7-05b2-5c5c-8f62-c4ea6f2529ee`)
- ← ekos/crates/recovery/src/rust_analyzer.rs (`50cde56d-1f82-53a6-bc71-b5b2f7c711bc`)
- ← ekos/crates/semantic/src/lib.rs (`54021a72-4846-550c-960f-e63303e4d103`)
- ← benchmark/benches/fact_ledger.rs (`51ded36f-c9b2-5f8a-97c1-43fa4d7a63a1`)
- ← ekos/crates/identity/src/lib.rs (`c958282a-6d42-50ab-9cf9-8533976f0820`)
- ← benchmark/benches/storage_compaction.rs (`7c6dcc8e-035b-5a69-89a6-a45e962f93d8`)
- ← benchmark/benches/ledger_write.rs (`6b35bf13-a69a-59b1-8971-1df6156a8388`)
- ← ekos/crates/recovery/src/confluence_analyzer.rs (`d1b7d840-ae82-5a26-b381-06fb944d4e3c`)
- ← benchmark/benches/fact_model.rs (`8764eb55-7e1c-540c-b1d2-6545dcef6699`)
- ← ekos/crates/cli/src/commands/docs.rs (`5503d15b-112f-541f-8189-8be05a060beb`)
- ← ekos/crates/recovery/src/document_semantics_analyzer.rs (`62e92526-f096-5ed2-bc72-0bdae8703aa3`)
- ← benchmark/benches/semantic_compiler.rs (`95222458-b6a4-5b9c-bcc9-0e2e1909eb44`)
- ← benchmark/benches/index_runs.rs (`3efee357-ff49-5ace-8153-f8ad82f0cd57`)
- ← ekos/crates/recovery/src/crate_topology_analyzer.rs (`83764758-1115-54df-bb75-4a49e7334245`)
- ← ekos/crates/recovery/src/cicd_analyzer.rs (`b6532a21-993c-5d28-8d99-891c30d70063`)

## Diagram

```mermaid
graph TD
    nf9ca41f8eb7952e7a1c6d9590be1e443["ekos_kir::ObjectKind"]
    n196ca3fde85c5ab9a14250f63dc586b9["ekos/crates/recovery/src/python_analyzer.rs"]
    n196ca3fde85c5ab9a14250f63dc586b9 -->|DependsOn| nf9ca41f8eb7952e7a1c6d9590be1e443
    n091171290fa35361a92f97212fff36cb["ekos/crates/recovery/src/github_analyzer.rs"]
    n091171290fa35361a92f97212fff36cb -->|DependsOn| nf9ca41f8eb7952e7a1c6d9590be1e443
    nf908320daaa8525a9521e6581d42da30["ekos/crates/recovery/src/git_analyzer.rs"]
    nf908320daaa8525a9521e6581d42da30 -->|DependsOn| nf9ca41f8eb7952e7a1c6d9590be1e443
    nc5652a0f42a35e1c82d40cf4d37fab34["ekos/crates/recovery/src/crypto_analyzer.rs"]
    nc5652a0f42a35e1c82d40cf4d37fab34 -->|DependsOn| nf9ca41f8eb7952e7a1c6d9590be1e443
    nb4fdd24c818458799136f0a70208955e["ekos/crates/semantic/src/transform_ir.rs"]
    nb4fdd24c818458799136f0a70208955e -->|DependsOn| nf9ca41f8eb7952e7a1c6d9590be1e443
    nea98c0023a2b5dd69aee01db9fa9bde1["benchmark/benches/runtime_load_neighborhood.rs"]
    nea98c0023a2b5dd69aee01db9fa9bde1 -->|DependsOn| nf9ca41f8eb7952e7a1c6d9590be1e443
    n6ca4fba018e559b798767dae72e2ce0e["ekos/crates/docs-gen/src/lib.rs"]
    n6ca4fba018e559b798767dae72e2ce0e -->|DependsOn| nf9ca41f8eb7952e7a1c6d9590be1e443
    n306def2ebd5a57849453692c119e8d43["ekos/crates/cli/src/commands/build.rs"]
    n306def2ebd5a57849453692c119e8d43 -->|DependsOn| nf9ca41f8eb7952e7a1c6d9590be1e443
    ncd768c5e164051c3b6eccb7ac78ade6d["ekos/crates/recovery/src/sql_analyzer.rs"]
    ncd768c5e164051c3b6eccb7ac78ade6d -->|DependsOn| nf9ca41f8eb7952e7a1c6d9590be1e443
    n20d45ed0c411592d80346486682f898c["ekos/crates/dbt-gen/src/lib.rs"]
    n20d45ed0c411592d80346486682f898c -->|DependsOn| nf9ca41f8eb7952e7a1c6d9590be1e443
    nd4eeb831bfb8592dad110e13adad2090["ekos/crates/recovery/src/local_docs_analyzer.rs"]
    nd4eeb831bfb8592dad110e13adad2090 -->|DependsOn| nf9ca41f8eb7952e7a1c6d9590be1e443
    n44d130a8ca02506dbc1a21b037fb492c["ekos/crates/identity/src/cross_system.rs"]
    n44d130a8ca02506dbc1a21b037fb492c -->|DependsOn| nf9ca41f8eb7952e7a1c6d9590be1e443
    n98a76aee026755e8941fd3a106eb2053["benchmark/benches/identity_resolver.rs"]
    n98a76aee026755e8941fd3a106eb2053 -->|DependsOn| nf9ca41f8eb7952e7a1c6d9590be1e443
    n9baa718ad61d554ca7917798003ba6c4["ekos/crates/recovery/src/dependency_analyzer.rs"]
    n9baa718ad61d554ca7917798003ba6c4 -->|DependsOn| nf9ca41f8eb7952e7a1c6d9590be1e443
    nd038c7b705b25c5c8f62c4ea6f2529ee["benchmark/benches/segment_store.rs"]
    nd038c7b705b25c5c8f62c4ea6f2529ee -->|DependsOn| nf9ca41f8eb7952e7a1c6d9590be1e443
    n50cde56d1f8253a6bc71b5b2f7c711bc["ekos/crates/recovery/src/rust_analyzer.rs"]
    n50cde56d1f8253a6bc71b5b2f7c711bc -->|DependsOn| nf9ca41f8eb7952e7a1c6d9590be1e443
    n54021a724846550c960fe63303e4d103["ekos/crates/semantic/src/lib.rs"]
    n54021a724846550c960fe63303e4d103 -->|DependsOn| nf9ca41f8eb7952e7a1c6d9590be1e443
    n51ded36fc9b25f8a97c143fa4d7a63a1["benchmark/benches/fact_ledger.rs"]
    n51ded36fc9b25f8a97c143fa4d7a63a1 -->|DependsOn| nf9ca41f8eb7952e7a1c6d9590be1e443
    nc958282a6d4250ab9cf98533976f0820["ekos/crates/identity/src/lib.rs"]
    nc958282a6d4250ab9cf98533976f0820 -->|DependsOn| nf9ca41f8eb7952e7a1c6d9590be1e443
    n7c6dcc8e035b5a6989a6a45e962f93d8["benchmark/benches/storage_compaction.rs"]
    n7c6dcc8e035b5a6989a6a45e962f93d8 -->|DependsOn| nf9ca41f8eb7952e7a1c6d9590be1e443
    n6b35bf13a69a59b189711df6156a8388["benchmark/benches/ledger_write.rs"]
    n6b35bf13a69a59b189711df6156a8388 -->|DependsOn| nf9ca41f8eb7952e7a1c6d9590be1e443
    nd1b7d840ae825a26b38106fb944d4e3c["ekos/crates/recovery/src/confluence_analyzer.rs"]
    nd1b7d840ae825a26b38106fb944d4e3c -->|DependsOn| nf9ca41f8eb7952e7a1c6d9590be1e443
    n8764eb557e1c540cb1d26545dcef6699["benchmark/benches/fact_model.rs"]
    n8764eb557e1c540cb1d26545dcef6699 -->|DependsOn| nf9ca41f8eb7952e7a1c6d9590be1e443
    n5503d15b112f541f81898be05a060beb["ekos/crates/cli/src/commands/docs.rs"]
    n5503d15b112f541f81898be05a060beb -->|DependsOn| nf9ca41f8eb7952e7a1c6d9590be1e443
    n62e92526f0965ed2bc720bdae8703aa3["ekos/crates/recovery/src/document_semantics_analyzer.rs"]
    n62e92526f0965ed2bc720bdae8703aa3 -->|DependsOn| nf9ca41f8eb7952e7a1c6d9590be1e443
    n95222458b6a45b9cbcc90e2e1909eb44["benchmark/benches/semantic_compiler.rs"]
    n95222458b6a45b9cbcc90e2e1909eb44 -->|DependsOn| nf9ca41f8eb7952e7a1c6d9590be1e443
    n3efee357ff495ace8153f8ad82f0cd57["benchmark/benches/index_runs.rs"]
    n3efee357ff495ace8153f8ad82f0cd57 -->|DependsOn| nf9ca41f8eb7952e7a1c6d9590be1e443
    n83764758111554dfbb754a49e7334245["ekos/crates/recovery/src/crate_topology_analyzer.rs"]
    n83764758111554dfbb754a49e7334245 -->|DependsOn| nf9ca41f8eb7952e7a1c6d9590be1e443
    nb6532a21993c5d288d99891c30d70063["ekos/crates/recovery/src/cicd_analyzer.rs"]
    nb6532a21993c5d288d99891c30d70063 -->|DependsOn| nf9ca41f8eb7952e7a1c6d9590be1e443
```

## Evidence

_No evidence cited._
