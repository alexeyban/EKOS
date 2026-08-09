# pytest (PythonModule)

## Properties

_No compiled properties._

## Relationships

### DependsOn

- ← tests/dp/quality/test_reconciliation.py (`673e3cd1-759a-574a-b0dd-d2216a5c6777`)
- ← tests/dp/quality/test_checks.py (`4e1453fe-3b3c-5512-918c-49d80c33798c`)
- ← tests/dp/conftest.py (`720ecee2-4b60-529b-872a-244dd9c837b3`)
- ← tests/dp/io/test_raw_source.py (`47ecd9af-36d5-5482-94d3-23c43936edb0`)
- ← tests/integration/semantic/test_mcp_live.py (`2b2c3f46-1ee2-547b-abd4-4c88f898c363`)
- ← tests/dp/semantic/test_document_chunker.py (`6ba5d553-ee39-50fb-a5f3-039f8e60b053`)
- ← tests/dp/metadata/test_loader.py (`460d2dcf-09f6-545f-897a-d0faece53ce1`)
- ← tests/dp/transforms/test_schema.py (`7e43ef28-b57c-53b8-868a-a1d14777bade`)
- ← tests/dp/io/test_run_stats.py (`b49dd140-f6f7-5339-8704-7098024220a6`)
- ← tests/dp/utils/test_env.py (`6f0fbfb8-9af6-58cc-98b9-8e7c8ded75b2`)
- ← tests/integration/semantic/test_dbt_smoke.py (`04875f9e-4f89-574f-bad1-540c39d1e88f`)
- ← tests/dp/semantic/test_rules_loader.py (`11db8d26-0218-542a-9518-c5304931957b`)
- ← tests/dp/metadata/test_semantic_loader.py (`076a9cd4-9ae9-544f-b387-fa6e08ed64a5`)
- ← tests/dp/semantic/test_ontology_loader.py (`7c2568ce-fb4f-5d12-9f63-74471fff8cd8`)
- ← tests/dp/semantic/test_graph.py (`fa29a1d3-68a9-50f3-96b9-504f3ccd858e`)
- ← tests/integration/semantic/conftest.py (`f3bf6620-d61a-56a2-b193-843d3308ae7f`)
- ← tests/integration/semantic/test_semantic_round_trip.py (`1467a6cf-5d45-5963-a2f9-46c99a5917d4`)

## Diagram

```mermaid
graph TD
    n16852505f5405ee9a22767bad061633e["pytest"]
    n673e3cd1759a574ab0ddd2216a5c6777["tests/dp/quality/test_reconciliation.py"]
    n673e3cd1759a574ab0ddd2216a5c6777 -->|DependsOn| n16852505f5405ee9a22767bad061633e
    n4e1453fe3b3c5512918c49d80c33798c["tests/dp/quality/test_checks.py"]
    n4e1453fe3b3c5512918c49d80c33798c -->|DependsOn| n16852505f5405ee9a22767bad061633e
    n720ecee24b60529b872a244dd9c837b3["tests/dp/conftest.py"]
    n720ecee24b60529b872a244dd9c837b3 -->|DependsOn| n16852505f5405ee9a22767bad061633e
    n47ecd9af36d5548294d323c43936edb0["tests/dp/io/test_raw_source.py"]
    n47ecd9af36d5548294d323c43936edb0 -->|DependsOn| n16852505f5405ee9a22767bad061633e
    n2b2c3f461ee2547babd44c88f898c363["tests/integration/semantic/test_mcp_live.py"]
    n2b2c3f461ee2547babd44c88f898c363 -->|DependsOn| n16852505f5405ee9a22767bad061633e
    n6ba5d553ee3950fba5f3039f8e60b053["tests/dp/semantic/test_document_chunker.py"]
    n6ba5d553ee3950fba5f3039f8e60b053 -->|DependsOn| n16852505f5405ee9a22767bad061633e
    n460d2dcf09f6545f897ad0faece53ce1["tests/dp/metadata/test_loader.py"]
    n460d2dcf09f6545f897ad0faece53ce1 -->|DependsOn| n16852505f5405ee9a22767bad061633e
    n7e43ef28b57c53b8868aa1d14777bade["tests/dp/transforms/test_schema.py"]
    n7e43ef28b57c53b8868aa1d14777bade -->|DependsOn| n16852505f5405ee9a22767bad061633e
    nb49dd140f6f7533987047098024220a6["tests/dp/io/test_run_stats.py"]
    nb49dd140f6f7533987047098024220a6 -->|DependsOn| n16852505f5405ee9a22767bad061633e
    n6f0fbfb89af658cc98b98e7c8ded75b2["tests/dp/utils/test_env.py"]
    n6f0fbfb89af658cc98b98e7c8ded75b2 -->|DependsOn| n16852505f5405ee9a22767bad061633e
    n04875f9e4f89574fbad1540c39d1e88f["tests/integration/semantic/test_dbt_smoke.py"]
    n04875f9e4f89574fbad1540c39d1e88f -->|DependsOn| n16852505f5405ee9a22767bad061633e
    n11db8d260218542a9518c5304931957b["tests/dp/semantic/test_rules_loader.py"]
    n11db8d260218542a9518c5304931957b -->|DependsOn| n16852505f5405ee9a22767bad061633e
    n076a9cd49ae9544fb387fa6e08ed64a5["tests/dp/metadata/test_semantic_loader.py"]
    n076a9cd49ae9544fb387fa6e08ed64a5 -->|DependsOn| n16852505f5405ee9a22767bad061633e
    n7c2568cefb4f5d129f6374471fff8cd8["tests/dp/semantic/test_ontology_loader.py"]
    n7c2568cefb4f5d129f6374471fff8cd8 -->|DependsOn| n16852505f5405ee9a22767bad061633e
    nfa29a1d368a950f396b9504f3ccd858e["tests/dp/semantic/test_graph.py"]
    nfa29a1d368a950f396b9504f3ccd858e -->|DependsOn| n16852505f5405ee9a22767bad061633e
    nf3bf6620d61a56a2b193843d3308ae7f["tests/integration/semantic/conftest.py"]
    nf3bf6620d61a56a2b193843d3308ae7f -->|DependsOn| n16852505f5405ee9a22767bad061633e
    n1467a6cf5d455963a2f946c99a5917d4["tests/integration/semantic/test_semantic_round_trip.py"]
    n1467a6cf5d455963a2f946c99a5917d4 -->|DependsOn| n16852505f5405ee9a22767bad061633e
```

## Evidence

_No evidence cited._
