# API

_Symbol names only, extracted via a lightweight text scan for declaration-line prefixes (`fn `, `def `, `class `, `func `, `interface `) — not a parsed API spec. Real `Api`/`Service` objects, if ever compiled, would render here directly; none are compiled today._

## TODO.md

- `the`
- `boundary`

## benchmark/benches/fact_ledger.rs

- `object`
- `bench_fact_ledger`

## benchmark/benches/fact_model.rs

- `realistic_object`
- `bench_fact_model`

## benchmark/benches/identity_resolver.rs

- `fixture_graph`
- `bench_identity_resolver`

## benchmark/benches/index_runs.rs

- `build_indexes`
- `bench_index_runs`

## benchmark/benches/ledger_write.rs

- `bench_ledger_write`

## benchmark/benches/observation_git.rs

- `fixture_repo`
- `bench_observation_git`

## benchmark/benches/runtime_load_neighborhood.rs

- `seed_ledger`
- `bench_load_neighborhood`

## benchmark/benches/segment_store.rs

- `ops`
- `bench_segment_store`

## benchmark/benches/semantic_compiler.rs

- `fixture_graph`
- `bench_semantic_compiler`

## benchmark/benches/sql_analyzer.rs

- `bench_sql_analyzer`

## benchmark/benches/storage_compaction.rs

- `realistic_object`
- `ledger_file_bytes`
- `populated_ledger`
- `bench_storage`

## docs/rfcs/0001-compiler-core.md

- `name`
- `dependencies`

## docs/rfcs/0002-artifact-system.md

- `id`
- `artifact_type`
- `dependencies`
- `schema_version`

## docs/rfcs/0006-observation-sdk.md

- `name`

## docs/rfcs/0008-llm-policy.md

- `model_name`

## docs/rfcs/0011-optimizer.md

- `version`

## docs/rfcs/0012-enterprise-connectors-scaffold.md

- `name`

## docs/rfcs/0017-crypto-connector.md

- `name`

## docs/rfcs/0023-local-document-connector.md

- `supported_extension`
- `parse`
- `recognize`

## docs/spikes/recovery_spike.py

- `call_claude`
- `evaluate`
- `main`

## ekos/crates/artifact/src/lib.rs

- `fmt`
- `canonicalize`
- `compute_content_id`
- `default`
- `same_content_same_id`
- `different_content_different_id`
- `volatile_metadata_excluded_from_id`
- `observation_artifact_round_trip`
- `index_artifact_round_trip`
- `canonicalize_sorts_keys`

## ekos/crates/artifact/src/pack.rs

- `segment_path`
- `loose_path`
- `write_packed`
- `drop`
- `write`
- `read`
- `exists`
- `list`
- `compress_frame_body`
- `hex_id_to_raw`
- `segment_paths`
- `scan_segment`
- `prune_empty_dirs`
- `id_of`
- `sample`
- `write_read_round_trip_and_cache_hit`
- `index_survives_reopen_without_sidecar`
- `torn_tail_is_truncated_and_prior_frames_survive`
- `corrupt_frame_body_is_detected_on_read`
- `reads_fall_back_to_loose_files_and_repack_migrates_them`
- `packed_storage_is_smaller_than_loose`
- `walk_bytes`

## ekos/crates/artifact/src/store.rs

- `write`
- `read`
- `exists`
- `list`
- `artifact_path`
- `write`
- `read`
- `exists`
- `list`
- `make_store`
- `write_and_read_round_trip`
- `second_write_is_cache_hit`
- `read_missing_returns_none`
- `git_object_layout`
- `list_returns_stored_ids`

## ekos/crates/cli/src/commands/ask.rs

- `ask_selects_ollama_provider_when_configured`

## ekos/crates/cli/src/commands/branch.rs

- `branch_path`
- `open_branch`

## ekos/crates/cli/src/commands/build.rs

- `load_fingerprints`
- `save_fingerprints`
- `prune_snapshots`

## ekos/crates/cli/src/commands/commit.rs

- `open_ledger`
- `ckm_rel_to_kir`
- `ckm_object_to_kir`
- `evidence_record_to_kir`

## ekos/crates/cli/src/commands/compile.rs

- `knowledge_artifact_ids`

## ekos/crates/cli/src/commands/dbt.rs

- `write_model`
- `transform_node`
- `resolve_output_dir_defaults_to_dbt_generated`

## ekos/crates/cli/src/commands/docs.rs

- `estimate_prompt_tokens`
- `confirm_prose_spend`
- `select_llm_provider_for_prose`
- `render_er_diagram_page`
- `write_page`
- `generate_curated`
- `resolve_output_dir_defaults_to_docs_generated`
- `format_parse_accepts_md_markdown_and_html_rejects_unknown`
- `estimate_prompt_tokens_grows_with_model_content`
- `confirm_prose_spend_auto_skips_the_prompt`
- `layout_parse_accepts_objects_and_curated_rejects_unknown`

## ekos/crates/cli/src/commands/doctor.rs

- `ok`
- `fail`

## ekos/crates/cli/src/commands/ekl.rs

- `render_cell`

## ekos/crates/cli/src/commands/identity.rs

- `seed_table`
- `scan_writes_unconfirmed_same_as_relationships`
- `rescan_does_not_duplicate_known_candidates`
- `scan_on_empty_ledger_writes_nothing`

## ekos/crates/cli/src/commands/ledger.rs

- `migrate_v3`
- `print_storage_report`

## ekos/crates/cli/src/commands/marketing.rs

- `select_llm_provider`
- `resolve_devlog_path`
- `approve`
- `log_line`
- `dotenv_file_populates_the_process_environment`
- `dotenv_file_never_overrides_an_already_set_var`
- `missing_dotenv_file_is_a_silent_no_op`
- `resolve_devlog_path_none_finds_latest`
- `resolve_devlog_path_accepts_bare_number`
- `resolve_devlog_path_bare_number_missing_file_is_none`
- `resolve_devlog_path_accepts_explicit_relative_path`
- `approve_with_auto_true_skips_prompt`

## ekos/crates/cli/src/commands/mcp.rs

