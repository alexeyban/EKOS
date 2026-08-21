# __future__ (PythonModule)

## Properties

_No compiled properties._

## Relationships

### DependsOn

- ← src/dp/io/raw_source.py (`35307936-2715-5284-af57-7302a06265d8`)
- ← src/dp/semantic/graph.py (`efdc0993-b53b-5997-a832-81d4347f2dd8`)
- ← src/dp/semantic/document_chunker.py (`b2e01fee-7a48-5250-94a8-a5530bff27e0`)
- ← tests/dp/quality/test_reconciliation.py (`673e3cd1-759a-574a-b0dd-d2216a5c6777`)
- ← src/dp/semantic/ontology_loader.py (`fc208e36-b1df-5a7d-91cf-efed6d59e68e`)
- ← src/dp/metadata/loader.py (`b92f5925-3f28-5b8c-9321-0b8eaf210175`)
- ← src/dp/semantic/mcp_tools.py (`9662dc12-d944-55cb-be77-7dbe15265ee7`)
- ← src/dp/semantic/rules_loader.py (`cc6dd1ee-9468-5cc7-bdfc-a9fb90788414`)
- ← tests/integration/semantic/test_mcp_live.py (`2b2c3f46-1ee2-547b-abd4-4c88f898c363`)
- ← tests/dp/semantic/test_llm_enricher.py (`4c45abca-060e-5aeb-9f0d-7c5480cad37a`)
- ← src/dp/quality/reconciliation.py (`8e9bf929-9c71-5ff4-b679-ab1530deb6b4`)
- ← src/dp/semantic/llm_enricher.py (`48cbaf39-e0fa-5dfb-b7f9-3c29241145c1`)
- ← src/dp/io/table.py (`16eb1a72-8fe3-547f-93bc-158d68367ce8`)
- ← scripts/notebook_dryrun.py (`ec7d626d-1aa5-50d1-87d3-7507d485a8c5`)
- ← src/dp/io/delta.py (`31c42960-32c1-571b-acd7-a22fcdb34ad7`)
- ← src/dp/transforms/cleaning.py (`ff9173c1-eae9-5f50-a2ee-d66c7b171805`)
- ← tests/dp/io/test_run_stats.py (`b49dd140-f6f7-5339-8704-7098024220a6`)
- ← src/dp/transforms/bronze.py (`4dd1b529-c566-5d54-afef-e7e6286314ca`)
- ← tests/integration/semantic/test_dbt_smoke.py (`04875f9e-4f89-574f-bad1-540c39d1e88f`)
- ← tests/dp/semantic/test_rules_loader.py (`11db8d26-0218-542a-9518-c5304931957b`)
- ← tests/dp/metadata/test_semantic_loader.py (`076a9cd4-9ae9-544f-b387-fa6e08ed64a5`)
- ← src/dp/semantic/visual_export.py (`21f0a170-901b-56e4-be84-c03f122240f5`)
- ← tests/dp/semantic/test_ontology_loader.py (`7c2568ce-fb4f-5d12-9f63-74471fff8cd8`)
- ← tests/dp/semantic/test_graph.py (`fa29a1d3-68a9-50f3-96b9-504f3ccd858e`)
- ← tests/integration/semantic/conftest.py (`f3bf6620-d61a-56a2-b193-843d3308ae7f`)
- ← tests/dp/semantic/test_mcp_tools.py (`4f73df62-9377-53aa-a84c-60d87949ad41`)
- ← src/dp/io/run_stats.py (`b58b935d-2918-5656-97b3-dad504a9e27c`)
- ← src/dp/quality/checks.py (`7fa68776-5adc-5f58-94c3-7e455e4ad27d`)
- ← src/dp/transforms/schema.py (`7e29d739-eb56-5fc1-b3e8-6ee53d0620ed`)
- ← tests/integration/semantic/test_semantic_round_trip.py (`1467a6cf-5d45-5963-a2f9-46c99a5917d4`)
- ← src/dp/semantic/embeddings.py (`34150f72-e0dc-5a51-b139-e37151e4d394`)
- ← src/dp/quality/reporter.py (`380d8a0e-916c-59ea-b799-31bb40f34ed1`)
- ← tests/dp/semantic/test_visual_export.py (`cfab3f85-2d5b-525a-9011-ea372ea9a2c8`)
- ← src/dp/metadata/semantic_loader.py (`eb3d03f1-86cf-5447-97d5-b158faf663df`)

