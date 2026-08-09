# API

_Program entities (functions, structs, enums, traits, classes, …) compiled from real Rust/Python source analysis, grouped by containing file. Each entity links to its own detail page (relationships, evidence, 1-hop diagram), written alongside this file. Real `Api`/`Service` objects, if a future connector ever compiles them, would render here directly._

## notebooks/semantic/extract_entities_llm.py

- `function` [`_llm`](entities/pythonsymbol/ll/llm.md)

## notebooks/semantic/generate_attribute_metadata.py

- `function` [`_llm`](entities/pythonsymbol/ll/llm-df2a5eef.md)

## notebooks/semantic/generate_semantic_tables.py

- `function` [`_create_entity_table`](entities/pythonsymbol/cr/create-entity-table.md)
- `function` [`_create_rel_table`](entities/pythonsymbol/cr/create-rel-table.md)

## scripts/notebook_dryrun.py

- `class` [`_DBUtils`](entities/pythonsymbol/db/dbutils.md)
- `class` [`_FsStub`](entities/pythonsymbol/fs/fsstub.md)
- `class` [`_SecretsStub`](entities/pythonsymbol/se/secretsstub.md)
- `class` [`_Widgets`](entities/pythonsymbol/wi/widgets.md)
- `function` [`_get_local_spark`](entities/pythonsymbol/ge/get-local-spark.md)
- `function` [`_split_cells`](entities/pythonsymbol/sp/split-cells.md)
- `function` [`main`](entities/pythonsymbol/ma/main.md)

## src/dp/io/delta.py

- `function` [`_build_update_cols`](entities/pythonsymbol/bu/build-update-cols.md)
- `function` [`read_delta`](entities/pythonsymbol/re/read-delta.md)
- `function` [`write_delta`](entities/pythonsymbol/wr/write-delta.md)

## src/dp/io/raw_source.py

- `function` [`read_keys_snapshot`](entities/pythonsymbol/re/read-keys-snapshot.md)
- `function` [`read_raw_snapshot`](entities/pythonsymbol/re/read-raw-snapshot.md)

## src/dp/io/run_stats.py

- `function` [`_parse_export_type_udf`](entities/pythonsymbol/pa/parse-export-type-udf.md)
- `function` [`read_adf_run_stats`](entities/pythonsymbol/re/read-adf-run-stats.md)

## src/dp/io/table.py

- `function` [`create_schema_if_not_exists`](entities/pythonsymbol/cr/create-schema-if-not-exists.md)
- `function` [`create_table_if_not_exists`](entities/pythonsymbol/cr/create-table-if-not-exists.md)
- `function` [`table_exists`](entities/pythonsymbol/ta/table-exists.md)

## src/dp/metadata/loader.py

- `function` [`_load_filtered_config`](entities/pythonsymbol/lo/load-filtered-config.md)
- `function` [`_load_json`](entities/pythonsymbol/lo/load-json.md)
- `function` [`_validate`](entities/pythonsymbol/va/validate.md)
- `function` [`load_dq_config`](entities/pythonsymbol/lo/load-dq-config.md)
- `function` [`load_ingestion_config`](entities/pythonsymbol/lo/load-ingestion-config.md)
- `function` [`load_transform_config`](entities/pythonsymbol/lo/load-transform-config.md)

## src/dp/metadata/semantic_loader.py

- `function` [`_load_json`](entities/pythonsymbol/lo/load-json-ee6d6ce5.md)
- `function` [`_load_json_adls`](entities/pythonsymbol/lo/load-json-adls.md)
- `function` [`extract_business_keys`](entities/pythonsymbol/ex/extract-business-keys.md)
- `function` [`extract_columns`](entities/pythonsymbol/ex/extract-columns.md)
- `function` [`extract_entities`](entities/pythonsymbol/ex/extract-entities.md)
- `function` [`extract_relationships`](entities/pythonsymbol/ex/extract-relationships.md)
- `function` [`extract_source_system`](entities/pythonsymbol/ex/extract-source-system.md)
- `function` [`load_source_metadata`](entities/pythonsymbol/lo/load-source-metadata.md)