- `ok_response`
- `error_response`
- `initialize_result`
- `tool_definitions`
- `tools_call`
- `call_tool`
- `transformation_chain`
- `explain_node`
- `node_summary`
- `node_comparable`
- `diff_chains`
- `bucket`
- `set_diff`
- `required_str`
- `required_id`
- `req`
- `parse`
- `initialize_echoes_protocol_version_and_names_server`
- `notifications_are_never_answered`
- `unknown_method_returns_method_not_found`
- `tools_list_exposes_the_runtime_tools`
- `dependents_of_unknown_object_is_a_tool_error`
- `impact_of_unknown_object_is_a_tool_error`
- `seeded_ledger`
- `impact_traces_multi_hop_dependents`
- `impact_with_invalid_direction_is_a_tool_error`
- `diff_on_fresh_workspace_reports_nothing_changed`
- `diff_with_bad_timestamp_is_a_tool_error`
- `status_works_on_a_fresh_workspace`
- `search_returns_empty_matches_on_a_fresh_workspace`
- `ekl_syntax_error_is_a_tool_error_not_a_protocol_error`
- `unknown_tool_is_reported_as_tool_error`
- `malformed_json_returns_parse_error_with_null_id`
- `seeded_transformation_ledger`
- `explain_walks_the_full_chain_with_evidence`
- `explain_of_unknown_object_is_a_tool_error`
- `diff_detects_added_and_removed_filter`
- `diff_of_identical_chains_reports_no_differences`
- `seeded_same_as_relationship`
- `identity_review_confirms_a_candidate_and_writes_an_event`
- `identity_review_rejects_a_candidate`
- `identity_review_with_invalid_decision_is_a_tool_error`
- `identity_review_of_non_same_as_relationship_is_a_tool_error`
- `identity_review_of_unknown_relationship_is_a_tool_error`

## ekos/crates/cli/src/commands/query.rs

- `open_ledger`

## ekos/crates/cli/src/commands/recover.rs

- `collect_git_artifact_ids`
- `collect_crypto_artifact_ids`
- `collect_github_artifact_ids`
- `collect_confluence_artifact_ids`
- `collect_localdocs_artifact_ids`
- `collect_pentaho_artifact_ids`
- `collect_python_artifact_ids`
- `collect_rust_artifact_ids`
- `should_register_document_semantics`
- `document_semantics_pass_not_registered_when_config_absent`
- `document_semantics_pass_not_registered_when_explicitly_disabled`
- `document_semantics_pass_not_registered_without_local_documents`
- `document_semantics_pass_registered_when_enabled_with_local_documents`
- `ollama_provider_selected_when_configured`
- `non_ollama_provider_falls_back_to_existing_chain`

## ekos/crates/cli/src/commands/resolve.rs

- `merge_into`

## ekos/crates/cli/tests/mcp_session.rs

- `setup_workspace`
- `load_config`
- `call_tool`

## ekos/crates/cli/tests/skeleton.rs

- `setup_workspace`
- `load_config`
- `init_creates_ekos_directory`
- `clean_removes_artifacts_not_ledger`

## ekos/crates/cli/tests/transformation_benchmark.rs

- `setup_workspace`
- `load_config`
- `call_tool`

## ekos/crates/common/src/compress.rs

- `round_trip_compressed`
- `read_auto_prefers_zst_and_falls_back_to_plain`
- `compressed_is_smaller_than_plain_on_repetitive_json`

## ekos/crates/common/src/lib.rs

- `fmt`
- `same_content_same_hash`
- `different_content_different_hash`

## ekos/crates/compiler-core/src/cache.rs

- `manifest_path`
- `name`
- `version`
- `cache_inputs`
- `no_manifest_means_recompute`
- `unchanged_identity_skips_recompute`
- `changed_config_hash_forces_recompute`
- `changed_inputs_forces_recompute`
- `changed_version_forces_recompute`

## ekos/crates/compiler-core/src/compiler.rs

- `name`

## ekos/crates/compiler-core/src/config.rs

- `default_root`
- `default_log_level`
- `default_log_format`
- `default`
- `default_ignore_patterns`
- `default`
- `default_github`
- `default_hashtags`
- `default`
- `default_sql_dialect`
- `default`
- `default`
- `parse_minimal_config`
- `default_config_is_valid`
- `document_semantics_defaults_to_disabled`
- `document_semantics_parses_from_kebab_case_table`
- `marketing_defaults_to_disabled_with_sensible_defaults`
- `sql_recover_defaults_to_generic_with_no_rules`
- `sql_recover_parses_dialect_rules_from_kebab_case_table`
- `marketing_parses_from_kebab_case_table`

## ekos/crates/compiler-core/src/diagnostics.rs

- `sink_collects_and_filters`

## ekos/crates/compiler-core/src/pass.rs

- `name`
- `dependencies`
- `version`
- `cache_inputs`
- `check_unique_names`
- `default`
- `name`
- `dependencies`
- `topological_order_a_b_c`
- `cycle_detected`
- `unknown_dependency`
- `duplicate_pass_names_are_diagnosed_not_reported_as_cycle`
- `zero_passes_empty_order`
- `name`
- `name`
- `execution_levels_groups_independent_passes_together`
- `execution_levels_detects_cycle`
- `name`

## ekos/crates/dbt-gen/src/lib.rs

- `slugify_snake`
- `get_str`
- `get_str_vec`
- `get_pairs`
- `get_aggs`
- `comment_block`
- `no_upstream_placeholder`
- `render_source`
- `render_sink`
- `render_join`
- `render_aggregate`
- `render_filter`
- `render_calculate`
- `render_unmapped`
- `node`
- `dbt_model_name_slugifies_source_path_and_index`
- `is_transform_node_matches_only_the_custom_kind`
- `is_feeds_into_matches_only_the_custom_kind`
- `source_node_renders_select_from_source_macro`
- `source_node_with_no_columns_selects_star`
- `sink_node_selects_star_from_its_upstream_ref`
- `sink_node_with_no_upstream_renders_an_honest_placeholder_not_a_panic`
- `join_node_renders_real_keys_and_kind`
- `join_node_never_double_qualifies_already_aliased_sql_source_keys`
- `join_node_with_no_keys_renders_honest_true_condition`
- `aggregate_node_renders_real_group_by_and_agg_funcs`
- `filter_node_inlines_raw_condition_flagged_as_unverified`
- `calculate_node_inlines_raw_expr_flagged_as_unverified`
- `unmapped_node_preserves_raw_and_reason_as_comments_and_still_chains_ref`
- `unmapped_node_with_no_upstream_still_renders_valid_sql_not_a_panic`
- `upstream_model_names_resolves_via_feeds_into_not_join_fields`
- `upstream_model_names_skips_edges_to_unresolved_nodes`
- `feeds_into_chain_resolves_correctly_through_an_unmapped_node`
- `schema_yml_lists_deduplicated_sources_and_sorted_models`
- `schema_yml_on_empty_input_is_honest_not_invalid_yaml`

