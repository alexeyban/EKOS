# json (PythonModule)

## Properties

_No compiled properties._

## Relationships

### DependsOn

- ← src/dp/semantic/document_chunker.py (`b2e01fee-7a48-5250-94a8-a5530bff27e0`)
- ← src/dp/semantic/ontology_loader.py (`fc208e36-b1df-5a7d-91cf-efed6d59e68e`)
- ← src/dp/metadata/loader.py (`b92f5925-3f28-5b8c-9321-0b8eaf210175`)
- ← notebooks/semantic/export_visual_model.py (`456cc917-5699-5196-84ae-381a044b53fc`)
- ← src/dp/semantic/mcp_tools.py (`9662dc12-d944-55cb-be77-7dbe15265ee7`)
- ← notebooks/semantic/test_semantic_layer.py (`4de946b6-e51d-5c63-9cf2-4a44984b10a0`)
- ← src/dp/semantic/rules_loader.py (`cc6dd1ee-9468-5cc7-bdfc-a9fb90788414`)
- ← notebooks/semantic/extract_entities_llm.py (`351ba344-3c6e-525f-a2bd-77ca36cfcd79`)
- ← tests/dp/semantic/test_llm_enricher.py (`4c45abca-060e-5aeb-9f0d-7c5480cad37a`)
- ← tests/dp/semantic/test_document_chunker.py (`6ba5d553-ee39-50fb-a5f3-039f8e60b053`)
- ← src/dp/semantic/llm_enricher.py (`48cbaf39-e0fa-5dfb-b7f9-3c29241145c1`)
- ← tests/dp/metadata/test_loader.py (`460d2dcf-09f6-545f-897a-d0faece53ce1`)
- ← notebooks/semantic/mcp_server.py (`a7d3ec87-640d-5341-8428-411519605e05`)
- ← notebooks/semantic/generate_attribute_metadata.py (`6cef00aa-8bdc-577f-ba7a-dce4f2e1113e`)
- ← tests/dp/io/test_run_stats.py (`b49dd140-f6f7-5339-8704-7098024220a6`)
- ← tests/dp/semantic/test_rules_loader.py (`11db8d26-0218-542a-9518-c5304931957b`)
- ← tests/dp/metadata/test_semantic_loader.py (`076a9cd4-9ae9-544f-b387-fa6e08ed64a5`)
- ← src/dp/semantic/visual_export.py (`21f0a170-901b-56e4-be84-c03f122240f5`)
- ← tests/dp/semantic/test_ontology_loader.py (`7c2568ce-fb4f-5d12-9f63-74471fff8cd8`)
- ← src/dp/utils/logger.py (`d41d6a36-dbba-5144-af8d-b2e842e02b18`)
- ← notebooks/shared/notify_failure.py (`8ffef62b-5ada-55e0-ab2a-1cbc394330aa`)
- ← src/dp/metadata/semantic_loader.py (`eb3d03f1-86cf-5447-97d5-b158faf663df`)

## Diagram

