# criterion::criterion_group (RustModule)

## Properties

_No compiled properties._

## Relationships

### DependsOn

- ← benchmark/benches/observation_git.rs (`ff3b4467-e5f5-5401-8b3f-2374fddfc13f`)
- ← benchmark/benches/sql_analyzer.rs (`9f24ed23-7bfa-5f21-b42c-4937fbd2de4d`)
- ← benchmark/benches/runtime_load_neighborhood.rs (`ea98c002-3a2b-5dd6-9aee-01db9fa9bde1`)
- ← benchmark/benches/identity_resolver.rs (`98a76aee-0267-55e8-941f-d3a106eb2053`)
- ← benchmark/benches/segment_store.rs (`d038c7b7-05b2-5c5c-8f62-c4ea6f2529ee`)
- ← benchmark/benches/fact_ledger.rs (`51ded36f-c9b2-5f8a-97c1-43fa4d7a63a1`)
- ← benchmark/benches/storage_compaction.rs (`7c6dcc8e-035b-5a69-89a6-a45e962f93d8`)
- ← benchmark/benches/ledger_write.rs (`6b35bf13-a69a-59b1-8971-1df6156a8388`)
- ← benchmark/benches/fact_model.rs (`8764eb55-7e1c-540c-b1d2-6545dcef6699`)
- ← benchmark/benches/semantic_compiler.rs (`95222458-b6a4-5b9c-bcc9-0e2e1909eb44`)
- ← benchmark/benches/index_runs.rs (`3efee357-ff49-5ace-8153-f8ad82f0cd57`)

## Diagram

```mermaid
graph TD
    n5af25adf49845853aeb4f0243e3fc1aa["criterion::criterion_group"]
    nff3b4467e5f554018b3f2374fddfc13f["benchmark/benches/observation_git.rs"]
    nff3b4467e5f554018b3f2374fddfc13f -->|DependsOn| n5af25adf49845853aeb4f0243e3fc1aa
    n9f24ed237bfa5f21b42c4937fbd2de4d["benchmark/benches/sql_analyzer.rs"]
    n9f24ed237bfa5f21b42c4937fbd2de4d -->|DependsOn| n5af25adf49845853aeb4f0243e3fc1aa
    nea98c0023a2b5dd69aee01db9fa9bde1["benchmark/benches/runtime_load_neighborhood.rs"]
    nea98c0023a2b5dd69aee01db9fa9bde1 -->|DependsOn| n5af25adf49845853aeb4f0243e3fc1aa
    n98a76aee026755e8941fd3a106eb2053["benchmark/benches/identity_resolver.rs"]
    n98a76aee026755e8941fd3a106eb2053 -->|DependsOn| n5af25adf49845853aeb4f0243e3fc1aa
    nd038c7b705b25c5c8f62c4ea6f2529ee["benchmark/benches/segment_store.rs"]
    nd038c7b705b25c5c8f62c4ea6f2529ee -->|DependsOn| n5af25adf49845853aeb4f0243e3fc1aa
    n51ded36fc9b25f8a97c143fa4d7a63a1["benchmark/benches/fact_ledger.rs"]
    n51ded36fc9b25f8a97c143fa4d7a63a1 -->|DependsOn| n5af25adf49845853aeb4f0243e3fc1aa
    n7c6dcc8e035b5a6989a6a45e962f93d8["benchmark/benches/storage_compaction.rs"]
    n7c6dcc8e035b5a6989a6a45e962f93d8 -->|DependsOn| n5af25adf49845853aeb4f0243e3fc1aa
    n6b35bf13a69a59b189711df6156a8388["benchmark/benches/ledger_write.rs"]
    n6b35bf13a69a59b189711df6156a8388 -->|DependsOn| n5af25adf49845853aeb4f0243e3fc1aa
    n8764eb557e1c540cb1d26545dcef6699["benchmark/benches/fact_model.rs"]
    n8764eb557e1c540cb1d26545dcef6699 -->|DependsOn| n5af25adf49845853aeb4f0243e3fc1aa
    n95222458b6a45b9cbcc90e2e1909eb44["benchmark/benches/semantic_compiler.rs"]
    n95222458b6a45b9cbcc90e2e1909eb44 -->|DependsOn| n5af25adf49845853aeb4f0243e3fc1aa
    n3efee357ff495ace8153f8ad82f0cd57["benchmark/benches/index_runs.rs"]
    n3efee357ff495ace8153f8ad82f0cd57 -->|DependsOn| n5af25adf49845853aeb4f0243e3fc1aa
```

## Evidence

_No evidence cited._