## ekos/crates/docs-gen/src/lib.rs

- `page_file_name`
- `mermaid_node_id`
- `mermaid_escape_label`
- `mermaid_arrow`
- `html_escape`
- `strip_mermaid_fence`
- `html_document`
- `format_value`
- `slugify`
- `is_feeds_into`
- `count_by_kind`
- `render_relationship_kind_graph`
- `transform_node_origin`
- `sequence_participant_line`
- `sample_table`
- `renders_name_kind_and_properties`
- `empty_object_renders_honest_placeholders_not_panics`
- `relationship_with_resolved_evidence_cites_the_fragment`
- `relationship_citing_unresolved_evidence_says_so_honestly`
- `relationships_group_by_kind_without_dropping_non_foreign_key`
- `slugify_handles_dots_and_mixed_case`
- `incoming_relationship_renders_reverse_arrow`
- `relationship_with_resolved_name_shows_name_not_just_id`
- `column_is_not_significant_but_every_other_kind_is`
- `non_table_kinds_render_pages_with_kind_prefixed_file_names`
- `different_kinds_sharing_a_name_do_not_collide_on_file_name`
- `index_page_groups_by_kind_and_links_every_page`
- `index_page_on_empty_set_is_honest_not_empty_file`
- `index_page_lists_diagrams_ahead_of_object_groups`
- `index_page_with_no_diagrams_omits_the_diagrams_section`
- `object_page_embeds_a_diagram_section_with_a_fenced_mermaid_block`
- `mermaid_graph_labels_edges_with_relationship_kind_and_direction`
- `mermaid_graph_dashes_coupled_with_edges_to_signal_a_derived_relationship`
- `mermaid_graph_unresolved_neighbor_falls_back_to_id_not_dropped`
- `mermaid_graph_escapes_quotes_in_labels`
- `er_diagram_renders_foreign_key_edges_between_given_tables`
- `er_diagram_excludes_foreign_keys_to_objects_outside_the_table_set`
- `er_diagram_ignores_non_foreign_key_relationships`
- `er_diagram_quotes_entity_names_containing_spaces`
- `model_and_markdown_page_agree_with_the_direct_render_object_page_wrapper`
- `html_page_has_correct_file_extension_and_is_a_full_document`
- `html_page_escapes_dangerous_characters_in_object_derived_text`
- `html_page_embeds_mermaid_source_without_markdown_fence`
- `html_page_on_empty_object_renders_honest_placeholders`
- `html_index_lists_diagrams_and_groups_pages_by_kind`
- `html_index_on_empty_set_is_honest_not_blank`
- `strip_mermaid_fence_removes_fence_but_keeps_body`
- `html_er_diagram_page_has_correct_file_name_and_embeds_source`
- `build_object_page_model_initializes_prose_to_none`
- `markdown_page_embeds_prose_and_its_citations_ahead_of_properties`

## ekos/crates/ekl/src/interpreter.rs

- `candidate_rows`
- `resolve_anchor`
- `expand_from_anchor`
- `object_row`
- `relationship_row`
- `project`
- `value_to_string`
- `value_as_f64`
- `literal_as_f64`
- `literal_to_string`
- `value_eq`
- `eval_predicate`
- `compare_rows`
- `fixture`
- `run`
- `example_1_all_tables`
- `example_2_return_name_only`
- `example_3_exact_name_match`
- `example_4_contains_predicate`
- `example_5_order_by_and_limit`
- `example_6_all_foreign_keys`
- `example_7_relationships_from_anchor`
- `example_8_object_neighbourhood`
- `example_9_no_matches`
- `example_10_return_projection_with_limit`
- `via_kind_filters_and_traces_dependencies`
- `depth_without_via_generalizes_single_hop_default`
- `unknown_anchor_returns_error`
- `finds_objects_of_a_newly_added_object_kind`
- `finds_relationships_of_a_custom_kind`

## ekos/crates/ekl/src/parser.rs

- `fmt`
- `new`
- `tokenize`
- `skip_whitespace`
- `match_symbol_op`
- `read_string`
- `read_number`
- `read_ident`
- `new`
- `peek`
- `peek_pos`
- `advance`
- `expect_keyword`
- `peek_keyword`
- `expect_ident`
- `expect_string`
- `expect_num`
- `parse_query`
- `parse_entity`
- `parse_predicate`
- `parse_op`
- `parse_literal`
- `describe`
- `parses_minimal_find_object`
- `parses_return_clause`
- `parses_relationship_with_from`
- `parses_order_by_and_limit`
- `parses_order_by_desc`
- `parses_and_chained_predicates`
- `parses_query_with_no_where`
- `parses_numeric_comparison_operators`
- `parses_via_and_depth`
- `depth_alone_generalizes_from_without_via`
- `queries_without_via_or_depth_default_to_none`
- `rejects_via_without_from`
- `rejects_unknown_entity`
- `rejects_missing_find_keyword`
- `rejects_unterminated_string`
- `rejects_trailing_garbage`
- `fuzz_random_strings_never_panic`

## ekos/crates/identity/src/cross_system.rs

