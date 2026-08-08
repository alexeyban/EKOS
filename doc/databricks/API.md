# API

_Symbol names only, extracted via a lightweight text scan for declaration-line prefixes (`fn `, `def `, `class `, `func `, `interface `) — not a parsed API spec. Real `Api`/`Service` objects, if ever compiled, would render here directly; none are compiled today._

## CLAUDE.md

- `write_delta`
- `get_watermark`
- `update_watermark`
- `assert_row_count`
- `assert_no_nulls`
- `assert_unique`
- `get_secret`

## article_databricks_serverless.md

- `table_exists`
- `table_exists`
- `_run_row_count`
- `_run_not_null`
- `_run_unique`

## devlog_10.md

- `the`

## notebooks/semantic/extract_entities_llm.py

- `_llm`

## notebooks/semantic/generate_attribute_metadata.py

- `_llm`

## notebooks/semantic/generate_semantic_tables.py

- `_create_entity_table`
- `_create_rel_table`

## scripts/notebook_dryrun.py

- `_Widgets`
- `__init__`
- `text`
- `dropdown`
- `get`
- `_FsStub`
- `head`
- `put`
- `ls`
- `_SecretsStub`
- `get`
- `_DBUtils`
- `__init__`
- `notebook`
- `_get_local_spark`
- `_split_cells`
- `main`
- `_is_cluster_error`

## src/dp/io/delta.py

- `read_delta`
- `_build_update_cols`
- `write_delta`

## src/dp/io/raw_source.py

- `read_raw_snapshot`
- `read_keys_snapshot`

## src/dp/io/run_stats.py

- `_parse_export_type_udf`
- `read_adf_run_stats`

## src/dp/io/table.py

- `table_exists`
- `create_schema_if_not_exists`
- `create_table_if_not_exists`

## src/dp/metadata/loader.py

- `_load_json`
- `_validate`
- `_load_filtered_config`
- `load_ingestion_config`
- `load_dq_config`
- `load_transform_config`

## src/dp/metadata/semantic_loader.py

- `_load_json`
- `_load_json_adls`
- `load_source_metadata`
- `extract_source_system`
- `extract_entities`
- `extract_relationships`
- `extract_business_keys`
- `extract_columns`

## src/dp/quality/checks.py

- `DQValidationError`
- `run_expectations`
- `_total`

## src/dp/quality/reconciliation.py

- `compute_bronze_reconciliation`

## src/dp/quality/reporter.py

- `write_dq_results`
- `write_adf_run_stats`

## src/dp/semantic/document_chunker.py

- `make_doc_id`
- `make_chunk_id`
- `chunk_text`
- `chunks_from_document`
- `context_json_to_document`

## src/dp/semantic/embeddings.py

- `_get_class_fields`
- `_make_embedding_source_column`
- `build_vs_index_name`
- `batch_texts`
- `compute_embeddings`
- `client_fn`
- `create_or_sync_vs_index`
- `query_vs_index`

## src/dp/semantic/graph.py

- `vertex_degrees`
- `neighbors`
- `subgraph`
- `shortest_path_sql`
- `connected_components`

## src/dp/semantic/llm_enricher.py

- `call_llm`
- `build_entity_enrichment_prompt`
- `build_attribute_metadata_prompt`
- `_extract_json`
- `parse_entity_enrichment`
- `parse_attribute_metadata`
- `enrich_entities`
- `enrich_attributes`

## src/dp/semantic/mcp_tools.py

- `_sql_fetch`
- `_parse_context`
- `_vs_search`
- `search_customer`
- `get_customer_history`
- `find_related_films`
- `explain_entity`
- `search_documents`

## src/dp/semantic/ontology_loader.py

- `load_ontology_yaml`
- `extract_ontology_entities`
- `extract_ontology_relationships`
- `extract_ontology_attributes`

## src/dp/semantic/rules_loader.py

- `_load_json`
- `load_business_rules`
- `group_rules_by_entity`

