# ekos_kir::KirId (RustModule)

## Properties

_No compiled properties._

## Relationships

### DependsOn

- ← ekos/crates/recovery/src/python_analyzer.rs (`196ca3fd-e85c-5ab9-a142-50f63dc586b9`)
- ← ekos/crates/recovery/src/github_analyzer.rs (`09117129-0fa3-5361-a92f-97212fff36cb`)
- ← ekos/crates/recovery/src/git_analyzer.rs (`f908320d-aaa8-525a-9521-e6581d42da30`)
- ← ekos/crates/recovery/src/crypto_analyzer.rs (`c5652a0f-42a3-5e1c-82d4-0cf4d37fab34`)
- ← ekos/crates/semantic/src/transform_ir.rs (`b4fdd24c-8184-5879-9136-f0a70208955e`)
- ← ekos/crates/ledger/src/lib.rs (`21938f45-b767-5933-9e71-12e15ff53eb1`)
- ← ekos/crates/cli/src/commands/dbt.rs (`d3579ceb-9751-53ad-b6be-693f17509a70`)
- ← ekos/crates/ledger/src/fact_ledger.rs (`eadcb59e-818f-5d1e-af87-ff29aba11423`)
- ← ekos/crates/cli/src/commands/build.rs (`306def2e-bd5a-5784-9453-692c119e8d43`)
- ← ekos/crates/recovery/src/sql_analyzer.rs (`cd768c5e-1640-51c3-b6ec-cb7ac78ade6d`)
- ← ekos/crates/dbt-gen/src/lib.rs (`20d45ed0-c411-592d-8034-6486682f898c`)
- ← ekos/crates/recovery/src/local_docs_analyzer.rs (`d4eeb831-bfb8-592d-ad11-0e13adad2090`)
- ← ekos/crates/identity/src/cross_system.rs (`44d130a8-ca02-506d-bc1a-21b037fb492c`)
- ← ekos/crates/cli/src/commands/identity.rs (`f6e3418b-d664-536b-8a69-b723a534ff1a`)
- ← ekos/crates/recovery/src/dependency_analyzer.rs (`9baa718a-d61d-554c-a791-7798003ba6c4`)
- ← ekos/crates/recovery/src/rust_analyzer.rs (`50cde56d-1f82-53a6-bc71-b5b2f7c711bc`)
- ← ekos/crates/cli/src/commands/query.rs (`76b10d14-834f-5bcb-8858-f46092b1989c`)
- ← ekos/crates/semantic/src/lib.rs (`54021a72-4846-550c-960f-e63303e4d103`)
- ← ekos/crates/recovery/src/crate_topology_analyzer.rs (`83764758-1115-54df-bb75-4a49e7334245`)
- ← ekos/crates/identity/src/lib.rs (`c958282a-6d42-50ab-9cf9-8533976f0820`)
- ← ekos/crates/ekl/src/interpreter.rs (`9c2cb6e4-ee09-503f-8cf5-ccfaf23ecd79`)
- ← ekos/crates/runtime/src/ai.rs (`e85e734d-ef58-5185-835a-34896d2da3f1`)
- ← benchmark/benches/fact_ledger.rs (`51ded36f-c9b2-5f8a-97c1-43fa4d7a63a1`)
- ← benchmark/benches/storage_compaction.rs (`7c6dcc8e-035b-5a69-89a6-a45e962f93d8`)
- ← ekos/crates/recovery/src/cicd_analyzer.rs (`b6532a21-993c-5d28-8d99-891c30d70063`)
- ← ekos/crates/cli/src/commands/mcp.rs (`ff76bb73-9285-5295-927c-5cb33a5bbc25`)
- ← ekos/crates/recovery/src/confluence_analyzer.rs (`d1b7d840-ae82-5a26-b381-06fb944d4e3c`)
- ← benchmark/benches/fact_model.rs (`8764eb55-7e1c-540c-b1d2-6545dcef6699`)
- ← ekos/crates/docs-gen/src/lib.rs (`6ca4fba0-18e5-59b7-9876-7dae72e2ce0e`)
- ← ekos/crates/recovery/src/document_semantics_analyzer.rs (`62e92526-f096-5ed2-bc72-0bdae8703aa3`)
- ← ekos/crates/runtime/src/lib.rs (`7d8cf6bd-fac1-56d9-a742-4db7a455ab7c`)