- `matchable_name`
- `normalize_cross_system`
- `type_family`
- `column_types`
- `type_compat_score`
- `column_overlap_score`
- `combine_signals`
- `table`
- `transform_source`
- `transform_sink`
- `normalize_strips_schema_prefix_and_etl_affixes`
- `column_overlap_scores_shared_column_names`
- `type_compat_none_when_either_side_untyped`
- `type_compat_scores_matching_families`
- `three_system_customer_table_scenario_produces_candidates`
- `transform_node_source_and_table_can_match`
- `unrelated_tables_do_not_produce_a_candidate`
- `non_table_like_objects_are_ignored`
- `identical_names_are_not_proposed_as_candidates`
- `exact_name_match_across_kinds_is_proposed_at_max_confidence`
- `exact_name_match_same_kind_is_still_skipped`

## ekos/crates/identity/src/lib.rs

- `resolve`
- `default`
- `default`
- `threshold_for`
- `score`
- `resolve`
- `structural_score`
- `new`
- `find`
- `union`
- `make_graph`
- `empty_graph_returns_empty`
- `single_object_no_merge`
- `exact_case_difference_proposes_merge`
- `plural_singular_proposes_merge`
- `underscore_variant_proposes_merge`
- `dissimilar_names_no_merge`
- `table_with_columns`
- `prefix_sharing_tables_with_disjoint_columns_do_not_merge`
- `similar_names_with_overlapping_columns_still_merge`
- `different_kind_same_name_conflict`
- `newly_added_object_kind_participates_in_conflict_detection`
- `unrelated_documents_sharing_a_folder_prefix_do_not_all_merge`
- `distinct_pdf_tables_in_one_document_do_not_all_merge`
- `three_way_transitivity_single_proposal`
- `stats_counts_pairs_and_candidates`
- `custom_threshold_prevents_merge`
- `result_is_serializable`
- `section_objects_are_never_merged_even_with_near_identical_names`
- `transform_node_objects_are_never_merged_even_with_shared_source_prefix`
- `rust_symbol_objects_are_never_merged_even_with_shared_name_suffix`
- `concept_same_real_entity_across_two_documents_merges`
- `concept_generic_short_names_across_unrelated_documents_do_not_all_merge`
- `other_custom_kinds_still_resolve_normally`

## ekos/crates/identity/src/similarity.rs

- `jaro_winkler_identical`
- `jaro_winkler_empty_strings`
- `jaro_winkler_orders_vs_order`
- `jaro_winkler_dissimilar`
- `normalize_strips_underscores`
- `normalize_lowercases`
- `normalize_strips_tbl_prefix_not_suffix`
- `normalize_preserves_distinct_names`

## ekos/crates/kir/src/lib.rs

- `default`
- `fmt`
- `from_str`
- `fmt`
- `fmt`
- `from_str`
- `kir_object_round_trip`
- `indexed_content_includes_ocr_text`
- `indexed_content_concatenates_excerpt_symbols_and_ocr_text`
- `indexed_content_empty_when_no_relevant_properties`
- `object_kind_taxonomy_round_trips`
- `object_kind_custom_fallback_still_works`
- `kir_evidence_round_trip`
- `kir_graph_add_and_get`
- `sample_graph`
- `kir_graph_full_round_trip`
- `kir_relationship_serializes_from_to`
- `kir_event_round_trip`
- `knowledge_artifact_embeds_kir_graph`

## ekos/crates/ledger/src/fact.rs

- `escape_segment`
- `split_path`
- `canonical_uuid`
- `flatten`
- `type_name`
- `value_to_json`
- `insert_path`
- `round_trip`
- `assert_parity`
- `object_round_trips_with_signature_parity`
- `relationship_and_evidence_round_trip`
- `typed_reconstruction_is_lossless`
- `numeric_fidelity_edge_cases`
- `dotted_keys_stay_distinct_from_nesting`
- `empty_containers_and_arrays_round_trip`
- `evidence_order_is_preserved`
- `non_canonical_ref_values_fall_back_verbatim`
- `diff_of_property_change_is_two_facts`
- `registry_ids_are_stable_and_reindexable`
- `reconstruction_is_order_independent`

## ekos/crates/ledger/src/fact_ledger.rs

- `from`
- `kind_of_payload`
- `append_payload`
- `append_inner`
- `typed_current`
- `all_of_kind`
- `self_counts`
- `all_current_payloads`
- `entity_entries`
- `state_at`
- `reconstruct_at`
- `current_sig`
- `entities_with_attr`
- `relationship_candidates`
- `flush_memtable`
- `runs_dir`
- `index_object`
- `tx_at`
- `fold_state`
- `copy_dir`
- `temp_ledger`
- `append_and_retrieve_object`
- `all_objects_and_relationships_are_listed`
- `append_is_idempotent`
- `get_unknown_object_returns_none`
- `evidence_round_trips_and_is_not_an_object`
- `event_round_trips_and_is_not_an_object`
- `updating_creates_new_version_and_keeps_latest_current`
- `object_at_returns_true_historical_version_after_update`
- `relationships_for_returns_both_directions`
- `relationships_at_filters_by_time`
- `fts_semantics_prefix_content_and_ranking`
- `fts_finds_objects_by_harvested_symbol`
- `fts_follows_object_updates`
- `diff_reports_updated_object_as_added_and_others_unchanged`
- `branch_copy_is_readable_and_merges_like_sqlite`
- `state_survives_reopen`
- `reads_serve_from_runs_after_seal_and_reopen`
- `search_index_rebuilds_after_deletion`
- `cross_backend_parity_with_sqlite_ledger`

## ekos/crates/ledger/src/index.rs

- `prefix`
- `push_escaped`
- `value_order_key`
- `push_pos`
- `order`
- `bytes`
- `in_prefix`
- `stores_values`
- `project`
- `write_run_raw`
- `encode_block`
- `decode_block`
- `corrupt`
- `read_block_raw`
- `scan`
- `all_raw`
- `store_with_objects`
- `eavt_scan_returns_one_entitys_facts`
- `avet_scan_finds_entity_by_ref_value`
- `aevt_scan_lists_every_entity_with_attribute`
- `scans_merge_across_runs_and_survive_compaction`
- `indexes_rebuild_from_segments`
- `value_keys_with_embedded_zeros_and_prefixes_stay_ordered`
- `block_pruning_still_finds_entries_across_blocks`

## ekos/crates/ledger/src/lib.rs