## src/dp/semantic/visual_export.py

- `to_mermaid_er`
- `to_dbdiagram`
- `to_graphml`
- `to_json_schema`
- `to_json_schema_catalog`

## src/dp/transforms/bronze.py

- `add_metadata_columns`
- `detect_deleted_rows`
- `build_merge_dataframe`

## src/dp/transforms/cleaning.py

- `trim_char_columns`
- `resolve_boolean_flag`
- `drop_columns`
- `rename_columns`
- `cast_column_types`

## src/dp/transforms/schema.py

- `enforce_schema`
- `get_schema_diff`

## src/dp/utils/env.py

- `resolve_conf`
- `get_catalog`
- `get_kv_scope`

## src/dp/utils/logger.py

- `_JsonFormatter`
- `format`
- `get_logger`

## src/dp/utils/secrets.py

- `get_secret`

## tests/dp/conftest.py

- `spark`
- `actor_df`

## tests/dp/io/test_delta.py

- `TestWriteDeltaMerge`
- `test_merge_inserts_new_rows`
- `test_merge_does_not_overwrite_inserted_at`
- `test_merge_soft_delete_marks_row`

## tests/dp/io/test_raw_source.py

- `TestReadRawSnapshotAdlsParquet`
- `test_reads_parquet_from_adls_path`
- `test_reads_incremental_parquet`
- `test_raises_when_adls_base_missing_for_adls_layout`
- `test_raises_on_unknown_layout`
- `test_raises_when_raw_catalog_missing_for_uc_layout`
- `TestReadKeysSnapshot`
- `test_reads_keys_from_adls_path`
- `test_raises_when_adls_base_empty`
- `TestReadRawSnapshotAdlsParquetNested`
- `test_reads_parquet_from_nested_path_incremental`
- `test_reads_parquet_from_nested_path_full`
- `TestReadKeysSnapshotNested`
- `test_reads_parquet_keys_snapshot_nested`

## tests/dp/io/test_run_stats.py

- `TestParseExportTypeUdf`
- `test_incremental`
- `test_full`
- `test_multiword_entity_incremental`
- `test_multiword_entity_full`
- `test_unknown_suffix_returns_none`
- `test_empty_string_returns_none`
- `test_none_returns_none`
- `_unique_entity_name`
- `_write_stats_files`
- `TestReadAdfRunStats`
- `test_raises_on_empty_adls_base`
- `test_raises_on_empty_run_timestamp`
- `test_reads_single_entity_from_nested_path`
- `test_reads_multiple_entities`
- `test_multiword_entity_full`
- `test_skips_rows_with_unknown_export_type`

## tests/dp/io/test_table.py

- `TestTableExists`
- `test_returns_false_for_nonexistent_table`
- `test_returns_true_for_existing_table`

## tests/dp/metadata/test_loader.py

- `ingestion_config_file`
- `dq_config_file`
- `TestLoadIngestionConfig`
- `test_returns_active_only_by_default`
- `test_returns_all_when_active_only_false`
- `test_filters_by_entity_name`
- `test_returns_empty_for_unknown_entity`
- `TestLoadDqConfig`
- `test_returns_expectations_for_entity`
- `test_returns_empty_for_unknown_entity`

## tests/dp/metadata/test_semantic_loader.py

- `metadata`
- `metadata_file`
- `test_load_source_metadata_returns_dict`
- `test_load_source_metadata_validates_against_real_schema`
- `test_extract_source_system`
- `test_extract_entities_active_only`
- `test_extract_entities_include_inactive`
- `test_extract_entities_row_shape`
- `test_extract_relationships_active_only`
- `test_extract_relationships_include_inactive`
- `test_extract_relationships_row_shape`
- `test_extract_business_keys_single_pk`
- `test_extract_business_keys_composite_pk`
- `test_extract_business_keys_excludes_inactive`

## tests/dp/quality/test_checks.py

