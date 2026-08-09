# std::path::PathBuf (RustModule)

## Properties

_No compiled properties._

## Relationships

### DependsOn

- ← ekos/crates/cli/src/commands/store.rs (`ce2ed217-2b42-5760-9d2e-e2ca1574a517`)
- ← ekos/crates/artifact/src/store.rs (`d997f78d-b111-570e-b530-510e98c14df8`)
- ← ekos/crates/ledger/src/segment/mod.rs (`80a64e0a-b0ac-51df-b3be-128cddd1cc83`)
- ← ekos/crates/compiler-core/src/compiler.rs (`3b7209fe-32c4-588e-b8b5-5fa2165ff88b`)
- ← tests/integration/tests/integration.rs (`23e9eb43-c8df-52b5-a581-f2efba7085ea`)
- ← ekos/crates/ledger/src/lib.rs (`21938f45-b767-5933-9e71-12e15ff53eb1`)
- ← ekos/crates/artifact/src/pack.rs (`98cd7507-d9e7-59e3-acfb-c7ffb05d9f73`)
- ← ekos/crates/ledger/src/search.rs (`b419f7f5-ae3b-50d7-9192-7ef6954555ce`)
- ← ekos/crates/cli/src/commands/dbt.rs (`d3579ceb-9751-53ad-b6be-693f17509a70`)
- ← ekos/crates/ledger/src/fact_ledger.rs (`eadcb59e-818f-5d1e-af87-ff29aba11423`)
- ← ekos/crates/cli/src/commands/docs.rs (`5503d15b-112f-541f-8189-8be05a060beb`)
- ← ekos/crates/cli/src/commands/branch.rs (`8ae8543c-ebb4-545a-b5fe-5735e3953e88`)
- ← ekos/crates/ledger/tests/estate_migration.rs (`ea07674a-8ab1-54ed-a18a-f016992a8c48`)
- ← ekos/crates/semantic/src/lib.rs (`54021a72-4846-550c-960f-e63303e4d103`)
- ← ekos/crates/recovery/src/crate_topology_analyzer.rs (`83764758-1115-54df-bb75-4a49e7334245`)
- ← ekos/plugins/crypto/src/lib.rs (`83728c61-df4d-5ad6-81c7-5bf50ff761fb`)
- ← ekos/crates/compiler-core/src/config.rs (`d0747f34-25e1-5426-80eb-a54fffcac598`)
- ← ekos/crates/ledger/src/index.rs (`a43fa387-2cac-5166-bfc2-ae4c965fc2ac`)
- ← ekos/crates/marketing/src/store.rs (`87860e47-e5db-5450-a233-7e7fe0c46d89`)
- ← ekos/crates/observation-sdk/src/lib.rs (`66ce958b-7250-5ec2-954e-eacf8f64aae0`)
- ← ekos/crates/common/src/compress.rs (`99637da4-0489-5fca-ba15-b1144f48c3cc`)
- ← ekos/crates/recovery/src/cache.rs (`0b06681a-4e07-5e02-a8d6-433ccf4aadc4`)
- ← ekos/crates/cli/src/commands/marketing.rs (`e4550c2d-5dcf-5779-b25d-ac86e4019342`)
- ← ekos/crates/marketing/src/devlog.rs (`4ca01e5b-d21d-5312-ac99-c6aa65d7d8d0`)
- ← ekos/crates/cli/src/bin/ekos.rs (`67ea4c4e-5e03-5c3a-8066-adf7aaed8a3e`)

## Diagram