- `zstd`
- `compress`
- `decompress`
- `create_v2`
- `migrate_fts_v2`
- `id_param`
- `sig_param`
- `ts_param`
- `payload_to_string`
- `payload_param`
- `query_payloads`
- `index_object_fts_v1`
- `index_object_fts_v2`
- `append_versioned`
- `find_objects_v1`
- `find_objects_v2`
- `versions_in_window`
- `init_schema_v2`
- `load_dictionary`
- `id_value_to_string`
- `sig_value_to_hex`
- `ts_value_to_datetime`
- `all_objects_with_rowids`
- `payload_samples`
- `sibling_path`
- `append_object`
- `append_evidence`
- `append_relationship`
- `append_event`
- `get_object`
- `get_evidence`
- `get_relationship`
- `get_event`
- `all_objects`
- `all_relationships`
- `relationships_for`
- `object_at`
- `relationships_at`
- `find_objects`
- `entry_count`
- `object_count`
- `relationship_count`
- `vacuum_into`
- `diff`
- `append_object`
- `append_evidence`
- `append_relationship`
- `append_event`
- `get_object`
- `get_evidence`

## ekos/crates/ledger/src/search.rs

- `terr`

## ekos/crates/ledger/src/segment/map.rs

- `maps_sealed_file_and_verifies_length`

## ekos/crates/ledger/src/segment/mod.rs

- `build_dict`
- `read_active_committed`
- `active_batches`
- `encode_frame`
- `walk_frames`
- `scan_slice`
- `scan_batches_filtered`
- `scan_headers_slice`
- `decode_header`
- `decode_frame`
- `segment_path`
- `load_manifest`
- `save_manifest`
- `write_head`
- `atomic_write`
- `hash_file`
- `sample_ops`
- `append_and_replay_across_reopen`
- `seal_rolls_to_new_segment_and_verifies`
- `torn_tail_is_truncated_and_prior_batches_survive`
- `valid_frames_past_stale_watermark_are_recovered`
- `corrupted_sealed_segment_fails_verification`
- `manifest_attribute_registry_round_trips`
- `batch_round_trip_preserves_ops_exactly`

## ekos/crates/ledger/tests/estate_migration.rs

- `dir_bytes`
- `mb`
- `migrate_estate_and_report_sizes`

## ekos/crates/marketing/src/devlog.rs

- `split_once_any_dash`
- `extract_section`
- `parses_number_title_date_from_real_format`
- `extracts_summary_body_only`
- `section_titles_excludes_meta_sections_only`
- `missing_summary_is_an_error`
- `missing_title_heading_is_an_error`
- `number_from_filename_handles_path_and_bare_name`
- `find_latest_picks_highest_numbered_devlog`
- `find_latest_returns_none_when_no_devlogs_exist`

## ekos/crates/marketing/src/importance.rs

- `devlog`
- `pure_chore_devlog_is_low`
- `devlog_mentioning_an_rfc_is_high`
- `ordinary_feature_improvement_is_medium`
- `low_signal_words_with_a_feature_signal_stay_medium`

## ekos/crates/marketing/src/oauth1.rs

- `normalized_param_string`
- `sign`
- `percent_encode_leaves_unreserved_characters_alone`
- `percent_encode_escapes_reserved_characters`
- `hmac_sha1_matches_rfc2202_test_vector_1`
- `creds`
- `base_params`
- `signature_is_deterministic_for_identical_inputs`
- `signature_changes_when_any_single_input_changes`
- `authorization_header_has_oauth_scheme_and_all_required_params`
- `nonce_is_reasonably_unique_across_calls`

## ekos/crates/marketing/src/prompt.rs

- `overage_from_too_long_reason`
- `config`
- `devlog`
- `system_prompt_encodes_every_hard_rule`
- `user_prompt_embeds_summary_github_and_hashtags`
- `retry_suffix_carries_the_rejection_reason`
- `retry_suffix_states_exact_overage_for_too_long_drafts`
- `retry_suffix_falls_back_without_overage_for_other_rejections`

## ekos/crates/marketing/src/publisher.rs

- `from_env_reports_which_var_is_missing`

## ekos/crates/marketing/src/store.rs

- `load_missing_file_is_an_empty_store`
- `record_then_save_then_reload_roundtrips`
- `is_posted_is_false_for_a_different_devlog_number`
- `tweets_json_file_lands_at_marketing_posted_tweets_json`

## ekos/crates/marketing/src/tweet.rs

- `config`
- `devlog`
- `valid_tweet_passes`
- `rejects_tweet_over_280_chars`
- `rejects_tweet_missing_ekos_mention`
- `rejects_tweet_missing_github_link`
- `rejects_tweet_with_too_many_hashtags`
- `rejects_empty_tweet`

## ekos/crates/observation-sdk/src/lib.rs

- `name`
- `is_ignored_catches_prefix_segments`
- `observation_package_counts`
- `fingerprint_stable_across_repeated_scans`
- `fingerprint_changes_on_new_file`
- `fingerprint_ignores_ignored_paths`

## ekos/crates/recovery/src/anthropic.rs

- `model_name`

## ekos/crates/recovery/src/cache.rs

- `cache_key`
- `cache_path`
- `model_name`
- `model_name`

## ekos/crates/recovery/src/confluence_analyzer.rs

- `page_kir_id`
- `body_excerpt`
- `find_linked_titles`
- `name`
- `cache_inputs`
- `ctx`
- `seed_page`
- `finds_content_title_links`
- `ignores_body_with_no_links`

## ekos/crates/recovery/src/crypto_analyzer.rs

- `name`
- `cache_inputs`
- `deterministic_id`
- `parse_attrs`
- `make_ctx`
- `seed_batch_artifact`
- `sample_batch_json`

## ekos/crates/recovery/src/dependency_analyzer.rs

- `technology_kir_id`
- `file_kir_id`
- `name`
- `cache_inputs`
- `ctx`

## ekos/crates/recovery/src/document_semantics_analyzer.rs

- `concept_kir_id`
- `normalize_concept_name`
- `collect_sections`
- `sections_from_graph`
- `name`
- `dependencies`
- `ctx`
- `read_output`
- `two_concept_response`
- `declares_its_local_docs_pass_as_a_dependency`

