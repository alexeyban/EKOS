# dp.utils.logger (PythonModule)

## Properties

_No compiled properties._

## Relationships

### DependsOn

- ← notebooks/semantic/generate_semantic_tables.py (`4c5a3b5b-087b-557e-b0e3-346cfa340acf`)
- ← src/dp/io/raw_source.py (`35307936-2715-5284-af57-7302a06265d8`)
- ← notebooks/semantic/create_graph_views.py (`6959bd2f-54f2-5e78-9854-0280e441e28b`)
- ← notebooks/semantic/load_ontology.py (`50682566-d5f6-5ae2-8784-a10e2cff5f73`)
- ← notebooks/semantic/export_visual_model.py (`456cc917-5699-5196-84ae-381a044b53fc`)
- ← notebooks/semantic/test_semantic_layer.py (`4de946b6-e51d-5c63-9cf2-4a44984b10a0`)
- ← notebooks/semantic/extract_entities_llm.py (`351ba344-3c6e-525f-a2bd-77ca36cfcd79`)
- ← src/dp/quality/reconciliation.py (`8e9bf929-9c71-5ff4-b679-ab1530deb6b4`)
- ← notebooks/semantic/mcp_server.py (`a7d3ec87-640d-5341-8428-411519605e05`)
- ← src/dp/io/table.py (`16eb1a72-8fe3-547f-93bc-158d68367ce8`)
- ← notebooks/semantic/load_semantic_metadata.py (`f967574f-24a0-591e-b91b-87bf29ef73fb`)
- ← notebooks/bronze/dvdrental/load_adf_run_stats.py (`1f10f723-f696-5ecf-b10b-7d22186567a8`)
- ← src/dp/io/delta.py (`31c42960-32c1-571b-acd7-a22fcdb34ad7`)
- ← notebooks/semantic/generate_attribute_metadata.py (`6cef00aa-8bdc-577f-ba7a-dce4f2e1113e`)
- ← notebooks/semantic/setup_native_features.py (`9d4db4e6-d282-5c2b-9b6e-601f0dfbd9ce`)
- ← src/dp/transforms/bronze.py (`4dd1b529-c566-5d54-afef-e7e6286314ca`)
- ← notebooks/semantic/load_business_rules.py (`de9f535f-6a60-5fc1-916a-ce8a2668bd59`)
- ← notebooks/semantic/generate_embeddings.py (`7712f37f-7552-50a2-bddd-5edff5fda595`)
- ← src/dp/io/run_stats.py (`b58b935d-2918-5656-97b3-dad504a9e27c`)
- ← notebooks/bronze/dvdrental/generic_raw_to_bronze.py (`93be948b-dba2-52c8-8c61-f506ab2c4273`)
- ← notebooks/semantic/load_documents.py (`28bfa6a8-c797-59c1-bad8-77fd49dbb1d6`)
- ← src/dp/quality/checks.py (`7fa68776-5adc-5f58-94c3-7e455e4ad27d`)
- ← notebooks/shared/notify_failure.py (`8ffef62b-5ada-55e0-ab2a-1cbc394330aa`)
- ← src/dp/quality/reporter.py (`380d8a0e-916c-59ea-b799-31bb40f34ed1`)

## Diagram