## src/dp/quality/checks.py

- `class` [`DQValidationError`](entities/pythonsymbol/dq/dqvalidationerror.md)
- `function` [`run_expectations`](entities/pythonsymbol/ru/run-expectations.md)

## src/dp/quality/reconciliation.py

- `function` [`compute_bronze_reconciliation`](entities/pythonsymbol/co/compute-bronze-reconciliation.md)

## src/dp/quality/reporter.py

- `function` [`write_adf_run_stats`](entities/pythonsymbol/wr/write-adf-run-stats.md)
- `function` [`write_dq_results`](entities/pythonsymbol/wr/write-dq-results.md)

## src/dp/semantic/document_chunker.py

- `function` [`chunk_text`](entities/pythonsymbol/ch/chunk-text.md)
- `function` [`chunks_from_document`](entities/pythonsymbol/ch/chunks-from-document.md)
- `function` [`context_json_to_document`](entities/pythonsymbol/co/context-json-to-document.md)
- `function` [`make_chunk_id`](entities/pythonsymbol/ma/make-chunk-id.md)
- `function` [`make_doc_id`](entities/pythonsymbol/ma/make-doc-id.md)

## src/dp/semantic/embeddings.py

- `function` [`_get_class_fields`](entities/pythonsymbol/ge/get-class-fields.md)
- `function` [`_make_embedding_source_column`](entities/pythonsymbol/ma/make-embedding-source-column.md)
- `function` [`batch_texts`](entities/pythonsymbol/ba/batch-texts.md)
- `function` [`build_vs_index_name`](entities/pythonsymbol/bu/build-vs-index-name.md)
- `function` [`compute_embeddings`](entities/pythonsymbol/co/compute-embeddings.md)
- `function` [`create_or_sync_vs_index`](entities/pythonsymbol/cr/create-or-sync-vs-index.md)
- `function` [`query_vs_index`](entities/pythonsymbol/qu/query-vs-index.md)

## src/dp/semantic/graph.py

- `function` [`connected_components`](entities/pythonsymbol/co/connected-components.md)
- `function` [`neighbors`](entities/pythonsymbol/ne/neighbors.md)
- `function` [`shortest_path_sql`](entities/pythonsymbol/sh/shortest-path-sql.md)
- `function` [`subgraph`](entities/pythonsymbol/su/subgraph.md)
- `function` [`vertex_degrees`](entities/pythonsymbol/ve/vertex-degrees.md)

## src/dp/semantic/llm_enricher.py

- `function` [`_extract_json`](entities/pythonsymbol/ex/extract-json.md)
- `function` [`build_attribute_metadata_prompt`](entities/pythonsymbol/bu/build-attribute-metadata-prompt.md)
- `function` [`build_entity_enrichment_prompt`](entities/pythonsymbol/bu/build-entity-enrichment-prompt.md)
- `function` [`call_llm`](entities/pythonsymbol/ca/call-llm.md)
- `function` [`enrich_attributes`](entities/pythonsymbol/en/enrich-attributes.md)
- `function` [`enrich_entities`](entities/pythonsymbol/en/enrich-entities.md)
- `function` [`parse_attribute_metadata`](entities/pythonsymbol/pa/parse-attribute-metadata.md)
- `function` [`parse_entity_enrichment`](entities/pythonsymbol/pa/parse-entity-enrichment.md)

## src/dp/semantic/mcp_tools.py