- `TestRunExpectations`
- `test_passes_on_valid_data`
- `test_fails_on_empty_dataframe`
- `test_returns_results_without_raising_when_disabled`
- `test_notes_key_in_kwargs_is_ignored`
- `test_unknown_expectation_type_is_skipped`
- `test_not_null_fails_on_null_values`
- `test_unique_passes_on_unique_values`
- `test_unique_fails_on_duplicates`
- `test_in_set_passes_on_valid_values`
- `test_in_set_fails_on_invalid_value`
- `test_between_passes_on_valid_range`
- `test_between_fails_on_out_of_range`

## tests/dp/quality/test_reconciliation.py

- `adf_stats_df`
- `TestComputeBronzeReconciliation`
- `_enrich`
- `test_match_when_counts_equal`
- `test_mismatch_detected`
- `test_bronze_has_more_rows_than_snapshot`
- `test_missing_bronze_table_produces_null_and_false`

## tests/dp/semantic/test_document_chunker.py

- `test_make_doc_id_is_deterministic`
- `test_make_doc_id_differs_by_type`
- `test_make_doc_id_length`
- `test_make_chunk_id_format`
- `test_make_chunk_id_pads_index`
- `test_chunk_text_empty`
- `test_chunk_text_short_text_is_single_chunk`
- `test_chunk_text_splits_long_text`
- `test_chunk_text_overlap_present`
- `test_chunk_text_invalid_overlap_raises`
- `test_chunk_text_exact_boundary`
- `test_chunks_from_document_returns_tuple`
- `test_chunks_from_document_doc_row_fields`
- `test_chunks_from_document_metadata_serialised`
- `test_chunks_from_document_chunk_fields`
- `test_chunks_from_document_empty_content`
- `test_chunks_from_document_multi_chunk`
- `test_context_json_to_document_roundtrip`
- `test_context_json_to_document_entity_version`

## tests/dp/semantic/test_embeddings.py

- `test_build_vs_index_name_pattern`
- `test_build_vs_index_name_film`
- `test_build_vs_index_name_document`
- `test_batch_texts_empty`
- `test_batch_texts_under_limit`
- `test_batch_texts_exact_batch`
- `test_batch_texts_splits_correctly`
- `test_batch_texts_default_batch_size`
- `test_batch_texts_preserves_content`
- `test_default_embedding_endpoint`
- `test_default_vs_endpoint`
- `_mock_client_fn`
- `test_compute_embeddings_returns_list`
- `test_compute_embeddings_order_preserved`
- `test_compute_embeddings_batches_correctly`
- `counting_client`
- `test_compute_embeddings_single_text`
- `_mock_sdk_ctx`
- `test_create_or_sync_creates_new_index`
- `test_create_or_sync_calls_sync_on_conflict`
- `test_create_or_sync_reraises_non_conflict_errors`
- `test_query_vs_index_returns_mapped_dicts`
- `test_query_vs_index_empty_when_no_data_array`

## tests/dp/semantic/test_graph.py

- `graph_views`
- `TestVertexDegrees`
- `test_out_degree_film1`
- `test_in_degree_actor1`
- `test_total_degree_actor1`
- `test_isolated_category_has_zero_out`
- `test_all_vertices_present`
- `TestNeighbors`
- `test_1hop_out_from_film1`
- `test_1hop_in_to_actor1`
- `test_2hop_out_from_film1_reaches_store`
- `test_start_vertex_excluded`
- `test_both_direction`
- `test_invalid_depth_raises`
- `test_invalid_direction_raises`
- `test_no_neighbors_returns_empty`
- `TestSubgraph`
- `test_vertex_count`
- `test_edge_count`
- `test_cross_boundary_edges_excluded`
- `test_empty_id_set_returns_empty`
- `TestConnectedComponents`
- `test_all_vertices_assigned`
- `test_one_component_for_connected_graph`
- `test_isolated_node_is_own_component`
- `test_component_is_minimum_id`
- `TestShortestPathSql`
- `test_direct_edge`
- `test_two_hop_path`
- `test_no_path_returns_empty_directed`
- `test_bidirectional_finds_reverse_path`
- `test_bidirectional_film_to_film_via_shared_actor`