## ekos/crates/recovery/src/git_analyzer.rs

- `name`
- `version`
- `cache_inputs`
- `contributor_kir_id`
- `make_commit_artifact`
- `make_ctx_with_store`
- `seed_artifact`
- `read_knowledge_graph`
- `make_repo_artifact`
- `contributor_kir_id_is_stable`

## ekos/crates/recovery/src/github_analyzer.rs

- `item_kir_id`
- `file_kir_id`
- `find_closed_issue_numbers`
- `body_excerpt`
- `name`
- `cache_inputs`
- `ctx`
- `seed_item`
- `finds_closes_keyword_case_insensitively`
- `ignores_unrecognized_phrasing`

## ekos/crates/recovery/src/llm.rs

- `model_name`
- `model_name`

## ekos/crates/recovery/src/llm_json.rs

- `bare_json_is_unchanged`
- `json_fence_is_stripped`
- `bare_fence_is_stripped`
- `surrounding_whitespace_is_trimmed`

## ekos/crates/recovery/src/local_docs_analyzer.rs

- `document_kir_id`
- `table_kir_id`
- `section_kir_id`
- `name`
- `cache_inputs`
- `ctx`
- `seed_doc`
- `seed_doc_with_sections`

## ekos/crates/recovery/src/ollama.rs

- `build_request`
- `model_name`
- `model_name_reflects_construction`
- `from_env_falls_back_to_defaults_when_unset`
- `request_always_sets_temperature_zero`

## ekos/crates/recovery/src/pentaho_analyzer.rs

- `name`
- `cache_inputs`
- `parse_kettle_xml`
- `parse_kjb`
- `parse_ktr`
- `map_step`
- `extract_filter_condition`
- `extract_calculator`
- `extract_join`
- `extract_join_keys`
- `extract_stream_lookup`
- `extract_group_by`
- `child_text`
- `xml_slice`
- `extract_table_from_sql`
- `sample_graph`
- `maps_table_input_to_source`
- `maps_table_input_columns_from_row_meta`
- `maps_filter_rows_to_filter_with_readable_condition`
- `maps_calculator_to_calculate`
- `maps_group_by_to_aggregate`
- `maps_table_output_to_sink`
- `hops_become_graph_edges_in_step_order`
- `unrecognized_step_type_becomes_unmapped_with_raw_xml_preserved`
- `maps_stream_lookup_to_left_join`
- `maps_merge_join_keys_from_keys_1_keys_2`
- `kjb_job_entries_become_unmapped`
- `coverage_percent_counts_non_unmapped_nodes`

## ekos/crates/recovery/src/python_analyzer.rs

- `name`
- `cache_inputs`
- `parse_python_file`
- `python_module_kir_id`
- `add_import`
- `add_symbol`
- `walk_top_level_statement`
- `try_recognize_chain_statement`
- `linearize_chain`
- `string_constant`
- `positional_string_arg`
- `keyword_arg`
- `source_slice`
- `join_keys_from_on`
- `join_kind_from_how`
- `agg_expr_from_arg`
- `calls_to_nodes`
- `parse`
- `recognizes_imports_as_depends_on`
- `recognizes_function_and_class_defs_as_symbols`
- `table_read_becomes_source_node`
- `read_format_load_becomes_source_node`
- `real_join_and_select_chain_becomes_join_node`
- `join_with_string_on_key_and_left_how`
- `with_column_becomes_calculate_node`
- `multi_step_chain_produces_linked_nodes`
- `group_by_agg_becomes_aggregate_node`
- `write_save_as_table_becomes_sink_node`
- `spark_sql_call_is_honestly_unmapped_never_parsed_as_sql`
- `plain_statement_with_no_recognized_chain_produces_no_graph`
- `databricks_notebook_comment_markers_do_not_break_parsing`

## ekos/crates/recovery/src/rust_analyzer.rs

- `name`
- `cache_inputs`
- `parse_rust_file`
- `type_name`
- `flatten_use_tree`
- `rust_module_kir_id`
- `add_import`
- `add_symbol`
- `visit_expr_call`
- `visit_expr_method_call`
- `parse`
- `recognizes_use_imports_as_depends_on`
- `recognizes_fn_struct_enum_trait_as_symbols`
- `same_file_function_call_becomes_calls_edge`
- `call_to_external_or_std_function_is_not_recorded`
- `same_file_method_call_becomes_calls_edge`
- `self_colon_colon_associated_call_resolves_via_impl_type`
- `ambiguous_method_name_across_two_types_is_not_recorded`
- `call_inside_macro_invocation_is_not_recorded`

## ekos/crates/recovery/src/sql_analyzer.rs

- `name`
- `cache_inputs`
- `add_fk_relationship`
- `col_names`
- `columns_json`
- `apply_llm_enrichment`
- `make_ctx`
- `structural_parse_extracts_six_tables`
- `structural_parse_extracts_fk_relationships`
- `northwind_structural_parse_extracts_thirteen_tables`
- `northwind_structural_parse_extracts_deep_fk_graph`
- `northwind_structural_parse_finds_order_details_composite_pk_table`
- `structural_parse_table_has_columns`
- `generic_dialect_fails_on_real_mysql_hash_comments_fixture`
- `mysql_dialect_parses_real_mysql_hash_comments_fixture`
- `recovers_tables_from_ddl_script_missing_semicolons_between_statements`
- `llm_enrichment_applies_entity_names`

## ekos/crates/recovery/src/sql_dialect_registry.rs

- `name`
- `sqlparser_dialect`
- `registry_contains_generic_mysql_and_postgres`
- `registry_contains_snowflake_and_databricks`
- `registry_contains_mssql_aliases`
- `resolve_dialect_name_falls_back_to_default_with_no_rules`
- `resolve_dialect_name_matches_first_rule_by_path_glob`
- `resolve_dialect_name_handles_mixed_dialect_workspace`

## ekos/crates/recovery/src/sql_transform_analyzer.rs

