# datetime (PythonModule)

## Properties

_No compiled properties._

## Relationships

### DependsOn

- ← src/dp/semantic/document_chunker.py (`b2e01fee-7a48-5250-94a8-a5530bff27e0`)
- ← tests/dp/transforms/test_bronze.py (`81ca72ce-2747-5768-8cc6-a45b22e63c36`)
- ← src/dp/semantic/ontology_loader.py (`fc208e36-b1df-5a7d-91cf-efed6d59e68e`)
- ← src/dp/semantic/rules_loader.py (`cc6dd1ee-9468-5cc7-bdfc-a9fb90788414`)
- ← notebooks/semantic/extract_entities_llm.py (`351ba344-3c6e-525f-a2bd-77ca36cfcd79`)
- ← notebooks/bronze/dvdrental/load_adf_run_stats.py (`1f10f723-f696-5ecf-b10b-7d22186567a8`)
- ← notebooks/semantic/generate_attribute_metadata.py (`6cef00aa-8bdc-577f-ba7a-dce4f2e1113e`)
- ← src/dp/transforms/bronze.py (`4dd1b529-c566-5d54-afef-e7e6286314ca`)
- ← notebooks/bronze/dvdrental/generic_raw_to_bronze.py (`93be948b-dba2-52c8-8c61-f506ab2c4273`)
- ← src/dp/quality/reporter.py (`380d8a0e-916c-59ea-b799-31bb40f34ed1`)
- ← src/dp/metadata/semantic_loader.py (`eb3d03f1-86cf-5447-97d5-b158faf663df`)

## Diagram

```mermaid
graph TD
    n1fb245050a5d5ae0891f1461cb54fdef["datetime"]
    nb2e01fee7a48525094a8a5530bff27e0["src/dp/semantic/document_chunker.py"]
    nb2e01fee7a48525094a8a5530bff27e0 -->|DependsOn| n1fb245050a5d5ae0891f1461cb54fdef
    n81ca72ce274757688cc6a45b22e63c36["tests/dp/transforms/test_bronze.py"]
    n81ca72ce274757688cc6a45b22e63c36 -->|DependsOn| n1fb245050a5d5ae0891f1461cb54fdef
    nfc208e36b1df5a7d91cfefed6d59e68e["src/dp/semantic/ontology_loader.py"]
    nfc208e36b1df5a7d91cfefed6d59e68e -->|DependsOn| n1fb245050a5d5ae0891f1461cb54fdef
    ncc6dd1ee94685cc7bdfca9fb90788414["src/dp/semantic/rules_loader.py"]
    ncc6dd1ee94685cc7bdfca9fb90788414 -->|DependsOn| n1fb245050a5d5ae0891f1461cb54fdef
    n351ba3443c6e525fa2bd77ca36cfcd79["notebooks/semantic/extract_entities_llm.py"]
    n351ba3443c6e525fa2bd77ca36cfcd79 -->|DependsOn| n1fb245050a5d5ae0891f1461cb54fdef
    n1f10f723f6965ecfb10b7d22186567a8["notebooks/bronze/dvdrental/load_adf_run_stats.py"]
    n1f10f723f6965ecfb10b7d22186567a8 -->|DependsOn| n1fb245050a5d5ae0891f1461cb54fdef
    n6cef00aa8bdc577fba7adce4f2e1113e["notebooks/semantic/generate_attribute_metadata.py"]
    n6cef00aa8bdc577fba7adce4f2e1113e -->|DependsOn| n1fb245050a5d5ae0891f1461cb54fdef
    n4dd1b529c5665d54afefe7e6286314ca["src/dp/transforms/bronze.py"]
    n4dd1b529c5665d54afefe7e6286314ca -->|DependsOn| n1fb245050a5d5ae0891f1461cb54fdef
    n93be948bdba252c88c61f506ab2c4273["notebooks/bronze/dvdrental/generic_raw_to_bronze.py"]
    n93be948bdba252c88c61f506ab2c4273 -->|DependsOn| n1fb245050a5d5ae0891f1461cb54fdef
    n380d8a0e916c59eab79931bb40f34ed1["src/dp/quality/reporter.py"]
    n380d8a0e916c59eab79931bb40f34ed1 -->|DependsOn| n1fb245050a5d5ae0891f1461cb54fdef
    neb3d03f186cf544797d5b158faf663df["src/dp/metadata/semantic_loader.py"]
    neb3d03f186cf544797d5b158faf663df -->|DependsOn| n1fb245050a5d5ae0891f1461cb54fdef
```

## Evidence

_No evidence cited._