- `function` [`_parse_context`](entities/pythonsymbol/pa/parse-context.md)
- `function` [`_sql_fetch`](entities/pythonsymbol/sq/sql-fetch.md)
- `function` [`_vs_search`](entities/pythonsymbol/vs/vs-search.md)
- `function` [`explain_entity`](entities/pythonsymbol/ex/explain-entity.md)
- `function` [`find_related_films`](entities/pythonsymbol/fi/find-related-films.md)
- `function` [`get_customer_history`](entities/pythonsymbol/ge/get-customer-history.md)
- `function` [`search_customer`](entities/pythonsymbol/se/search-customer.md)
- `function` [`search_documents`](entities/pythonsymbol/se/search-documents.md)

## src/dp/semantic/ontology_loader.py

- `function` [`extract_ontology_attributes`](entities/pythonsymbol/ex/extract-ontology-attributes.md)
- `function` [`extract_ontology_entities`](entities/pythonsymbol/ex/extract-ontology-entities.md)
- `function` [`extract_ontology_relationships`](entities/pythonsymbol/ex/extract-ontology-relationships.md)
- `function` [`load_ontology_yaml`](entities/pythonsymbol/lo/load-ontology-yaml.md)

## src/dp/semantic/rules_loader.py

- `function` [`_load_json`](entities/pythonsymbol/lo/load-json-a0ebef22.md)
- `function` [`group_rules_by_entity`](entities/pythonsymbol/gr/group-rules-by-entity.md)
- `function` [`load_business_rules`](entities/pythonsymbol/lo/load-business-rules.md)

## src/dp/semantic/visual_export.py

- `function` [`to_dbdiagram`](entities/pythonsymbol/to/to-dbdiagram.md)
- `function` [`to_graphml`](entities/pythonsymbol/to/to-graphml.md)
- `function` [`to_json_schema`](entities/pythonsymbol/to/to-json-schema.md)
- `function` [`to_json_schema_catalog`](entities/pythonsymbol/to/to-json-schema-catalog.md)
- `function` [`to_mermaid_er`](entities/pythonsymbol/to/to-mermaid-er.md)

## src/dp/transforms/bronze.py

- `function` [`add_metadata_columns`](entities/pythonsymbol/ad/add-metadata-columns.md)
- `function` [`build_merge_dataframe`](entities/pythonsymbol/bu/build-merge-dataframe.md)
- `function` [`detect_deleted_rows`](entities/pythonsymbol/de/detect-deleted-rows.md)

## src/dp/transforms/cleaning.py

- `function` [`cast_column_types`](entities/pythonsymbol/ca/cast-column-types.md)
- `function` [`drop_columns`](entities/pythonsymbol/dr/drop-columns.md)
- `function` [`rename_columns`](entities/pythonsymbol/re/rename-columns.md)
- `function` [`resolve_boolean_flag`](entities/pythonsymbol/re/resolve-boolean-flag.md)
- `function` [`trim_char_columns`](entities/pythonsymbol/tr/trim-char-columns.md)

## src/dp/transforms/schema.py

- `function` [`enforce_schema`](entities/pythonsymbol/en/enforce-schema.md)
- `function` [`get_schema_diff`](entities/pythonsymbol/ge/get-schema-diff.md)

## src/dp/utils/env.py

- `function` [`get_catalog`](entities/pythonsymbol/ge/get-catalog.md)
- `function` [`get_kv_scope`](entities/pythonsymbol/ge/get-kv-scope.md)
- `function` [`resolve_conf`](entities/pythonsymbol/re/resolve-conf.md)

## src/dp/utils/logger.py

- `class` [`_JsonFormatter`](entities/pythonsymbol/js/jsonformatter.md)
- `function` [`get_logger`](entities/pythonsymbol/ge/get-logger.md)

## src/dp/utils/secrets.py

- `function` [`get_secret`](entities/pythonsymbol/ge/get-secret.md)

## tests/dp/conftest.py

- `function` [`actor_df`](entities/pythonsymbol/ac/actor-df.md)
- `function` [`spark`](entities/pythonsymbol/sp/spark-898b0b4f.md)

## tests/dp/io/test_delta.py