- `name`
- `cache_inputs`
- `source_kind_for`
- `dispatch_one_statement`
- `parse_sql_statement_by_statement`
- `push`
- `query_to_graph`
- `select_to_graph`
- `table_factor_node`
- `join_node`
- `extract_equi_keys`
- `collect_equi_keys`
- `is_plain_column`
- `as_aggregate_function`
- `extract_aggregates`
- `calculated_projection`
- `procedure_body_to_graph`
- `function_to_graph`
- `function_body_text`
- `append_fragment`
- `graphs`
- `recovers_statements_from_script_missing_semicolons_between_them`
- `simple_select_with_where`
- `select_with_join`
- `select_with_group_by`
- `view_wrapping_multi_table_query_gets_a_sink`
- `stored_procedure_with_embedded_select_and_control_flow`
- `independent_statement_survives_when_another_procedure_in_the_same_file_has_unparseable_control_flow`
- `function_with_dollar_quoted_body_extracts_embedded_select`
- `calculated_projection_becomes_calculate_node`
- `left_join_maps_to_left_join_kind`
- `cte_query_becomes_unmapped_not_dropped`
- `informix_falls_back_to_generic_dialect_and_still_parses_simple_select`
- `databricks_dialect_parses_simple_select`
- `mysql_dialect_parses_simple_select`
- `mysql_dialect_parses_hash_comment_that_generic_dialect_rejects`
- `ddl_statements_produce_no_transform_graphs`
- `coverage_percent_reflects_unmapped_ratio`

## ekos/crates/recovery/src/statement_repair.rs

- `ends_with_set_op_keyword`
- `starts_with_keyword`
- `inserts_semicolons_between_statements_missing_them`
- `leaves_already_well_formed_multi_statement_sql_unchanged_in_effect`
- `does_not_split_a_union_chain_across_lines`
- `does_not_split_inside_open_parens`

## ekos/crates/runtime/src/ai.rs

- `default`
- `gather_context`
- `extract_citations`
- `temp_ledger`
- `seed`

## ekos/crates/runtime/src/lib.rs

- `temp_ledger`
- `obj`
- `fk`
- `same_as_unconfirmed`
- `same_as_confirmed`
- `load_object_unknown_returns_none`
- `relationships_for_returns_both_directions`
- `load_object_known_returns_object`
- `load_neighborhood_depth_0_is_root_only`
- `load_neighborhood_excludes_unconfirmed_same_as_but_keeps_confirmed`
- `load_neighborhood_depth_1_returns_direct_neighbours`
- `load_neighborhood_depth_2_returns_two_hops`
- `load_neighborhood_handles_cycles`
- `trace_impact_dependents_and_dependencies_are_disjoint`
- `trace_impact_excludes_unconfirmed_same_as_but_keeps_confirmed`
- `trace_impact_filters_by_kind`
- `trace_impact_handles_cycles`
- `trace_impact_max_hops_bounds_traversal`
- `reconstruct_state_returns_object_rels_evidence`
- `reconstruct_state_at_before_write_returns_none`
- `find_objects_matches_by_name_prefix`
- `find_objects_no_match_returns_empty`
- `list_objects_returns_every_object`
- `list_relationships_returns_every_relationship`
- `reconstruct_state_at_after_write_returns_state`

## ekos/crates/semantic/src/lib.rs

- `name`
- `cache_inputs`
- `cache_inputs_are_declared_and_sorted`
- `two_object_graph`
- `build_ckm_produces_correct_counts`
- `build_ckm_embeds_evidence_in_objects`
- `validate_passes_on_valid_ckm`
- `validate_catches_dangling_relationship`
- `dedup_relationships_merges_duplicate`
- `apply_merges_remaps_relationship_ids`
- `apply_merges_deduplicates_relationships`
- `ckm_is_serializable`

## ekos/crates/semantic/src/transform_ir.rs

- `content_id`
- `source_node_serializes_deterministically`
- `filter_node_serializes_deterministically`
- `join_node_serializes_deterministically`
- `aggregate_node_serializes_deterministically`
- `calculate_node_serializes_deterministically`
- `sink_node_serializes_deterministically`
- `unmapped_node_serializes_deterministically`
- `origin`
- `sample_graph`
- `transform_node_kir_id_is_stable_across_repeated_lowering`
- `transform_node_kir_id_differs_by_index_and_source_path`
- `lower_to_kir_produces_one_object_per_node_with_evidence`
- `lower_to_kir_sets_node_type_property`
- `lower_to_kir_indexes_filter_condition_as_excerpt`
- `lower_to_kir_produces_feeds_into_edges_matching_graph_edges`
- `lower_to_kir_is_idempotent_across_repeated_runs`
- `transform_nodes_round_trip_through_ledger_versioning`
- `lower_to_kir_unmapped_node_preserves_raw_and_reason`
- `node_type`
- `evidence_fragment`
- `properties`
- `transform_evidence_kir_id`

## ekos/crates/sql-dialect-sdk/src/lib.rs

- `name`
- `sqlparser_dialect`
- `preprocess`

## ekos/docs/rfcs/0025-additional-document-formats.md

- `supported_extension`
- `supported_extensions`

## ekos/docs/rfcs/0026-document-semantics-extraction.md

- `dependencies`
- `concept_kir_id`

## ekos/docs/rfcs/0027-unified-transformation-semantics.md

- `transform_node_kir_id`

## ekos/docs/rfcs/0031-pluggable-sql-dialects.md

- `name`
- `sqlparser_dialect`
- `preprocess`
- `build_dialect_registry`

## ekos/plugins/confluence/src/lib.rs

- `request`
- `name`
- `page`

## ekos/plugins/crypto/src/lib.rs

- `latest_batch_dir`
- `read_entities`
- `read_relationships`
- `read_evidence`
- `read_rows`
- `get_string`
- `get_string_list`
- `name`
- `sample_batch`

## ekos/plugins/fabric/src/lib.rs

- `name`
- `lakehouse`
- `sample_workspace_items`

## ekos/plugins/file/src/lib.rs

- `default`
- `name`
- `text_excerpt`
- `harvest_symbols`
- `harvest_symbols_finds_known_declaration_kinds`
- `harvest_symbols_ignores_lines_without_a_declaration`
- `harvest_symbols_is_capped`