```mermaid
graph TD
    n92ae1b8c8579500b8b32f5743c20e986["std::path::PathBuf"]
    nce2ed2172b4257609d2ee2ca1574a517["ekos/crates/cli/src/commands/store.rs"]
    nce2ed2172b4257609d2ee2ca1574a517 -->|DependsOn| n92ae1b8c8579500b8b32f5743c20e986
    nd997f78db111570eb530510e98c14df8["ekos/crates/artifact/src/store.rs"]
    nd997f78db111570eb530510e98c14df8 -->|DependsOn| n92ae1b8c8579500b8b32f5743c20e986
    n80a64e0ab0ac51dfb3be128cddd1cc83["ekos/crates/ledger/src/segment/mod.rs"]
    n80a64e0ab0ac51dfb3be128cddd1cc83 -->|DependsOn| n92ae1b8c8579500b8b32f5743c20e986
    n3b7209fe32c4588eb8b55fa2165ff88b["ekos/crates/compiler-core/src/compiler.rs"]
    n3b7209fe32c4588eb8b55fa2165ff88b -->|DependsOn| n92ae1b8c8579500b8b32f5743c20e986
    n23e9eb43c8df52b5a581f2efba7085ea["tests/integration/tests/integration.rs"]
    n23e9eb43c8df52b5a581f2efba7085ea -->|DependsOn| n92ae1b8c8579500b8b32f5743c20e986
    n21938f45b76759339e7112e15ff53eb1["ekos/crates/ledger/src/lib.rs"]
    n21938f45b76759339e7112e15ff53eb1 -->|DependsOn| n92ae1b8c8579500b8b32f5743c20e986
    n98cd7507d9e759e3acfbc7ffb05d9f73["ekos/crates/artifact/src/pack.rs"]
    n98cd7507d9e759e3acfbc7ffb05d9f73 -->|DependsOn| n92ae1b8c8579500b8b32f5743c20e986
    nb419f7f5ae3b50d791927ef6954555ce["ekos/crates/ledger/src/search.rs"]
    nb419f7f5ae3b50d791927ef6954555ce -->|DependsOn| n92ae1b8c8579500b8b32f5743c20e986
    nd3579ceb975153adb6be693f17509a70["ekos/crates/cli/src/commands/dbt.rs"]
    nd3579ceb975153adb6be693f17509a70 -->|DependsOn| n92ae1b8c8579500b8b32f5743c20e986
    neadcb59e818f5d1eaf87ff29aba11423["ekos/crates/ledger/src/fact_ledger.rs"]
    neadcb59e818f5d1eaf87ff29aba11423 -->|DependsOn| n92ae1b8c8579500b8b32f5743c20e986
    n5503d15b112f541f81898be05a060beb["ekos/crates/cli/src/commands/docs.rs"]
    n5503d15b112f541f81898be05a060beb -->|DependsOn| n92ae1b8c8579500b8b32f5743c20e986
    n8ae8543cebb4545ab5fe5735e3953e88["ekos/crates/cli/src/commands/branch.rs"]
    n8ae8543cebb4545ab5fe5735e3953e88 -->|DependsOn| n92ae1b8c8579500b8b32f5743c20e986
    nea07674a8ab154eda18af016992a8c48["ekos/crates/ledger/tests/estate_migration.rs"]
    nea07674a8ab154eda18af016992a8c48 -->|DependsOn| n92ae1b8c8579500b8b32f5743c20e986
    n54021a724846550c960fe63303e4d103["ekos/crates/semantic/src/lib.rs"]
    n54021a724846550c960fe63303e4d103 -->|DependsOn| n92ae1b8c8579500b8b32f5743c20e986
    n83764758111554dfbb754a49e7334245["ekos/crates/recovery/src/crate_topology_analyzer.rs"]
    n83764758111554dfbb754a49e7334245 -->|DependsOn| n92ae1b8c8579500b8b32f5743c20e986
    n83728c61df4d5ad681c75bf50ff761fb["ekos/plugins/crypto/src/lib.rs"]
    n83728c61df4d5ad681c75bf50ff761fb -->|DependsOn| n92ae1b8c8579500b8b32f5743c20e986
    nd0747f3425e1542680eba54fffcac598["ekos/crates/compiler-core/src/config.rs"]
    nd0747f3425e1542680eba54fffcac598 -->|DependsOn| n92ae1b8c8579500b8b32f5743c20e986
    na43fa3872cac5166bfc2ae4c965fc2ac["ekos/crates/ledger/src/index.rs"]
    na43fa3872cac5166bfc2ae4c965fc2ac -->|DependsOn| n92ae1b8c8579500b8b32f5743c20e986
    n87860e47e5db5450a2337e7fe0c46d89["ekos/crates/marketing/src/store.rs"]
    n87860e47e5db5450a2337e7fe0c46d89 -->|DependsOn| n92ae1b8c8579500b8b32f5743c20e986
    n66ce958b72505ec2954eeacf8f64aae0["ekos/crates/observation-sdk/src/lib.rs"]
    n66ce958b72505ec2954eeacf8f64aae0 -->|DependsOn| n92ae1b8c8579500b8b32f5743c20e986
    n99637da404895fcaba15b1144f48c3cc["ekos/crates/common/src/compress.rs"]
    n99637da404895fcaba15b1144f48c3cc -->|DependsOn| n92ae1b8c8579500b8b32f5743c20e986
    n0b06681a4e075e02a8d6433ccf4aadc4["ekos/crates/recovery/src/cache.rs"]
    n0b06681a4e075e02a8d6433ccf4aadc4 -->|DependsOn| n92ae1b8c8579500b8b32f5743c20e986
    ne4550c2d5dcf5779b25dac86e4019342["ekos/crates/cli/src/commands/marketing.rs"]
    ne4550c2d5dcf5779b25dac86e4019342 -->|DependsOn| n92ae1b8c8579500b8b32f5743c20e986
    n4ca01e5bd21d5312ac99c6aa65d7d8d0["ekos/crates/marketing/src/devlog.rs"]
    n4ca01e5bd21d5312ac99c6aa65d7d8d0 -->|DependsOn| n92ae1b8c8579500b8b32f5743c20e986
    n67ea4c4e5e035c3a8066adf7aaed8a3e["ekos/crates/cli/src/bin/ekos.rs"]
    n67ea4c4e5e035c3a8066adf7aaed8a3e -->|DependsOn| n92ae1b8c8579500b8b32f5743c20e986
```

## Evidence

_No evidence cited._