## tests/dp/semantic/test_llm_enricher.py

- `TestBuildEntityEnrichmentPrompt`
- `test_includes_entity_type`
- `test_marks_primary_key_columns`
- `test_requests_json_output`
- `test_empty_columns_does_not_raise`
- `TestBuildAttributeMetadataPrompt`
- `test_includes_column_name`
- `test_notes_primary_key`
- `test_requests_sensitivity`
- `TestParseEntityEnrichment`
- `test_parses_valid_json`
- `test_strips_markdown_fences`
- `test_falls_back_on_invalid_json`
- `test_handles_missing_keys`
- `test_json_embedded_in_text`
- `TestParseAttributeMetadata`
- `test_parses_valid_json`
- `test_fallback_returns_internal_sensitivity`
- `TestEnrichEntities`
- `test_enriches_all_entities`
- `test_preserves_original_fields`
- `test_columns_are_passed_to_prompt`
- `mock_llm`
- `TestEnrichAttributes`
- `test_enriches_all_columns`
- `test_preserves_original_fields`

## tests/dp/semantic/test_mcp_tools.py

- `_make_spark`
- `_Row`
- `asDict`
- `_make_vs_ctx`
- `test_parse_context_parses_json_string`
- `test_parse_context_handles_bad_json`
- `test_parse_context_handles_missing_key`
- `test_parse_context_handles_dict_value`
- `test_vs_search_returns_rows`
- `test_vs_search_returns_empty_on_exception`
- `test_vs_search_returns_empty_when_no_data`
- `test_search_customer_sql_fallback`
- `test_search_customer_uses_vs_when_provided`
- `test_search_customer_falls_back_to_sql_when_vs_empty`
- `test_get_customer_history_structure`
- `_Row`
- `asDict`
- `_side_effect`
- `test_find_related_films_sql_fallback`
- `_Row`
- `asDict`
- `_side_effect`
- `test_find_related_films_excludes_source_from_vs_results`
- `_Row`
- `asDict`
- `test_explain_entity_returns_all_sections`
- `_Row`
- `asDict`
- `_side_effect`
- `test_search_documents_sql_fallback`
- `test_search_documents_doc_type_filter`
- `test_search_documents_no_filter`
- `test_search_documents_uses_vs_client`

## tests/dp/semantic/test_ontology_loader.py

- `ontology_file`
- `TestLoadOntologyYaml`
- `test_returns_dict`
- `test_loads_real_file`
- `TestExtractOntologyEntities`
- `test_returns_one_row_per_entity`
- `test_row_shape`
- `test_empty_ontology`
- `test_real_ontology_entity_count`
- `TestExtractOntologyRelationships`
- `test_returns_one_row_per_rel`
- `test_row_shape`
- `test_empty_relationships`
- `TestExtractOntologyAttributes`
- `test_returns_one_row_per_attribute`
- `test_row_shape`
- `test_units_field`
- `test_empty_attributes`

## tests/dp/semantic/test_rules_loader.py

- `rules_file`
- `TestLoadBusinessRules`
- `test_loads_all_rules_without_filter`
- `test_active_only_filters_inactive`
- `test_entity_filter`
- `test_loaded_at_is_added`
- `test_validates_against_real_schema`
- `test_real_rules_have_required_fields`
- `TestGroupRulesByEntity`
- `test_groups_correctly`
- `test_empty_input`
- `test_multiple_rules_same_entity`

## tests/dp/semantic/test_visual_export.py