## Diagram

```mermaid
graph TD
    n75801906146757818ca0b0b9fe33bf5c["ekos_kir::KirId"]
    n196ca3fde85c5ab9a14250f63dc586b9["ekos/crates/recovery/src/python_analyzer.rs"]
    n196ca3fde85c5ab9a14250f63dc586b9 -->|DependsOn| n75801906146757818ca0b0b9fe33bf5c
    n091171290fa35361a92f97212fff36cb["ekos/crates/recovery/src/github_analyzer.rs"]
    n091171290fa35361a92f97212fff36cb -->|DependsOn| n75801906146757818ca0b0b9fe33bf5c
    nf908320daaa8525a9521e6581d42da30["ekos/crates/recovery/src/git_analyzer.rs"]
    nf908320daaa8525a9521e6581d42da30 -->|DependsOn| n75801906146757818ca0b0b9fe33bf5c
    nc5652a0f42a35e1c82d40cf4d37fab34["ekos/crates/recovery/src/crypto_analyzer.rs"]
    nc5652a0f42a35e1c82d40cf4d37fab34 -->|DependsOn| n75801906146757818ca0b0b9fe33bf5c
    nb4fdd24c818458799136f0a70208955e["ekos/crates/semantic/src/transform_ir.rs"]
    nb4fdd24c818458799136f0a70208955e -->|DependsOn| n75801906146757818ca0b0b9fe33bf5c
    n21938f45b76759339e7112e15ff53eb1["ekos/crates/ledger/src/lib.rs"]
    n21938f45b76759339e7112e15ff53eb1 -->|DependsOn| n75801906146757818ca0b0b9fe33bf5c
    nd3579ceb975153adb6be693f17509a70["ekos/crates/cli/src/commands/dbt.rs"]
    nd3579ceb975153adb6be693f17509a70 -->|DependsOn| n75801906146757818ca0b0b9fe33bf5c
    neadcb59e818f5d1eaf87ff29aba11423["ekos/crates/ledger/src/fact_ledger.rs"]
    neadcb59e818f5d1eaf87ff29aba11423 -->|DependsOn| n75801906146757818ca0b0b9fe33bf5c
    n306def2ebd5a57849453692c119e8d43["ekos/crates/cli/src/commands/build.rs"]
    n306def2ebd5a57849453692c119e8d43 -->|DependsOn| n75801906146757818ca0b0b9fe33bf5c
    ncd768c5e164051c3b6eccb7ac78ade6d["ekos/crates/recovery/src/sql_analyzer.rs"]
    ncd768c5e164051c3b6eccb7ac78ade6d -->|DependsOn| n75801906146757818ca0b0b9fe33bf5c
    n20d45ed0c411592d80346486682f898c["ekos/crates/dbt-gen/src/lib.rs"]
    n20d45ed0c411592d80346486682f898c -->|DependsOn| n75801906146757818ca0b0b9fe33bf5c
    nd4eeb831bfb8592dad110e13adad2090["ekos/crates/recovery/src/local_docs_analyzer.rs"]
    nd4eeb831bfb8592dad110e13adad2090 -->|DependsOn| n75801906146757818ca0b0b9fe33bf5c
    n44d130a8ca02506dbc1a21b037fb492c["ekos/crates/identity/src/cross_system.rs"]
    n44d130a8ca02506dbc1a21b037fb492c -->|DependsOn| n75801906146757818ca0b0b9fe33bf5c
    nf6e3418bd664536b8a69b723a534ff1a["ekos/crates/cli/src/commands/identity.rs"]
    nf6e3418bd664536b8a69b723a534ff1a -->|DependsOn| n75801906146757818ca0b0b9fe33bf5c
    n9baa718ad61d554ca7917798003ba6c4["ekos/crates/recovery/src/dependency_analyzer.rs"]
    n9baa718ad61d554ca7917798003ba6c4 -->|DependsOn| n75801906146757818ca0b0b9fe33bf5c
    n50cde56d1f8253a6bc71b5b2f7c711bc["ekos/crates/recovery/src/rust_analyzer.rs"]
    n50cde56d1f8253a6bc71b5b2f7c711bc -->|DependsOn| n75801906146757818ca0b0b9fe33bf5c
    n76b10d14834f5bcb8858f46092b1989c["ekos/crates/cli/src/commands/query.rs"]
    n76b10d14834f5bcb8858f46092b1989c -->|DependsOn| n75801906146757818ca0b0b9fe33bf5c
    n54021a724846550c960fe63303e4d103["ekos/crates/semantic/src/lib.rs"]
    n54021a724846550c960fe63303e4d103 -->|DependsOn| n75801906146757818ca0b0b9fe33bf5c
    n83764758111554dfbb754a49e7334245["ekos/crates/recovery/src/crate_topology_analyzer.rs"]
    n83764758111554dfbb754a49e7334245 -->|DependsOn| n75801906146757818ca0b0b9fe33bf5c
    nc958282a6d4250ab9cf98533976f0820["ekos/crates/identity/src/lib.rs"]
    nc958282a6d4250ab9cf98533976f0820 -->|DependsOn| n75801906146757818ca0b0b9fe33bf5c
    n9c2cb6e4ee09503f8cf5ccfaf23ecd79["ekos/crates/ekl/src/interpreter.rs"]
    n9c2cb6e4ee09503f8cf5ccfaf23ecd79 -->|DependsOn| n75801906146757818ca0b0b9fe33bf5c
    ne85e734def585185835a34896d2da3f1["ekos/crates/runtime/src/ai.rs"]
    ne85e734def585185835a34896d2da3f1 -->|DependsOn| n75801906146757818ca0b0b9fe33bf5c
    n51ded36fc9b25f8a97c143fa4d7a63a1["benchmark/benches/fact_ledger.rs"]
    n51ded36fc9b25f8a97c143fa4d7a63a1 -->|DependsOn| n75801906146757818ca0b0b9fe33bf5c
    n7c6dcc8e035b5a6989a6a45e962f93d8["benchmark/benches/storage_compaction.rs"]
    n7c6dcc8e035b5a6989a6a45e962f93d8 -->|DependsOn| n75801906146757818ca0b0b9fe33bf5c
    nb6532a21993c5d288d99891c30d70063["ekos/crates/recovery/src/cicd_analyzer.rs"]
    nb6532a21993c5d288d99891c30d70063 -->|DependsOn| n75801906146757818ca0b0b9fe33bf5c
    nff76bb7392855295927c5cb33a5bbc25["ekos/crates/cli/src/commands/mcp.rs"]
    nff76bb7392855295927c5cb33a5bbc25 -->|DependsOn| n75801906146757818ca0b0b9fe33bf5c
    nd1b7d840ae825a26b38106fb944d4e3c["ekos/crates/recovery/src/confluence_analyzer.rs"]
    nd1b7d840ae825a26b38106fb944d4e3c -->|DependsOn| n75801906146757818ca0b0b9fe33bf5c
    n8764eb557e1c540cb1d26545dcef6699["benchmark/benches/fact_model.rs"]
    n8764eb557e1c540cb1d26545dcef6699 -->|DependsOn| n75801906146757818ca0b0b9fe33bf5c
    n6ca4fba018e559b798767dae72e2ce0e["ekos/crates/docs-gen/src/lib.rs"]
    n6ca4fba018e559b798767dae72e2ce0e -->|DependsOn| n75801906146757818ca0b0b9fe33bf5c
    n62e92526f0965ed2bc720bdae8703aa3["ekos/crates/recovery/src/document_semantics_analyzer.rs"]
    n62e92526f0965ed2bc720bdae8703aa3 -->|DependsOn| n75801906146757818ca0b0b9fe33bf5c
    n7d8cf6bdfac156d9a7424db7a455ab7c["ekos/crates/runtime/src/lib.rs"]
    n7d8cf6bdfac156d9a7424db7a455ab7c -->|DependsOn| n75801906146757818ca0b0b9fe33bf5c
```

## Evidence

_No evidence cited._
