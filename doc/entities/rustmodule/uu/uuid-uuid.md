# uuid::Uuid (RustModule)

## Properties

_No compiled properties._

## Relationships

### DependsOn

- ← ekos/crates/recovery/src/python_analyzer.rs (`196ca3fd-e85c-5ab9-a142-50f63dc586b9`)
- ← ekos/crates/recovery/src/github_analyzer.rs (`09117129-0fa3-5361-a92f-97212fff36cb`)
- ← ekos/crates/recovery/src/git_analyzer.rs (`f908320d-aaa8-525a-9521-e6581d42da30`)
- ← ekos/crates/recovery/src/crypto_analyzer.rs (`c5652a0f-42a3-5e1c-82d4-0cf4d37fab34`)
- ← ekos/crates/kir/src/lib.rs (`078a10d5-8141-5e74-b89d-7120ec1be4f8`)
- ← ekos/crates/semantic/src/transform_ir.rs (`b4fdd24c-8184-5879-9136-f0a70208955e`)
- ← ekos/crates/ledger/src/lib.rs (`21938f45-b767-5933-9e71-12e15ff53eb1`)
- ← ekos/crates/ledger/src/search.rs (`b419f7f5-ae3b-50d7-9192-7ef6954555ce`)
- ← ekos/crates/ledger/src/fact_ledger.rs (`eadcb59e-818f-5d1e-af87-ff29aba11423`)
- ← ekos/crates/cli/src/commands/build.rs (`306def2e-bd5a-5784-9453-692c119e8d43`)
- ← ekos/crates/recovery/src/local_docs_analyzer.rs (`d4eeb831-bfb8-592d-ad11-0e13adad2090`)
- ← ekos/crates/recovery/src/dependency_analyzer.rs (`9baa718a-d61d-554c-a791-7798003ba6c4`)
- ← ekos/crates/recovery/src/rust_analyzer.rs (`50cde56d-1f82-53a6-bc71-b5b2f7c711bc`)
- ← ekos/crates/ledger/src/fact.rs (`7837f9fa-7178-5151-a068-e75361336c37`)
- ← ekos/crates/ledger/src/index.rs (`a43fa387-2cac-5166-bfc2-ae4c965fc2ac`)
- ← ekos/crates/recovery/src/confluence_analyzer.rs (`d1b7d840-ae82-5a26-b381-06fb944d4e3c`)
- ← ekos/crates/recovery/src/document_semantics_analyzer.rs (`62e92526-f096-5ed2-bc72-0bdae8703aa3`)
- ← benchmark/benches/index_runs.rs (`3efee357-ff49-5ace-8153-f8ad82f0cd57`)
- ← ekos/crates/recovery/src/crate_topology_analyzer.rs (`83764758-1115-54df-bb75-4a49e7334245`)
- ← ekos/crates/recovery/src/cicd_analyzer.rs (`b6532a21-993c-5d28-8d99-891c30d70063`)

## Diagram