- `TestToMermaidEr`
- `test_starts_with_er_diagram_keyword`
- `test_contains_entity_names`
- `test_contains_relationship`
- `test_no_duplicate_relationships`
- `test_empty_entities`
- `TestToDbdiagram`
- `test_contains_table_blocks`
- `test_pk_annotation`
- `test_contains_ref`
- `test_no_duplicate_refs`
- `TestToGraphml`
- `test_is_valid_xml_header`
- `test_contains_nodes`
- `test_contains_edge`
- `test_entity_class_attribute`
- `test_no_duplicate_edges`
- `TestToJsonSchema`
- `test_returns_valid_schema`
- `test_required_contains_pks`
- `test_properties_mapped_correctly`
- `test_date_format`
- `test_composite_pk`
- `TestToJsonSchemaCatalog`
- `test_definitions_contains_all_entities`
- `test_definitions_have_no_schema_key`
- `test_top_level_schema`

## tests/dp/transforms/test_bronze.py

- `TestAddMetadataColumns`
- `test_adds_four_columns`
- `test_is_deleted_defaults_false`
- `test_deleted_at_defaults_null`
- `test_timestamps_match_run_timestamp`
- `test_original_columns_preserved`
- `TestDetectDeletedRows`
- `test_returns_empty_on_first_run_no_table`
- `test_detects_missing_rows`
- `test_returns_empty_when_no_deletions`
- `test_composite_pk_detection`
- `test_keys_snapshot_df_used_for_anti_join`
- `test_keys_snapshot_df_none_falls_back_to_snapshot_pks`
- `TestBuildMergeDataframe`
- `test_returns_active_when_no_deletes`
- `test_unions_active_and_deleted`

## tests/dp/transforms/test_cleaning.py

- `TestTrimCharColumns`
- `test_trims_trailing_spaces`
- `test_skips_missing_columns`
- `test_trims_leading_spaces`
- `TestResolveBooleanFlag`
- `test_activebool_wins_over_active`
- `test_falls_back_to_active_when_activebool_null`
- `test_output_col_name_configurable`
- `TestDropColumns`
- `test_drops_existing_columns`
- `test_idempotent_on_missing_columns`
- `TestRenameColumns`
- `test_renames_existing_column`
- `test_skips_missing_source_column`
- `TestCastColumnTypes`
- `test_casts_string_to_int`
- `test_skips_missing_columns`

## tests/dp/transforms/test_schema.py

- `TestEnforceSchema`
- `test_selects_expected_columns`
- `test_casts_column_types`
- `test_raises_on_missing_required_column`
- `test_optional_missing_column_is_skipped`
- `TestGetSchemaDiff`
- `test_detects_added_columns`
- `test_detects_removed_columns`
- `test_detects_type_changes`
- `test_no_diff_on_matching_schema`

## tests/dp/utils/test_env.py

- `TestGetCatalog`
- `test_returns_catalog_from_conf`
- `test_unknown_env_raises`
- `TestGetKvScope`
- `test_returns_scope_from_conf`
- `test_unknown_env_raises`
- `TestResolveConf`
- `test_loads_dev_yml`
- `test_missing_file_raises`

## tests/integration/semantic/conftest.py

- `pytest_configure`
- `_skip_if_no_cluster`
- `catalog`
- `spark`

## tests/integration/semantic/test_dbt_smoke.py

- `test_semantic_context_tables_populated`
- `test_sem_customer_context_row_count`
- `test_sem_film_context_row_count`
- `test_entity_v_view_exists`
- `test_graph_vertex_view_exists`
- `test_attribute_metadata_populated`

## tests/integration/semantic/test_mcp_live.py

- `test_explain_entity_customer_returns_structure`
- `test_explain_entity_film_returns_structure`
- `test_search_customer_sql_fallback_no_error`
- `test_get_customer_history_structure`
- `test_find_related_films_sql_fallback`
- `test_search_documents_no_error`

## tests/integration/semantic/test_semantic_round_trip.py

- `test_business_rule_table_exists_and_has_rows`
- `test_business_rule_round_trip`
- `test_ontology_entity_table_populated`