- `class` [`TestWriteDeltaMerge`](entities/pythonsymbol/te/testwritedeltamerge.md)

## tests/dp/io/test_raw_source.py

- `class` [`TestReadKeysSnapshot`](entities/pythonsymbol/te/testreadkeyssnapshot.md)
- `class` [`TestReadKeysSnapshotNested`](entities/pythonsymbol/te/testreadkeyssnapshotnested.md)
- `class` [`TestReadRawSnapshotAdlsParquet`](entities/pythonsymbol/te/testreadrawsnapshotadlsparquet.md)
- `class` [`TestReadRawSnapshotAdlsParquetNested`](entities/pythonsymbol/te/testreadrawsnapshotadlsparquetnested.md)

## tests/dp/io/test_run_stats.py

- `class` [`TestParseExportTypeUdf`](entities/pythonsymbol/te/testparseexporttypeudf.md)
- `class` [`TestReadAdfRunStats`](entities/pythonsymbol/te/testreadadfrunstats.md)
- `function` [`_unique_entity_name`](entities/pythonsymbol/un/unique-entity-name.md)
- `function` [`_write_stats_files`](entities/pythonsymbol/wr/write-stats-files.md)

## tests/dp/io/test_table.py

- `class` [`TestTableExists`](entities/pythonsymbol/te/testtableexists.md)

## tests/dp/metadata/test_loader.py

- `class` [`TestLoadDqConfig`](entities/pythonsymbol/te/testloaddqconfig.md)
- `class` [`TestLoadIngestionConfig`](entities/pythonsymbol/te/testloadingestionconfig.md)
- `function` [`dq_config_file`](entities/pythonsymbol/dq/dq-config-file.md)
- `function` [`ingestion_config_file`](entities/pythonsymbol/in/ingestion-config-file.md)

## tests/dp/metadata/test_semantic_loader.py

- `function` [`metadata`](entities/pythonsymbol/me/metadata.md)
- `function` [`metadata_file`](entities/pythonsymbol/me/metadata-file.md)
- `function` [`test_extract_business_keys_composite_pk`](entities/pythonsymbol/te/test-extract-business-keys-composite-pk.md)
- `function` [`test_extract_business_keys_excludes_inactive`](entities/pythonsymbol/te/test-extract-business-keys-excludes-inactive.md)
- `function` [`test_extract_business_keys_single_pk`](entities/pythonsymbol/te/test-extract-business-keys-single-pk.md)
- `function` [`test_extract_entities_active_only`](entities/pythonsymbol/te/test-extract-entities-active-only.md)
- `function` [`test_extract_entities_include_inactive`](entities/pythonsymbol/te/test-extract-entities-include-inactive.md)
- `function` [`test_extract_entities_row_shape`](entities/pythonsymbol/te/test-extract-entities-row-shape.md)
- `function` [`test_extract_relationships_active_only`](entities/pythonsymbol/te/test-extract-relationships-active-only.md)
- `function` [`test_extract_relationships_include_inactive`](entities/pythonsymbol/te/test-extract-relationships-include-inactive.md)
- `function` [`test_extract_relationships_row_shape`](entities/pythonsymbol/te/test-extract-relationships-row-shape.md)
- `function` [`test_extract_source_system`](entities/pythonsymbol/te/test-extract-source-system.md)
- `function` [`test_load_source_metadata_returns_dict`](entities/pythonsymbol/te/test-load-source-metadata-returns-dict.md)
- `function` [`test_load_source_metadata_validates_against_real_schema`](entities/pythonsymbol/te/test-load-source-metadata-validates-against-real-schema.md)

## tests/dp/quality/test_checks.py

- `class` [`TestRunExpectations`](entities/pythonsymbol/te/testrunexpectations.md)

## tests/dp/quality/test_reconciliation.py

- `class` [`TestComputeBronzeReconciliation`](entities/pythonsymbol/te/testcomputebronzereconciliation.md)
- `function` [`adf_stats_df`](entities/pythonsymbol/ad/adf-stats-df.md)