```mermaid
graph TD
    n2b86b71f5ad8518e9bf40bc0ee14c94d["uuid::Uuid"]
    n196ca3fde85c5ab9a14250f63dc586b9["ekos/crates/recovery/src/python_analyzer.rs"]
    n196ca3fde85c5ab9a14250f63dc586b9 -->|DependsOn| n2b86b71f5ad8518e9bf40bc0ee14c94d
    n091171290fa35361a92f97212fff36cb["ekos/crates/recovery/src/github_analyzer.rs"]
    n091171290fa35361a92f97212fff36cb -->|DependsOn| n2b86b71f5ad8518e9bf40bc0ee14c94d
    nf908320daaa8525a9521e6581d42da30["ekos/crates/recovery/src/git_analyzer.rs"]
    nf908320daaa8525a9521e6581d42da30 -->|DependsOn| n2b86b71f5ad8518e9bf40bc0ee14c94d
    nc5652a0f42a35e1c82d40cf4d37fab34["ekos/crates/recovery/src/crypto_analyzer.rs"]
    nc5652a0f42a35e1c82d40cf4d37fab34 -->|DependsOn| n2b86b71f5ad8518e9bf40bc0ee14c94d
    n078a10d581415e74b89d7120ec1be4f8["ekos/crates/kir/src/lib.rs"]
    n078a10d581415e74b89d7120ec1be4f8 -->|DependsOn| n2b86b71f5ad8518e9bf40bc0ee14c94d
    nb4fdd24c818458799136f0a70208955e["ekos/crates/semantic/src/transform_ir.rs"]
    nb4fdd24c818458799136f0a70208955e -->|DependsOn| n2b86b71f5ad8518e9bf40bc0ee14c94d
    n21938f45b76759339e7112e15ff53eb1["ekos/crates/ledger/src/lib.rs"]
    n21938f45b76759339e7112e15ff53eb1 -->|DependsOn| n2b86b71f5ad8518e9bf40bc0ee14c94d
    nb419f7f5ae3b50d791927ef6954555ce["ekos/crates/ledger/src/search.rs"]
    nb419f7f5ae3b50d791927ef6954555ce -->|DependsOn| n2b86b71f5ad8518e9bf40bc0ee14c94d
    neadcb59e818f5d1eaf87ff29aba11423["ekos/crates/ledger/src/fact_ledger.rs"]
    neadcb59e818f5d1eaf87ff29aba11423 -->|DependsOn| n2b86b71f5ad8518e9bf40bc0ee14c94d
    n306def2ebd5a57849453692c119e8d43["ekos/crates/cli/src/commands/build.rs"]
    n306def2ebd5a57849453692c119e8d43 -->|DependsOn| n2b86b71f5ad8518e9bf40bc0ee14c94d
    nd4eeb831bfb8592dad110e13adad2090["ekos/crates/recovery/src/local_docs_analyzer.rs"]
    nd4eeb831bfb8592dad110e13adad2090 -->|DependsOn| n2b86b71f5ad8518e9bf40bc0ee14c94d
    n9baa718ad61d554ca7917798003ba6c4["ekos/crates/recovery/src/dependency_analyzer.rs"]
    n9baa718ad61d554ca7917798003ba6c4 -->|DependsOn| n2b86b71f5ad8518e9bf40bc0ee14c94d
    n50cde56d1f8253a6bc71b5b2f7c711bc["ekos/crates/recovery/src/rust_analyzer.rs"]
    n50cde56d1f8253a6bc71b5b2f7c711bc -->|DependsOn| n2b86b71f5ad8518e9bf40bc0ee14c94d
    n7837f9fa71785151a068e75361336c37["ekos/crates/ledger/src/fact.rs"]
    n7837f9fa71785151a068e75361336c37 -->|DependsOn| n2b86b71f5ad8518e9bf40bc0ee14c94d
    na43fa3872cac5166bfc2ae4c965fc2ac["ekos/crates/ledger/src/index.rs"]
    na43fa3872cac5166bfc2ae4c965fc2ac -->|DependsOn| n2b86b71f5ad8518e9bf40bc0ee14c94d
    nd1b7d840ae825a26b38106fb944d4e3c["ekos/crates/recovery/src/confluence_analyzer.rs"]
    nd1b7d840ae825a26b38106fb944d4e3c -->|DependsOn| n2b86b71f5ad8518e9bf40bc0ee14c94d
    n62e92526f0965ed2bc720bdae8703aa3["ekos/crates/recovery/src/document_semantics_analyzer.rs"]
    n62e92526f0965ed2bc720bdae8703aa3 -->|DependsOn| n2b86b71f5ad8518e9bf40bc0ee14c94d
    n3efee357ff495ace8153f8ad82f0cd57["benchmark/benches/index_runs.rs"]
    n3efee357ff495ace8153f8ad82f0cd57 -->|DependsOn| n2b86b71f5ad8518e9bf40bc0ee14c94d
    n83764758111554dfbb754a49e7334245["ekos/crates/recovery/src/crate_topology_analyzer.rs"]
    n83764758111554dfbb754a49e7334245 -->|DependsOn| n2b86b71f5ad8518e9bf40bc0ee14c94d
    nb6532a21993c5d288d99891c30d70063["ekos/crates/recovery/src/cicd_analyzer.rs"]
    nb6532a21993c5d288d99891c30d70063 -->|DependsOn| n2b86b71f5ad8518e9bf40bc0ee14c94d
```

## Evidence

_No evidence cited._
