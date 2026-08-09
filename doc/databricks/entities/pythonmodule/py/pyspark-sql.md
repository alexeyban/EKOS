# pyspark.sql (PythonModule)

## Properties

_No compiled properties._

## Relationships

### DependsOn

- ← src/dp/io/raw_source.py (`35307936-2715-5284-af57-7302a06265d8`)
- ← src/dp/semantic/graph.py (`efdc0993-b53b-5997-a832-81d4347f2dd8`)
- ← tests/dp/transforms/test_bronze.py (`81ca72ce-2747-5768-8cc6-a45b22e63c36`)
- ← tests/dp/quality/test_reconciliation.py (`673e3cd1-759a-574a-b0dd-d2216a5c6777`)
- ← tests/dp/conftest.py (`720ecee2-4b60-529b-872a-244dd9c837b3`)
- ← notebooks/semantic/load_ontology.py (`50682566-d5f6-5ae2-8784-a10e2cff5f73`)
- ← tests/integration/semantic/test_mcp_live.py (`2b2c3f46-1ee2-547b-abd4-4c88f898c363`)
- ← notebooks/semantic/extract_entities_llm.py (`351ba344-3c6e-525f-a2bd-77ca36cfcd79`)
- ← src/dp/quality/reconciliation.py (`8e9bf929-9c71-5ff4-b679-ab1530deb6b4`)
- ← src/dp/io/table.py (`16eb1a72-8fe3-547f-93bc-158d68367ce8`)
- ← notebooks/semantic/load_semantic_metadata.py (`f967574f-24a0-591e-b91b-87bf29ef73fb`)
- ← src/dp/io/delta.py (`31c42960-32c1-571b-acd7-a22fcdb34ad7`)
- ← notebooks/semantic/generate_attribute_metadata.py (`6cef00aa-8bdc-577f-ba7a-dce4f2e1113e`)
- ← src/dp/transforms/cleaning.py (`ff9173c1-eae9-5f50-a2ee-d66c7b171805`)
- ← src/dp/transforms/bronze.py (`4dd1b529-c566-5d54-afef-e7e6286314ca`)
- ← notebooks/semantic/load_business_rules.py (`de9f535f-6a60-5fc1-916a-ce8a2668bd59`)
- ← tests/integration/semantic/test_dbt_smoke.py (`04875f9e-4f89-574f-bad1-540c39d1e88f`)
- ← tests/dp/semantic/test_graph.py (`fa29a1d3-68a9-50f3-96b9-504f3ccd858e`)
- ← src/dp/io/run_stats.py (`b58b935d-2918-5656-97b3-dad504a9e27c`)
- ← notebooks/semantic/load_documents.py (`28bfa6a8-c797-59c1-bad8-77fd49dbb1d6`)
- ← src/dp/quality/checks.py (`7fa68776-5adc-5f58-94c3-7e455e4ad27d`)
- ← src/dp/transforms/schema.py (`7e29d739-eb56-5fc1-b3e8-6ee53d0620ed`)
- ← tests/integration/semantic/test_semantic_round_trip.py (`1467a6cf-5d45-5963-a2f9-46c99a5917d4`)
- ← src/dp/quality/reporter.py (`380d8a0e-916c-59ea-b799-31bb40f34ed1`)

## Diagram

