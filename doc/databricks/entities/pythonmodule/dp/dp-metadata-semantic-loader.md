# dp.metadata.semantic_loader (PythonModule)

## Properties

_No compiled properties._

## Relationships

### DependsOn

- ← notebooks/semantic/extract_entities_llm.py (`351ba344-3c6e-525f-a2bd-77ca36cfcd79`)
- ← notebooks/semantic/load_semantic_metadata.py (`f967574f-24a0-591e-b91b-87bf29ef73fb`)
- ← tests/dp/metadata/test_semantic_loader.py (`076a9cd4-9ae9-544f-b387-fa6e08ed64a5`)

## Diagram

```mermaid
graph TD
    n9c10d7a2275f5d53b6d5cc7b0aa2bd1d["dp.metadata.semantic_loader"]
    n351ba3443c6e525fa2bd77ca36cfcd79["notebooks/semantic/extract_entities_llm.py"]
    n351ba3443c6e525fa2bd77ca36cfcd79 -->|DependsOn| n9c10d7a2275f5d53b6d5cc7b0aa2bd1d
    nf967574f24a0591eb91b87bf29ef73fb["notebooks/semantic/load_semantic_metadata.py"]
    nf967574f24a0591eb91b87bf29ef73fb -->|DependsOn| n9c10d7a2275f5d53b6d5cc7b0aa2bd1d
    n076a9cd49ae9544fb387fa6e08ed64a5["tests/dp/metadata/test_semantic_loader.py"]
    n076a9cd49ae9544fb387fa6e08ed64a5 -->|DependsOn| n9c10d7a2275f5d53b6d5cc7b0aa2bd1d
```

## Evidence

_No evidence cited._