```mermaid
graph TD
    n807b425ad4c15d8283742d1739339cd2["dp.utils.logger"]
    n4c5a3b5b087b557eb0e3346cfa340acf["notebooks/semantic/generate_semantic_tables.py"]
    n4c5a3b5b087b557eb0e3346cfa340acf -->|DependsOn| n807b425ad4c15d8283742d1739339cd2
    n3530793627155284af577302a06265d8["src/dp/io/raw_source.py"]
    n3530793627155284af577302a06265d8 -->|DependsOn| n807b425ad4c15d8283742d1739339cd2
    n6959bd2f54f25e7898540280e441e28b["notebooks/semantic/create_graph_views.py"]
    n6959bd2f54f25e7898540280e441e28b -->|DependsOn| n807b425ad4c15d8283742d1739339cd2
    n50682566d5f65ae28784a10e2cff5f73["notebooks/semantic/load_ontology.py"]
    n50682566d5f65ae28784a10e2cff5f73 -->|DependsOn| n807b425ad4c15d8283742d1739339cd2
    n456cc9175699519684ae381a044b53fc["notebooks/semantic/export_visual_model.py"]
    n456cc9175699519684ae381a044b53fc -->|DependsOn| n807b425ad4c15d8283742d1739339cd2
    n4de946b6e51d5c639cf24a44984b10a0["notebooks/semantic/test_semantic_layer.py"]
    n4de946b6e51d5c639cf24a44984b10a0 -->|DependsOn| n807b425ad4c15d8283742d1739339cd2
    n351ba3443c6e525fa2bd77ca36cfcd79["notebooks/semantic/extract_entities_llm.py"]
    n351ba3443c6e525fa2bd77ca36cfcd79 -->|DependsOn| n807b425ad4c15d8283742d1739339cd2
    n8e9bf9299c715ff4b679ab1530deb6b4["src/dp/quality/reconciliation.py"]
    n8e9bf9299c715ff4b679ab1530deb6b4 -->|DependsOn| n807b425ad4c15d8283742d1739339cd2
    na7d3ec87640d53418428411519605e05["notebooks/semantic/mcp_server.py"]
    na7d3ec87640d53418428411519605e05 -->|DependsOn| n807b425ad4c15d8283742d1739339cd2
    n16eb1a728fe3547f93bc158d68367ce8["src/dp/io/table.py"]
    n16eb1a728fe3547f93bc158d68367ce8 -->|DependsOn| n807b425ad4c15d8283742d1739339cd2
    nf967574f24a0591eb91b87bf29ef73fb["notebooks/semantic/load_semantic_metadata.py"]
    nf967574f24a0591eb91b87bf29ef73fb -->|DependsOn| n807b425ad4c15d8283742d1739339cd2
    n1f10f723f6965ecfb10b7d22186567a8["notebooks/bronze/dvdrental/load_adf_run_stats.py"]
    n1f10f723f6965ecfb10b7d22186567a8 -->|DependsOn| n807b425ad4c15d8283742d1739339cd2
    n31c4296032c1571bacd7a22fcdb34ad7["src/dp/io/delta.py"]
    n31c4296032c1571bacd7a22fcdb34ad7 -->|DependsOn| n807b425ad4c15d8283742d1739339cd2
    n6cef00aa8bdc577fba7adce4f2e1113e["notebooks/semantic/generate_attribute_metadata.py"]
    n6cef00aa8bdc577fba7adce4f2e1113e -->|DependsOn| n807b425ad4c15d8283742d1739339cd2
    n9d4db4e6d2825c2b9b6e601f0dfbd9ce["notebooks/semantic/setup_native_features.py"]
    n9d4db4e6d2825c2b9b6e601f0dfbd9ce -->|DependsOn| n807b425ad4c15d8283742d1739339cd2
    n4dd1b529c5665d54afefe7e6286314ca["src/dp/transforms/bronze.py"]
    n4dd1b529c5665d54afefe7e6286314ca -->|DependsOn| n807b425ad4c15d8283742d1739339cd2
    nde9f535f6a605fc1916ace8a2668bd59["notebooks/semantic/load_business_rules.py"]
    nde9f535f6a605fc1916ace8a2668bd59 -->|DependsOn| n807b425ad4c15d8283742d1739339cd2
    n7712f37f755250a2bddd5edff5fda595["notebooks/semantic/generate_embeddings.py"]
    n7712f37f755250a2bddd5edff5fda595 -->|DependsOn| n807b425ad4c15d8283742d1739339cd2
    nb58b935d2918565697b3dad504a9e27c["src/dp/io/run_stats.py"]
    nb58b935d2918565697b3dad504a9e27c -->|DependsOn| n807b425ad4c15d8283742d1739339cd2
    n93be948bdba252c88c61f506ab2c4273["notebooks/bronze/dvdrental/generic_raw_to_bronze.py"]
    n93be948bdba252c88c61f506ab2c4273 -->|DependsOn| n807b425ad4c15d8283742d1739339cd2
    n28bfa6a8c79759c1bad877fd49dbb1d6["notebooks/semantic/load_documents.py"]
    n28bfa6a8c79759c1bad877fd49dbb1d6 -->|DependsOn| n807b425ad4c15d8283742d1739339cd2
    n7fa687765adc5f5894c37e455e4ad27d["src/dp/quality/checks.py"]
    n7fa687765adc5f5894c37e455e4ad27d -->|DependsOn| n807b425ad4c15d8283742d1739339cd2
    n8ffef62b5ada55e0ab2a1cbc394330aa["notebooks/shared/notify_failure.py"]
    n8ffef62b5ada55e0ab2a1cbc394330aa -->|DependsOn| n807b425ad4c15d8283742d1739339cd2
    n380d8a0e916c59eab79931bb40f34ed1["src/dp/quality/reporter.py"]
    n380d8a0e916c59eab79931bb40f34ed1 -->|DependsOn| n807b425ad4c15d8283742d1739339cd2
```

## Evidence

_No evidence cited._
