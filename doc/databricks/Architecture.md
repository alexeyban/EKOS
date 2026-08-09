# Architecture

## Components

- **Document**: 7
- **File**: 194
- **Person**: 1
- **PythonModule**: 45 — see [API.md](API.md)
- **PythonSymbol**: 252 — see [API.md](API.md)
- **Section**: 128
- **TransformNode**: 105

## Crate & Workspace Topology

_No crate/workspace manifests compiled._

## Technologies

_No technology dependencies compiled._

## CI/CD Pipelines

_No CI/CD pipeline definitions compiled._

## Entity Relationships

_No table foreign-key relationships compiled._

## Dependency Graph

### Contains

_380 `Contains` relationships compiled — diagram omitted, too large to render usefully. First 15 shown below; every object's own detail page (linked) lists its full relationship set._

- src/dp/utils/secrets.py → [get_secret](entities/pythonsymbol/ge/get-secret.md)
- notebooks/semantic/generate_semantic_tables.py → [_create_entity_table](entities/pythonsymbol/cr/create-entity-table.md)
- notebooks/semantic/generate_semantic_tables.py → [_create_rel_table](entities/pythonsymbol/cr/create-rel-table.md)
- src/dp/io/raw_source.py → [read_raw_snapshot](entities/pythonsymbol/re/read-raw-snapshot.md)
- src/dp/io/raw_source.py → [read_keys_snapshot](entities/pythonsymbol/re/read-keys-snapshot.md)
- src/dp/semantic/graph.py → [vertex_degrees](entities/pythonsymbol/ve/vertex-degrees.md)
- src/dp/semantic/graph.py → [neighbors](entities/pythonsymbol/ne/neighbors.md)
- src/dp/semantic/graph.py → [subgraph](entities/pythonsymbol/su/subgraph.md)
- src/dp/semantic/graph.py → [shortest_path_sql](entities/pythonsymbol/sh/shortest-path-sql.md)
- src/dp/semantic/graph.py → [connected_components](entities/pythonsymbol/co/connected-components.md)
- src/dp/semantic/document_chunker.py → [make_doc_id](entities/pythonsymbol/ma/make-doc-id.md)
- src/dp/semantic/document_chunker.py → [make_chunk_id](entities/pythonsymbol/ma/make-chunk-id.md)
- src/dp/semantic/document_chunker.py → [chunk_text](entities/pythonsymbol/ch/chunk-text.md)
- src/dp/semantic/document_chunker.py → [chunks_from_document](entities/pythonsymbol/ch/chunks-from-document.md)
- src/dp/semantic/document_chunker.py → [context_json_to_document](entities/pythonsymbol/co/context-json-to-document.md)

### CoupledWith

_90 `CoupledWith` relationships compiled — diagram omitted, too large to render usefully. First 15 shown below; every object's own detail page (linked) lists its full relationship set._

- tests/dp/semantic/test_embeddings.py → tests/dp/semantic/test_mcp_tools.py
- notebooks/bronze/dvdrental/generic_raw_to_bronze.py → tests/dp/io/test_delta.py
- conf/prod.yml → conf/test.yml
- notebooks/semantic/generate_embeddings.py → tests/dp/semantic/test_embeddings.py
- src/dp/semantic/graph.py → tests/dp/semantic/test_graph.py
- jobs/gold/dvdrental_silver_to_gold.yml → unknown
- tests/dp/transforms/test_bronze.py → tests/dp/transforms/test_cleaning.py
- notebooks/bronze/dvdrental/generic_raw_to_bronze.py → tests/dp/io/test_run_stats.py
- conf/prod.yml → databricks.yml
- notebooks/bronze/dvdrental/generic_raw_to_bronze.py → unknown
- README.md → TODO.md
- conf/dev.yml → conf/test.yml
- src/dp/io/delta.py → src/dp/transforms/bronze.py
- jobs/semantic/dvdrental_load_semantic_metadata.yml → notebooks/semantic/load_semantic_metadata.py
- conf/dev.yml → unknown

### DependsOn

_245 `DependsOn` relationships compiled — diagram omitted, too large to render usefully. First 15 shown below; every object's own detail page (linked) lists its full relationship set._

- src/dp/utils/secrets.py → [typing](entities/pythonmodule/ty/typing.md)
- notebooks/semantic/generate_semantic_tables.py → [dp.io.table](entities/pythonmodule/dp/dp-io-table.md)
- notebooks/semantic/generate_semantic_tables.py → [dp.utils.logger](entities/pythonmodule/dp/dp-utils-logger.md)
- src/dp/io/raw_source.py → [__future__](entities/pythonmodule/fu/future.md)
- src/dp/io/raw_source.py → [pyspark.sql](entities/pythonmodule/py/pyspark-sql.md)
- src/dp/io/raw_source.py → [dp.utils.logger](entities/pythonmodule/dp/dp-utils-logger.md)
- src/dp/semantic/graph.py → [__future__](entities/pythonmodule/fu/future.md)
- src/dp/semantic/graph.py → [pyspark.sql](entities/pythonmodule/py/pyspark-sql.md)
- src/dp/semantic/graph.py → [pyspark.sql.types](entities/pythonmodule/py/pyspark-sql-types.md)
- src/dp/semantic/document_chunker.py → [__future__](entities/pythonmodule/fu/future.md)
- src/dp/semantic/document_chunker.py → [hashlib](entities/pythonmodule/ha/hashlib.md)
- src/dp/semantic/document_chunker.py → [json](entities/pythonmodule/js/json.md)
- src/dp/semantic/document_chunker.py → [datetime](entities/pythonmodule/da/datetime.md)
- notebooks/semantic/create_graph_views.py → [sys](entities/pythonmodule/sy/sys.md)
- notebooks/semantic/create_graph_views.py → [dp.utils.logger](entities/pythonmodule/dp/dp-utils-logger.md)

### OwnedBy

_117 `OwnedBy` relationships compiled — diagram omitted, too large to render usefully. First 15 shown below; every object's own detail page (linked) lists its full relationship set._

- unknown → Aleksei Banaev
- unknown → Aleksei Banaev
- unknown → Aleksei Banaev
- unknown → Aleksei Banaev
- unknown → Aleksei Banaev
- unknown → Aleksei Banaev
- unknown → Aleksei Banaev
- unknown → Aleksei Banaev
- unknown → Aleksei Banaev
- unknown → Aleksei Banaev
- unknown → Aleksei Banaev
- unknown → Aleksei Banaev
- unknown → Aleksei Banaev
- unknown → Aleksei Banaev
- unknown → Aleksei Banaev