```mermaid
graph TD
    n4330d5b3c9dd54eba4bcb5db46824aed["json"]
    nb2e01fee7a48525094a8a5530bff27e0["src/dp/semantic/document_chunker.py"]
    nb2e01fee7a48525094a8a5530bff27e0 -->|DependsOn| n4330d5b3c9dd54eba4bcb5db46824aed
    nfc208e36b1df5a7d91cfefed6d59e68e["src/dp/semantic/ontology_loader.py"]
    nfc208e36b1df5a7d91cfefed6d59e68e -->|DependsOn| n4330d5b3c9dd54eba4bcb5db46824aed
    nb92f59253f285b8c93210b8eaf210175["src/dp/metadata/loader.py"]
    nb92f59253f285b8c93210b8eaf210175 -->|DependsOn| n4330d5b3c9dd54eba4bcb5db46824aed
    n456cc9175699519684ae381a044b53fc["notebooks/semantic/export_visual_model.py"]
    n456cc9175699519684ae381a044b53fc -->|DependsOn| n4330d5b3c9dd54eba4bcb5db46824aed
    n9662dc12d94455cbbe777dbe15265ee7["src/dp/semantic/mcp_tools.py"]
    n9662dc12d94455cbbe777dbe15265ee7 -->|DependsOn| n4330d5b3c9dd54eba4bcb5db46824aed
    n4de946b6e51d5c639cf24a44984b10a0["notebooks/semantic/test_semantic_layer.py"]
    n4de946b6e51d5c639cf24a44984b10a0 -->|DependsOn| n4330d5b3c9dd54eba4bcb5db46824aed
    ncc6dd1ee94685cc7bdfca9fb90788414["src/dp/semantic/rules_loader.py"]
    ncc6dd1ee94685cc7bdfca9fb90788414 -->|DependsOn| n4330d5b3c9dd54eba4bcb5db46824aed
    n351ba3443c6e525fa2bd77ca36cfcd79["notebooks/semantic/extract_entities_llm.py"]
    n351ba3443c6e525fa2bd77ca36cfcd79 -->|DependsOn| n4330d5b3c9dd54eba4bcb5db46824aed
    n4c45abca060e5aeb9f0d7c5480cad37a["tests/dp/semantic/test_llm_enricher.py"]
    n4c45abca060e5aeb9f0d7c5480cad37a -->|DependsOn| n4330d5b3c9dd54eba4bcb5db46824aed
    n6ba5d553ee3950fba5f3039f8e60b053["tests/dp/semantic/test_document_chunker.py"]
    n6ba5d553ee3950fba5f3039f8e60b053 -->|DependsOn| n4330d5b3c9dd54eba4bcb5db46824aed
    n48cbaf39e0fa5dfbb7f93c29241145c1["src/dp/semantic/llm_enricher.py"]
    n48cbaf39e0fa5dfbb7f93c29241145c1 -->|DependsOn| n4330d5b3c9dd54eba4bcb5db46824aed
    n460d2dcf09f6545f897ad0faece53ce1["tests/dp/metadata/test_loader.py"]
    n460d2dcf09f6545f897ad0faece53ce1 -->|DependsOn| n4330d5b3c9dd54eba4bcb5db46824aed
    na7d3ec87640d53418428411519605e05["notebooks/semantic/mcp_server.py"]
    na7d3ec87640d53418428411519605e05 -->|DependsOn| n4330d5b3c9dd54eba4bcb5db46824aed
    n6cef00aa8bdc577fba7adce4f2e1113e["notebooks/semantic/generate_attribute_metadata.py"]
    n6cef00aa8bdc577fba7adce4f2e1113e -->|DependsOn| n4330d5b3c9dd54eba4bcb5db46824aed
    nb49dd140f6f7533987047098024220a6["tests/dp/io/test_run_stats.py"]
    nb49dd140f6f7533987047098024220a6 -->|DependsOn| n4330d5b3c9dd54eba4bcb5db46824aed
    n11db8d260218542a9518c5304931957b["tests/dp/semantic/test_rules_loader.py"]
    n11db8d260218542a9518c5304931957b -->|DependsOn| n4330d5b3c9dd54eba4bcb5db46824aed
    n076a9cd49ae9544fb387fa6e08ed64a5["tests/dp/metadata/test_semantic_loader.py"]
    n076a9cd49ae9544fb387fa6e08ed64a5 -->|DependsOn| n4330d5b3c9dd54eba4bcb5db46824aed
    n21f0a170901b56e4be84c03f122240f5["src/dp/semantic/visual_export.py"]
    n21f0a170901b56e4be84c03f122240f5 -->|DependsOn| n4330d5b3c9dd54eba4bcb5db46824aed
    n7c2568cefb4f5d129f6374471fff8cd8["tests/dp/semantic/test_ontology_loader.py"]
    n7c2568cefb4f5d129f6374471fff8cd8 -->|DependsOn| n4330d5b3c9dd54eba4bcb5db46824aed
    nd41d6a36dbba5144af8db2e842e02b18["src/dp/utils/logger.py"]
    nd41d6a36dbba5144af8db2e842e02b18 -->|DependsOn| n4330d5b3c9dd54eba4bcb5db46824aed
    n8ffef62b5ada55e0ab2a1cbc394330aa["notebooks/shared/notify_failure.py"]
    n8ffef62b5ada55e0ab2a1cbc394330aa -->|DependsOn| n4330d5b3c9dd54eba4bcb5db46824aed
    neb3d03f186cf544797d5b158faf663df["src/dp/metadata/semantic_loader.py"]
    neb3d03f186cf544797d5b158faf663df -->|DependsOn| n4330d5b3c9dd54eba4bcb5db46824aed
```

## Evidence

_No evidence cited._