## tests/dp/semantic/test_document_chunker.py

- `function` [`test_chunk_text_empty`](entities/pythonsymbol/te/test-chunk-text-empty.md)
- `function` [`test_chunk_text_exact_boundary`](entities/pythonsymbol/te/test-chunk-text-exact-boundary.md)
- `function` [`test_chunk_text_invalid_overlap_raises`](entities/pythonsymbol/te/test-chunk-text-invalid-overlap-raises.md)
- `function` [`test_chunk_text_overlap_present`](entities/pythonsymbol/te/test-chunk-text-overlap-present.md)
- `function` [`test_chunk_text_short_text_is_single_chunk`](entities/pythonsymbol/te/test-chunk-text-short-text-is-single-chunk.md)
- `function` [`test_chunk_text_splits_long_text`](entities/pythonsymbol/te/test-chunk-text-splits-long-text.md)
- `function` [`test_chunks_from_document_chunk_fields`](entities/pythonsymbol/te/test-chunks-from-document-chunk-fields.md)
- `function` [`test_chunks_from_document_doc_row_fields`](entities/pythonsymbol/te/test-chunks-from-document-doc-row-fields.md)
- `function` [`test_chunks_from_document_empty_content`](entities/pythonsymbol/te/test-chunks-from-document-empty-content.md)
- `function` [`test_chunks_from_document_metadata_serialised`](entities/pythonsymbol/te/test-chunks-from-document-metadata-serialised.md)
- `function` [`test_chunks_from_document_multi_chunk`](entities/pythonsymbol/te/test-chunks-from-document-multi-chunk.md)
- `function` [`test_chunks_from_document_returns_tuple`](entities/pythonsymbol/te/test-chunks-from-document-returns-tuple.md)
- `function` [`test_context_json_to_document_entity_version`](entities/pythonsymbol/te/test-context-json-to-document-entity-version.md)
- `function` [`test_context_json_to_document_roundtrip`](entities/pythonsymbol/te/test-context-json-to-document-roundtrip.md)
- `function` [`test_make_chunk_id_format`](entities/pythonsymbol/te/test-make-chunk-id-format.md)
- `function` [`test_make_chunk_id_pads_index`](entities/pythonsymbol/te/test-make-chunk-id-pads-index.md)
- `function` [`test_make_doc_id_differs_by_type`](entities/pythonsymbol/te/test-make-doc-id-differs-by-type.md)
- `function` [`test_make_doc_id_is_deterministic`](entities/pythonsymbol/te/test-make-doc-id-is-deterministic.md)
- `function` [`test_make_doc_id_length`](entities/pythonsymbol/te/test-make-doc-id-length.md)

## tests/dp/semantic/test_embeddings.py