## ekos/plugins/git/src/lib.rs

- `default`
- `name`
- `parse_stat_summary`
- `make_git_repo`
- `parse_stat_extracts_numbers`
- `parse_stat_insertions_only`

## ekos/plugins/github/src/lib.rs

- `request`
- `name`
- `issue`
- `pr`

## ekos/plugins/localdocs/src/docx.rs

- `supported_extension`
- `parse`
- `paragraph_text`
- `table_rows`
- `extract_media_images`
- `parses_paragraph_text_and_table_rows`

## ekos/plugins/localdocs/src/email.rs

- `supported_extension`
- `parse`
- `header_block`
- `render_address`
- `body_text`
- `supported_extension_is_eml`
- `first_section_is_the_header_block`
- `body_sections_follow_the_header_with_sequential_indexes`
- `text_plain_part_is_preferred_over_the_html_alternative`
- `html_body_is_used_when_no_text_plain_part_exists`
- `no_tables_images_or_page_count`
- `malformed_input_returns_parse_error_rather_than_panicking`

## ekos/plugins/localdocs/src/html.rs

- `supported_extension`
- `parse`
- `extension_is_whatever_the_parser_was_constructed_with`
- `nested_tags_are_flattened_into_prose_without_markup`
- `table_content_survives_as_prose_but_no_structured_table_is_extracted`
- `sections_have_no_page_and_respect_the_budget`
- `truncated_markup_degrades_gracefully_rather_than_panicking`

## ekos/plugins/localdocs/src/lib.rs

- `supported_extension`
- `supported_extensions`
- `parse`
- `recognize`
- `parser_for`
- `name`
- `supported_extension`
- `parse`
- `supported_extension`
- `parse`
- `recognize`
- `recognize`
- `default_parsers`
- `silent_ocr`
- `with_defaults_registers_a_parser_for_every_rfc_0025_extension`
- `extension_lookup_is_case_insensitive`

## ekos/plugins/localdocs/src/ocr.rs

- `recognize`
- `recognize`
- `mock_ocr_returns_fixed_text`
- `missing_tesseract_binary_is_soft_skippable`

## ekos/plugins/localdocs/src/pdf.rs

- `supported_extension`
- `parse`
- `parse_inner`
- `extract_sections`
- `extract_tables`
- `has_uniform_column_count`
- `split_table_row`
- `real_justified_prose_produces_no_table`
- `real_toc_fragment_is_still_detected_as_a_table`
- `has_uniform_column_count_rejects_mismatched_rows`
- `split_table_row_requires_two_space_gap`
- `extract_tables_groups_contiguous_rows`
- `extract_tables_ignores_single_row_matches`
- `extract_sections_returns_empty_on_garbage_bytes_without_panicking`
- `parse_real_multipage_pdf_produces_one_section_per_page`
- `build_two_page_pdf`

## ekos/plugins/localdocs/src/sanitize.rs

- `is_sanitized_char`
- `strips_zero_width_characters`
- `strips_unicode_tag_block`
- `leaves_ordinary_text_untouched`
- `empty_input_is_a_no_op`

## ekos/plugins/localdocs/src/text.rs

- `supported_extension`
- `parse`
- `split_to_budget`
- `extension_is_whatever_the_parser_was_constructed_with`
- `supported_extensions_defaults_to_the_single_extension`
- `plain_text_round_trips_into_text_and_one_section`
- `invalid_utf8_degrades_to_replacement_chars_rather_than_failing`
- `markdown_headings_are_kept_as_literal_text`
- `chunking_respects_the_budget_and_indexes_sequentially`
- `a_single_line_longer_than_the_budget_is_split_not_overflowed`
- `multibyte_text_splits_on_char_boundaries`
- `sections_are_capped_at_sections_max`
- `whitespace_only_input_produces_no_sections`

## ekos/plugins/oracle/src/lib.rs

- `name`
- `orders_table`
- `fk_constraint`

## ekos/plugins/pentaho/src/lib.rs

- `kettle_kind`
- `name`

## ekos/plugins/python/src/lib.rs

- `name`

## ekos/plugins/rust/src/lib.rs

- `name`

## ekos/plugins/salesforce/src/lib.rs

- `name`
- `account`
- `contact`

## ekos/plugins/sap/src/lib.rs

- `name`
- `sample_bo`
- `gwsample_basic_business_objects`
- `sample_org_unit`
- `sample_org_hierarchy`

## ekos/plugins/snowflake/src/lib.rs

- `name`
- `orders_table`
- `sample_account_objects`

## ekos/plugins/sql-dialect-databricks/src/lib.rs

- `name`
- `sqlparser_dialect`
- `name_is_databricks`
- `preprocess_is_identity`
- `databricks_dialect_parses_backtick_delimited_identifiers`

## ekos/plugins/sql-dialect-mssql/src/lib.rs

- `name`
- `sqlparser_dialect`
- `name_reports_the_configured_alias`
- `preprocess_is_identity`
- `mssql_dialect_parses_create_procedure_with_begin_end_body`

## ekos/plugins/sql-dialect-mysql/src/lib.rs

- `name`
- `sqlparser_dialect`
- `preprocess`
- `strip_delimiter_directives`
- `name_is_mysql`
- `preprocess_is_identity_when_no_delimiter_directive_present`
- `preprocess_strips_delimiter_directives_and_restores_semicolons`
- `mysql_dialect_parses_hash_comment_that_generic_dialect_rejects`

## ekos/plugins/sql-dialect-postgres/src/lib.rs

- `name`
- `sqlparser_dialect`
- `name_is_postgres`
- `preprocess_is_identity`
- `postgres_dialect_parses_dollar_quoted_function_body`

## ekos/plugins/sql-dialect-snowflake/src/lib.rs

- `name`
- `sqlparser_dialect`
- `name_is_snowflake`
- `preprocess_is_identity`
- `snowflake_dialect_parses_trailing_comma_in_projection`

## ekos_todo.md

- `scan`

## tests/fixtures/sample_project/src/main.rs

- `main`

## tests/integration/tests/integration.rs

- `fixtures_dir`
- `table_count`
- `copy_dir`