## Diagram

```mermaid
graph TD
    n05d2f361c0ba58ed966b59f4c1b79b08["__future__"]
    n3530793627155284af577302a06265d8["src/dp/io/raw_source.py"]
    n3530793627155284af577302a06265d8 -->|DependsOn| n05d2f361c0ba58ed966b59f4c1b79b08
    nefdc0993b53b5997a83281d4347f2dd8["src/dp/semantic/graph.py"]
    nefdc0993b53b5997a83281d4347f2dd8 -->|DependsOn| n05d2f361c0ba58ed966b59f4c1b79b08
    nb2e01fee7a48525094a8a5530bff27e0["src/dp/semantic/document_chunker.py"]
    nb2e01fee7a48525094a8a5530bff27e0 -->|DependsOn| n05d2f361c0ba58ed966b59f4c1b79b08
    n673e3cd1759a574ab0ddd2216a5c6777["tests/dp/quality/test_reconciliation.py"]
    n673e3cd1759a574ab0ddd2216a5c6777 -->|DependsOn| n05d2f361c0ba58ed966b59f4c1b79b08
    nfc208e36b1df5a7d91cfefed6d59e68e["src/dp/semantic/ontology_loader.py"]
    nfc208e36b1df5a7d91cfefed6d59e68e -->|DependsOn| n05d2f361c0ba58ed966b59f4c1b79b08
    nb92f59253f285b8c93210b8eaf210175["src/dp/metadata/loader.py"]
    nb92f59253f285b8c93210b8eaf210175 -->|DependsOn| n05d2f361c0ba58ed966b59f4c1b79b08
    n9662dc12d94455cbbe777dbe15265ee7["src/dp/semantic/mcp_tools.py"]
    n9662dc12d94455cbbe777dbe15265ee7 -->|DependsOn| n05d2f361c0ba58ed966b59f4c1b79b08
    ncc6dd1ee94685cc7bdfca9fb90788414["src/dp/semantic/rules_loader.py"]
    ncc6dd1ee94685cc7bdfca9fb90788414 -->|DependsOn| n05d2f361c0ba58ed966b59f4c1b79b08
    n2b2c3f461ee2547babd44c88f898c363["tests/integration/semantic/test_mcp_live.py"]
    n2b2c3f461ee2547babd44c88f898c363 -->|DependsOn| n05d2f361c0ba58ed966b59f4c1b79b08
    n4c45abca060e5aeb9f0d7c5480cad37a["tests/dp/semantic/test_llm_enricher.py"]
    n4c45abca060e5aeb9f0d7c5480cad37a -->|DependsOn| n05d2f361c0ba58ed966b59f4c1b79b08
    n8e9bf9299c715ff4b679ab1530deb6b4["src/dp/quality/reconciliation.py"]
    n8e9bf9299c715ff4b679ab1530deb6b4 -->|DependsOn| n05d2f361c0ba58ed966b59f4c1b79b08
    n48cbaf39e0fa5dfbb7f93c29241145c1["src/dp/semantic/llm_enricher.py"]
    n48cbaf39e0fa5dfbb7f93c29241145c1 -->|DependsOn| n05d2f361c0ba58ed966b59f4c1b79b08
    n16eb1a728fe3547f93bc158d68367ce8["src/dp/io/table.py"]
    n16eb1a728fe3547f93bc158d68367ce8 -->|DependsOn| n05d2f361c0ba58ed966b59f4c1b79b08
    nec7d626d1aa550d187d37507d485a8c5["scripts/notebook_dryrun.py"]
    nec7d626d1aa550d187d37507d485a8c5 -->|DependsOn| n05d2f361c0ba58ed966b59f4c1b79b08
    n31c4296032c1571bacd7a22fcdb34ad7["src/dp/io/delta.py"]
    n31c4296032c1571bacd7a22fcdb34ad7 -->|DependsOn| n05d2f361c0ba58ed966b59f4c1b79b08
    nff9173c1eae95f50a2eed66c7b171805["src/dp/transforms/cleaning.py"]
    nff9173c1eae95f50a2eed66c7b171805 -->|DependsOn| n05d2f361c0ba58ed966b59f4c1b79b08
    nb49dd140f6f7533987047098024220a6["tests/dp/io/test_run_stats.py"]
    nb49dd140f6f7533987047098024220a6 -->|DependsOn| n05d2f361c0ba58ed966b59f4c1b79b08
    n4dd1b529c5665d54afefe7e6286314ca["src/dp/transforms/bronze.py"]
    n4dd1b529c5665d54afefe7e6286314ca -->|DependsOn| n05d2f361c0ba58ed966b59f4c1b79b08
    n04875f9e4f89574fbad1540c39d1e88f["tests/integration/semantic/test_dbt_smoke.py"]
    n04875f9e4f89574fbad1540c39d1e88f -->|DependsOn| n05d2f361c0ba58ed966b59f4c1b79b08
    n11db8d260218542a9518c5304931957b["tests/dp/semantic/test_rules_loader.py"]
    n11db8d260218542a9518c5304931957b -->|DependsOn| n05d2f361c0ba58ed966b59f4c1b79b08
    n076a9cd49ae9544fb387fa6e08ed64a5["tests/dp/metadata/test_semantic_loader.py"]
    n076a9cd49ae9544fb387fa6e08ed64a5 -->|DependsOn| n05d2f361c0ba58ed966b59f4c1b79b08
    n21f0a170901b56e4be84c03f122240f5["src/dp/semantic/visual_export.py"]
    n21f0a170901b56e4be84c03f122240f5 -->|DependsOn| n05d2f361c0ba58ed966b59f4c1b79b08
    n7c2568cefb4f5d129f6374471fff8cd8["tests/dp/semantic/test_ontology_loader.py"]
    n7c2568cefb4f5d129f6374471fff8cd8 -->|DependsOn| n05d2f361c0ba58ed966b59f4c1b79b08
    nfa29a1d368a950f396b9504f3ccd858e["tests/dp/semantic/test_graph.py"]
    nfa29a1d368a950f396b9504f3ccd858e -->|DependsOn| n05d2f361c0ba58ed966b59f4c1b79b08
    nf3bf6620d61a56a2b193843d3308ae7f["tests/integration/semantic/conftest.py"]
    nf3bf6620d61a56a2b193843d3308ae7f -->|DependsOn| n05d2f361c0ba58ed966b59f4c1b79b08
    n4f73df62937753aaa84c60d87949ad41["tests/dp/semantic/test_mcp_tools.py"]
    n4f73df62937753aaa84c60d87949ad41 -->|DependsOn| n05d2f361c0ba58ed966b59f4c1b79b08
    nb58b935d2918565697b3dad504a9e27c["src/dp/io/run_stats.py"]
    nb58b935d2918565697b3dad504a9e27c -->|DependsOn| n05d2f361c0ba58ed966b59f4c1b79b08
    n7fa687765adc5f5894c37e455e4ad27d["src/dp/quality/checks.py"]
    n7fa687765adc5f5894c37e455e4ad27d -->|DependsOn| n05d2f361c0ba58ed966b59f4c1b79b08
    n7e29d739eb565fc1b3e86ee53d0620ed["src/dp/transforms/schema.py"]
    n7e29d739eb565fc1b3e86ee53d0620ed -->|DependsOn| n05d2f361c0ba58ed966b59f4c1b79b08
    n1467a6cf5d455963a2f946c99a5917d4["tests/integration/semantic/test_semantic_round_trip.py"]
    n1467a6cf5d455963a2f946c99a5917d4 -->|DependsOn| n05d2f361c0ba58ed966b59f4c1b79b08
    n34150f72e0dc5a51b139e37151e4d394["src/dp/semantic/embeddings.py"]
    n34150f72e0dc5a51b139e37151e4d394 -->|DependsOn| n05d2f361c0ba58ed966b59f4c1b79b08
    n380d8a0e916c59eab79931bb40f34ed1["src/dp/quality/reporter.py"]
    n380d8a0e916c59eab79931bb40f34ed1 -->|DependsOn| n05d2f361c0ba58ed966b59f4c1b79b08
    ncfab3f852d5b525a9011ea372ea9a2c8["tests/dp/semantic/test_visual_export.py"]
    ncfab3f852d5b525a9011ea372ea9a2c8 -->|DependsOn| n05d2f361c0ba58ed966b59f4c1b79b08
    neb3d03f186cf544797d5b158faf663df["src/dp/metadata/semantic_loader.py"]
    neb3d03f186cf544797d5b158faf663df -->|DependsOn| n05d2f361c0ba58ed966b59f4c1b79b08
```

## Evidence

_No evidence cited._