- `function` [`_mock_client_fn`](entities/pythonsymbol/mo/mock-client-fn.md)
- `function` [`_mock_sdk_ctx`](entities/pythonsymbol/mo/mock-sdk-ctx.md)
- `function` [`test_batch_texts_default_batch_size`](entities/pythonsymbol/te/test-batch-texts-default-batch-size.md)
- `function` [`test_batch_texts_empty`](entities/pythonsymbol/te/test-batch-texts-empty.md)
- `function` [`test_batch_texts_exact_batch`](entities/pythonsymbol/te/test-batch-texts-exact-batch.md)
- `function` [`test_batch_texts_preserves_content`](entities/pythonsymbol/te/test-batch-texts-preserves-content.md)
- `function` [`test_batch_texts_splits_correctly`](entities/pythonsymbol/te/test-batch-texts-splits-correctly.md)
- `function` [`test_batch_texts_under_limit`](entities/pythonsymbol/te/test-batch-texts-under-limit.md)
- `function` [`test_build_vs_index_name_document`](entities/pythonsymbol/te/test-build-vs-index-name-document.md)
- `function` [`test_build_vs_index_name_film`](entities/pythonsymbol/te/test-build-vs-index-name-film.md)
- `function` [`test_build_vs_index_name_pattern`](entities/pythonsymbol/te/test-build-vs-index-name-pattern.md)
- `function` [`test_compute_embeddings_batches_correctly`](entities/pythonsymbol/te/test-compute-embeddings-batches-correctly.md)
- `function` [`test_compute_embeddings_order_preserved`](entities/pythonsymbol/te/test-compute-embeddings-order-preserved.md)
- `function` [`test_compute_embeddings_returns_list`](entities/pythonsymbol/te/test-compute-embeddings-returns-list.md)
- `function` [`test_compute_embeddings_single_text`](entities/pythonsymbol/te/test-compute-embeddings-single-text.md)
- `function` [`test_create_or_sync_calls_sync_on_conflict`](entities/pythonsymbol/te/test-create-or-sync-calls-sync-on-conflict.md)
- `function` [`test_create_or_sync_creates_new_index`](entities/pythonsymbol/te/test-create-or-sync-creates-new-index.md)
- `function` [`test_create_or_sync_reraises_non_conflict_errors`](entities/pythonsymbol/te/test-create-or-sync-reraises-non-conflict-errors.md)
- `function` [`test_default_embedding_endpoint`](entities/pythonsymbol/te/test-default-embedding-endpoint.md)
- `function` [`test_default_vs_endpoint`](entities/pythonsymbol/te/test-default-vs-endpoint.md)
- `function` [`test_query_vs_index_empty_when_no_data_array`](entities/pythonsymbol/te/test-query-vs-index-empty-when-no-data-array.md)
- `function` [`test_query_vs_index_returns_mapped_dicts`](entities/pythonsymbol/te/test-query-vs-index-returns-mapped-dicts.md)

## tests/dp/semantic/test_graph.py

- `class` [`TestConnectedComponents`](entities/pythonsymbol/te/testconnectedcomponents.md)
- `class` [`TestNeighbors`](entities/pythonsymbol/te/testneighbors.md)
- `class` [`TestShortestPathSql`](entities/pythonsymbol/te/testshortestpathsql.md)
- `class` [`TestSubgraph`](entities/pythonsymbol/te/testsubgraph.md)
- `class` [`TestVertexDegrees`](entities/pythonsymbol/te/testvertexdegrees.md)
- `function` [`graph_views`](entities/pythonsymbol/gr/graph-views.md)

## tests/dp/semantic/test_llm_enricher.py

- `class` [`TestBuildAttributeMetadataPrompt`](entities/pythonsymbol/te/testbuildattributemetadataprompt.md)
- `class` [`TestBuildEntityEnrichmentPrompt`](entities/pythonsymbol/te/testbuildentityenrichmentprompt.md)
- `class` [`TestEnrichAttributes`](entities/pythonsymbol/te/testenrichattributes.md)
- `class` [`TestEnrichEntities`](entities/pythonsymbol/te/testenrichentities.md)
- `class` [`TestParseAttributeMetadata`](entities/pythonsymbol/te/testparseattributemetadata.md)
- `class` [`TestParseEntityEnrichment`](entities/pythonsymbol/te/testparseentityenrichment.md)

## tests/dp/semantic/test_mcp_tools.py