```mermaid
graph TD
    n85eb5395f3d8590dafa393263fc7528a["pyspark.sql"]
    n3530793627155284af577302a06265d8["src/dp/io/raw_source.py"]
    n3530793627155284af577302a06265d8 -->|DependsOn| n85eb5395f3d8590dafa393263fc7528a
    nefdc0993b53b5997a83281d4347f2dd8["src/dp/semantic/graph.py"]
    nefdc0993b53b5997a83281d4347f2dd8 -->|DependsOn| n85eb5395f3d8590dafa393263fc7528a
    n81ca72ce274757688cc6a45b22e63c36["tests/dp/transforms/test_bronze.py"]
    n81ca72ce274757688cc6a45b22e63c36 -->|DependsOn| n85eb5395f3d8590dafa393263fc7528a
    n673e3cd1759a574ab0ddd2216a5c6777["tests/dp/quality/test_reconciliation.py"]
    n673e3cd1759a574ab0ddd2216a5c6777 -->|DependsOn| n85eb5395f3d8590dafa393263fc7528a
    n720ecee24b60529b872a244dd9c837b3["tests/dp/conftest.py"]
    n720ecee24b60529b872a244dd9c837b3 -->|DependsOn| n85eb5395f3d8590dafa393263fc7528a
    n50682566d5f65ae28784a10e2cff5f73["notebooks/semantic/load_ontology.py"]
    n50682566d5f65ae28784a10e2cff5f73 -->|DependsOn| n85eb5395f3d8590dafa393263fc7528a
    n2b2c3f461ee2547babd44c88f898c363["tests/integration/semantic/test_mcp_live.py"]
    n2b2c3f461ee2547babd44c88f898c363 -->|DependsOn| n85eb5395f3d8590dafa393263fc7528a
    n351ba3443c6e525fa2bd77ca36cfcd79["notebooks/semantic/extract_entities_llm.py"]
    n351ba3443c6e525fa2bd77ca36cfcd79 -->|DependsOn| n85eb5395f3d8590dafa393263fc7528a
    n8e9bf9299c715ff4b679ab1530deb6b4["src/dp/quality/reconciliation.py"]
    n8e9bf9299c715ff4b679ab1530deb6b4 -->|DependsOn| n85eb5395f3d8590dafa393263fc7528a
    n16eb1a728fe3547f93bc158d68367ce8["src/dp/io/table.py"]
    n16eb1a728fe3547f93bc158d68367ce8 -->|DependsOn| n85eb5395f3d8590dafa393263fc7528a
    nf967574f24a0591eb91b87bf29ef73fb["notebooks/semantic/load_semantic_metadata.py"]
    nf967574f24a0591eb91b87bf29ef73fb -->|DependsOn| n85eb5395f3d8590dafa393263fc7528a
    n31c4296032c1571bacd7a22fcdb34ad7["src/dp/io/delta.py"]
    n31c4296032c1571bacd7a22fcdb34ad7 -->|DependsOn| n85eb5395f3d8590dafa393263fc7528a
    n6cef00aa8bdc577fba7adce4f2e1113e["notebooks/semantic/generate_attribute_metadata.py"]
    n6cef00aa8bdc577fba7adce4f2e1113e -->|DependsOn| n85eb5395f3d8590dafa393263fc7528a
    nff9173c1eae95f50a2eed66c7b171805["src/dp/transforms/cleaning.py"]
    nff9173c1eae95f50a2eed66c7b171805 -->|DependsOn| n85eb5395f3d8590dafa393263fc7528a
    n4dd1b529c5665d54afefe7e6286314ca["src/dp/transforms/bronze.py"]
    n4dd1b529c5665d54afefe7e6286314ca -->|DependsOn| n85eb5395f3d8590dafa393263fc7528a
    nde9f535f6a605fc1916ace8a2668bd59["notebooks/semantic/load_business_rules.py"]
    nde9f535f6a605fc1916ace8a2668bd59 -->|DependsOn| n85eb5395f3d8590dafa393263fc7528a
    n04875f9e4f89574fbad1540c39d1e88f["tests/integration/semantic/test_dbt_smoke.py"]
    n04875f9e4f89574fbad1540c39d1e88f -->|DependsOn| n85eb5395f3d8590dafa393263fc7528a
    nfa29a1d368a950f396b9504f3ccd858e["tests/dp/semantic/test_graph.py"]
    nfa29a1d368a950f396b9504f3ccd858e -->|DependsOn| n85eb5395f3d8590dafa393263fc7528a
    nb58b935d2918565697b3dad504a9e27c["src/dp/io/run_stats.py"]
    nb58b935d2918565697b3dad504a9e27c -->|DependsOn| n85eb5395f3d8590dafa393263fc7528a
    n28bfa6a8c79759c1bad877fd49dbb1d6["notebooks/semantic/load_documents.py"]
    n28bfa6a8c79759c1bad877fd49dbb1d6 -->|DependsOn| n85eb5395f3d8590dafa393263fc7528a
    n7fa687765adc5f5894c37e455e4ad27d["src/dp/quality/checks.py"]
    n7fa687765adc5f5894c37e455e4ad27d -->|DependsOn| n85eb5395f3d8590dafa393263fc7528a
    n7e29d739eb565fc1b3e86ee53d0620ed["src/dp/transforms/schema.py"]
    n7e29d739eb565fc1b3e86ee53d0620ed -->|DependsOn| n85eb5395f3d8590dafa393263fc7528a
    n1467a6cf5d455963a2f946c99a5917d4["tests/integration/semantic/test_semantic_round_trip.py"]
    n1467a6cf5d455963a2f946c99a5917d4 -->|DependsOn| n85eb5395f3d8590dafa393263fc7528a
    n380d8a0e916c59eab79931bb40f34ed1["src/dp/quality/reporter.py"]
    n380d8a0e916c59eab79931bb40f34ed1 -->|DependsOn| n85eb5395f3d8590dafa393263fc7528a
```

## Evidence

_No evidence cited._