- `function` [`_make_spark`](entities/pythonsymbol/ma/make-spark.md)
- `function` [`_make_vs_ctx`](entities/pythonsymbol/ma/make-vs-ctx.md)
- `function` [`test_explain_entity_returns_all_sections`](entities/pythonsymbol/te/test-explain-entity-returns-all-sections.md)
- `function` [`test_find_related_films_excludes_source_from_vs_results`](entities/pythonsymbol/te/test-find-related-films-excludes-source-from-vs-results.md)
- `function` [`test_find_related_films_sql_fallback`](entities/pythonsymbol/te/test-find-related-films-sql-fallback.md)
- `function` [`test_get_customer_history_structure`](entities/pythonsymbol/te/test-get-customer-history-structure-f02891b1.md)
- `function` [`test_parse_context_handles_bad_json`](entities/pythonsymbol/te/test-parse-context-handles-bad-json.md)
- `function` [`test_parse_context_handles_dict_value`](entities/pythonsymbol/te/test-parse-context-handles-dict-value.md)
- `function` [`test_parse_context_handles_missing_key`](entities/pythonsymbol/te/test-parse-context-handles-missing-key.md)
- `function` [`test_parse_context_parses_json_string`](entities/pythonsymbol/te/test-parse-context-parses-json-string.md)
- `function` [`test_search_customer_falls_back_to_sql_when_vs_empty`](entities/pythonsymbol/te/test-search-customer-falls-back-to-sql-when-vs-empty.md)
- `function` [`test_search_customer_sql_fallback`](entities/pythonsymbol/te/test-search-customer-sql-fallback.md)
- `function` [`test_search_customer_uses_vs_when_provided`](entities/pythonsymbol/te/test-search-customer-uses-vs-when-provided.md)
- `function` [`test_search_documents_doc_type_filter`](entities/pythonsymbol/te/test-search-documents-doc-type-filter.md)
- `function` [`test_search_documents_no_filter`](entities/pythonsymbol/te/test-search-documents-no-filter.md)
- `function` [`test_search_documents_sql_fallback`](entities/pythonsymbol/te/test-search-documents-sql-fallback.md)
- `function` [`test_search_documents_uses_vs_client`](entities/pythonsymbol/te/test-search-documents-uses-vs-client.md)
- `function` [`test_vs_search_returns_empty_on_exception`](entities/pythonsymbol/te/test-vs-search-returns-empty-on-exception.md)
- `function` [`test_vs_search_returns_empty_when_no_data`](entities/pythonsymbol/te/test-vs-search-returns-empty-when-no-data.md)
- `function` [`test_vs_search_returns_rows`](entities/pythonsymbol/te/test-vs-search-returns-rows.md)

## tests/dp/semantic/test_ontology_loader.py

- `class` [`TestExtractOntologyAttributes`](entities/pythonsymbol/te/testextractontologyattributes.md)
- `class` [`TestExtractOntologyEntities`](entities/pythonsymbol/te/testextractontologyentities.md)
- `class` [`TestExtractOntologyRelationships`](entities/pythonsymbol/te/testextractontologyrelationships.md)
- `class` [`TestLoadOntologyYaml`](entities/pythonsymbol/te/testloadontologyyaml.md)
- `function` [`ontology_file`](entities/pythonsymbol/on/ontology-file.md)

## tests/dp/semantic/test_rules_loader.py

- `class` [`TestGroupRulesByEntity`](entities/pythonsymbol/te/testgrouprulesbyentity.md)
- `class` [`TestLoadBusinessRules`](entities/pythonsymbol/te/testloadbusinessrules.md)
- `function` [`rules_file`](entities/pythonsymbol/ru/rules-file.md)

## tests/dp/semantic/test_visual_export.py

- `class` [`TestToDbdiagram`](entities/pythonsymbol/te/testtodbdiagram.md)
- `class` [`TestToGraphml`](entities/pythonsymbol/te/testtographml.md)
- `class` [`TestToJsonSchema`](entities/pythonsymbol/te/testtojsonschema.md)
- `class` [`TestToJsonSchemaCatalog`](entities/pythonsymbol/te/testtojsonschemacatalog.md)
- `class` [`TestToMermaidEr`](entities/pythonsymbol/te/testtomermaider.md)

## tests/dp/transforms/test_bronze.py

- `class` [`TestAddMetadataColumns`](entities/pythonsymbol/te/testaddmetadatacolumns.md)
- `class` [`TestBuildMergeDataframe`](entities/pythonsymbol/te/testbuildmergedataframe.md)
- `class` [`TestDetectDeletedRows`](entities/pythonsymbol/te/testdetectdeletedrows.md)

## tests/dp/transforms/test_cleaning.py

- `class` [`TestCastColumnTypes`](entities/pythonsymbol/te/testcastcolumntypes.md)
- `class` [`TestDropColumns`](entities/pythonsymbol/te/testdropcolumns.md)
- `class` [`TestRenameColumns`](entities/pythonsymbol/te/testrenamecolumns.md)
- `class` [`TestResolveBooleanFlag`](entities/pythonsymbol/te/testresolvebooleanflag.md)
- `class` [`TestTrimCharColumns`](entities/pythonsymbol/te/testtrimcharcolumns.md)

## tests/dp/transforms/test_schema.py

- `class` [`TestEnforceSchema`](entities/pythonsymbol/te/testenforceschema.md)
- `class` [`TestGetSchemaDiff`](entities/pythonsymbol/te/testgetschemadiff.md)

## tests/dp/utils/test_env.py

- `class` [`TestGetCatalog`](entities/pythonsymbol/te/testgetcatalog.md)
- `class` [`TestGetKvScope`](entities/pythonsymbol/te/testgetkvscope.md)
- `class` [`TestResolveConf`](entities/pythonsymbol/te/testresolveconf.md)

## tests/integration/semantic/conftest.py

- `function` [`_skip_if_no_cluster`](entities/pythonsymbol/sk/skip-if-no-cluster.md)
- `function` [`catalog`](entities/pythonsymbol/ca/catalog.md)
- `function` [`pytest_configure`](entities/pythonsymbol/py/pytest-configure.md)
- `function` [`spark`](entities/pythonsymbol/sp/spark.md)

## tests/integration/semantic/test_dbt_smoke.py

- `function` [`test_attribute_metadata_populated`](entities/pythonsymbol/te/test-attribute-metadata-populated.md)
- `function` [`test_entity_v_view_exists`](entities/pythonsymbol/te/test-entity-v-view-exists.md)
- `function` [`test_graph_vertex_view_exists`](entities/pythonsymbol/te/test-graph-vertex-view-exists.md)
- `function` [`test_sem_customer_context_row_count`](entities/pythonsymbol/te/test-sem-customer-context-row-count.md)
- `function` [`test_sem_film_context_row_count`](entities/pythonsymbol/te/test-sem-film-context-row-count.md)
- `function` [`test_semantic_context_tables_populated`](entities/pythonsymbol/te/test-semantic-context-tables-populated.md)

## tests/integration/semantic/test_mcp_live.py

- `function` [`test_explain_entity_customer_returns_structure`](entities/pythonsymbol/te/test-explain-entity-customer-returns-structure.md)
- `function` [`test_explain_entity_film_returns_structure`](entities/pythonsymbol/te/test-explain-entity-film-returns-structure.md)
- `function` [`test_find_related_films_sql_fallback`](entities/pythonsymbol/te/test-find-related-films-sql-fallback-be802b72.md)
- `function` [`test_get_customer_history_structure`](entities/pythonsymbol/te/test-get-customer-history-structure.md)
- `function` [`test_search_customer_sql_fallback_no_error`](entities/pythonsymbol/te/test-search-customer-sql-fallback-no-error.md)
- `function` [`test_search_documents_no_error`](entities/pythonsymbol/te/test-search-documents-no-error.md)

## tests/integration/semantic/test_semantic_round_trip.py

- `function` [`test_business_rule_round_trip`](entities/pythonsymbol/te/test-business-rule-round-trip.md)
- `function` [`test_business_rule_table_exists_and_has_rows`](entities/pythonsymbol/te/test-business-rule-table-exists-and-has-rows.md)
- `function` [`test_ontology_entity_table_populated`](entities/pythonsymbol/te/test-ontology-entity-table-populated.md)

