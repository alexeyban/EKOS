# API

_Program entities (functions, structs, enums, traits, classes, …) compiled from real Rust/Python source analysis, grouped by containing file. Each entity links to its own detail page (relationships, evidence, 1-hop diagram), written alongside this file. Real `Api`/`Service` objects, if a future connector ever compiles them, would render here directly._

## benchmark/benches/fact_ledger.rs

- `function` [`bench_fact_ledger`](rustsymbol-bench-fact-ledger.md)
- `function` [`object`](rustsymbol-object.md)

## benchmark/benches/fact_model.rs

- `function` [`bench_fact_model`](rustsymbol-bench-fact-model.md)
- `function` [`realistic_object`](rustsymbol-realistic-object-0dcf9d8f.md)

## benchmark/benches/identity_resolver.rs

- `function` [`bench_identity_resolver`](rustsymbol-bench-identity-resolver.md)
- `function` [`fixture_graph`](rustsymbol-fixture-graph-e3802af0.md)

## benchmark/benches/index_runs.rs

- `function` [`bench_index_runs`](rustsymbol-bench-index-runs.md)
- `function` [`build_indexes`](rustsymbol-build-indexes.md)

## benchmark/benches/ledger_write.rs

- `function` [`bench_ledger_write`](rustsymbol-bench-ledger-write.md)

## benchmark/benches/observation_git.rs

- `function` [`bench_observation_git`](rustsymbol-bench-observation-git.md)
- `function` [`fixture_repo`](rustsymbol-fixture-repo.md)

## benchmark/benches/runtime_load_neighborhood.rs

- `function` [`bench_load_neighborhood`](rustsymbol-bench-load-neighborhood.md)
- `function` [`seed_ledger`](rustsymbol-seed-ledger.md)

## benchmark/benches/segment_store.rs

- `function` [`bench_segment_store`](rustsymbol-bench-segment-store.md)
- `function` [`ops`](rustsymbol-ops.md)

## benchmark/benches/semantic_compiler.rs

- `function` [`bench_semantic_compiler`](rustsymbol-bench-semantic-compiler.md)
- `function` [`fixture_graph`](rustsymbol-fixture-graph.md)

## benchmark/benches/sql_analyzer.rs

- `function` [`bench_sql_analyzer`](rustsymbol-bench-sql-analyzer.md)

## benchmark/benches/storage_compaction.rs

- `function` [`bench_storage`](rustsymbol-bench-storage.md)
- `function` [`ledger_file_bytes`](rustsymbol-ledger-file-bytes.md)
- `function` [`populated_ledger`](rustsymbol-populated-ledger.md)
- `function` [`realistic_object`](rustsymbol-realistic-object.md)

## docs/spikes/recovery_spike.py

- `function` [`call_claude`](pythonsymbol-call-claude.md)
- `function` [`evaluate`](pythonsymbol-evaluate.md)
- `function` [`main`](pythonsymbol-main.md)

## ekos/crates/artifact/src/lib.rs

- `struct` [`ArtifactId`](rustsymbol-artifactid.md)
- `method` [`ArtifactId::as_str`](rustsymbol-artifactid-as-str.md)
- `method` [`ArtifactId::compute`](rustsymbol-artifactid-compute.md)
- `method` [`ArtifactId::fmt`](rustsymbol-artifactid-fmt.md)
- `method` [`ArtifactId::prefix`](rustsymbol-artifactid-prefix.md)
- `struct` [`ArtifactMeta`](rustsymbol-artifactmeta.md)
- `method` [`ArtifactMeta::default`](rustsymbol-artifactmeta-default.md)
- `method` [`ArtifactMeta::new`](rustsymbol-artifactmeta-new.md)
- `enum` [`ArtifactType`](rustsymbol-artifacttype.md)
- `struct` [`DiagnosticArtifact`](rustsymbol-diagnosticartifact.md)
- `method` [`DiagnosticArtifact::new`](rustsymbol-diagnosticartifact-new.md)
- `struct` [`DiagnosticContent`](rustsymbol-diagnosticcontent.md)
- `struct` [`DiagnosticRecord`](rustsymbol-diagnosticrecord.md)
- `struct` [`EvidenceArtifact`](rustsymbol-evidenceartifact.md)
- `method` [`EvidenceArtifact::new`](rustsymbol-evidenceartifact-new.md)
- `struct` [`EvidenceContent`](rustsymbol-evidencecontent.md)
- `struct` [`IndexArtifact`](rustsymbol-indexartifact.md)
- `method` [`IndexArtifact::new`](rustsymbol-indexartifact-new.md)
- `struct` [`IndexContent`](rustsymbol-indexcontent.md)
- `struct` [`KnowledgeArtifact`](rustsymbol-knowledgeartifact.md)
- `method` [`KnowledgeArtifact::new`](rustsymbol-knowledgeartifact-new.md)
- `struct` [`KnowledgeContent`](rustsymbol-knowledgecontent.md)
- `struct` [`ObservationArtifact`](rustsymbol-observationartifact.md)
- `method` [`ObservationArtifact::new`](rustsymbol-observationartifact-new.md)
- `method` [`ObservationArtifact::with_producer`](rustsymbol-observationartifact-with-producer.md)
- `struct` [`ObservationContent`](rustsymbol-observationcontent.md)
- `function` [`canonicalize`](rustsymbol-canonicalize.md)
- `function` [`compute_content_id`](rustsymbol-compute-content-id.md)

## ekos/crates/artifact/src/pack.rs

- `struct` [`FrameLoc`](rustsymbol-frameloc.md)
- `struct` [`PackArtifactStore`](rustsymbol-packartifactstore.md)
- `method` [`PackArtifactStore::drop`](rustsymbol-packartifactstore-drop.md)
- `method` [`PackArtifactStore::exists`](rustsymbol-packartifactstore-exists.md)
- `method` [`PackArtifactStore::list`](rustsymbol-packartifactstore-list.md)
- `method` [`PackArtifactStore::loose_path`](rustsymbol-packartifactstore-loose-path.md)
- `method` [`PackArtifactStore::open`](rustsymbol-packartifactstore-open.md)
- `method` [`PackArtifactStore::packed_count`](rustsymbol-packartifactstore-packed-count.md)
- `method` [`PackArtifactStore::read`](rustsymbol-packartifactstore-read.md)
- `method` [`PackArtifactStore::repack_loose`](rustsymbol-packartifactstore-repack-loose.md)
- `method` [`PackArtifactStore::segment_path`](rustsymbol-packartifactstore-segment-path.md)
- `method` [`PackArtifactStore::sync`](rustsymbol-packartifactstore-sync.md)
- `method` [`PackArtifactStore::write`](rustsymbol-packartifactstore-write.md)
- `method` [`PackArtifactStore::write_packed`](rustsymbol-packartifactstore-write-packed.md)
- `struct` [`PackInner`](rustsymbol-packinner.md)
- `function` [`compress_frame_body`](rustsymbol-compress-frame-body.md)
- `function` [`hex_id_to_raw`](rustsymbol-hex-id-to-raw.md)
- `function` [`prune_empty_dirs`](rustsymbol-prune-empty-dirs.md)
- `function` [`scan_segment`](rustsymbol-scan-segment.md)
- `function` [`segment_paths`](rustsymbol-segment-paths.md)

## ekos/crates/artifact/src/store.rs

- `trait` [`ArtifactStore`](rustsymbol-artifactstore.md)
- `struct` [`FileSystemArtifactStore`](rustsymbol-filesystemartifactstore.md)
- `method` [`FileSystemArtifactStore::artifact_path`](rustsymbol-filesystemartifactstore-artifact-path.md)
- `method` [`FileSystemArtifactStore::exists`](rustsymbol-filesystemartifactstore-exists.md)
- `method` [`FileSystemArtifactStore::list`](rustsymbol-filesystemartifactstore-list.md)
- `method` [`FileSystemArtifactStore::new`](rustsymbol-filesystemartifactstore-new.md)
- `method` [`FileSystemArtifactStore::read`](rustsymbol-filesystemartifactstore-read.md)
- `method` [`FileSystemArtifactStore::root`](rustsymbol-filesystemartifactstore-root.md)
- `method` [`FileSystemArtifactStore::write`](rustsymbol-filesystemartifactstore-write.md)
- `enum` [`StoreError`](rustsymbol-storeerror.md)

## ekos/crates/cli/src/bin/ekos.rs

- `enum` [`ArtifactCommands`](rustsymbol-artifactcommands.md)
- `enum` [`BranchCommands`](rustsymbol-branchcommands.md)
- `struct` [`Cli`](rustsymbol-cli.md)
- `enum` [`Commands`](rustsymbol-commands.md)
- `enum` [`DbtCommands`](rustsymbol-dbtcommands.md)
- `enum` [`DocsCommands`](rustsymbol-docscommands.md)
- `enum` [`IdentityCommands`](rustsymbol-identitycommands.md)
- `enum` [`LedgerCommands`](rustsymbol-ledgercommands.md)
- `enum` [`MarketingCommands`](rustsymbol-marketingcommands.md)
- `enum` [`McpCommands`](rustsymbol-mcpcommands.md)
- `enum` [`QueryCommands`](rustsymbol-querycommands.md)
- `function` [`main`](rustsymbol-main-caa3d7b4.md)

## ekos/crates/cli/src/commands/artifact.rs

- `function` [`repack`](rustsymbol-repack.md)

## ekos/crates/cli/src/commands/ask.rs

- `function` [`ai_config`](rustsymbol-ai-config.md)
- `function` [`run`](rustsymbol-run-9c8ba43a.md)

## ekos/crates/cli/src/commands/branch.rs

- `function` [`branch_path`](rustsymbol-branch-path.md)
- `function` [`create`](rustsymbol-create.md)
- `function` [`delete`](rustsymbol-delete.md)
- `function` [`list`](rustsymbol-list.md)
- `function` [`merge`](rustsymbol-merge.md)
- `function` [`open_branch`](rustsymbol-open-branch.md)

## ekos/crates/cli/src/commands/build.rs

- `function` [`load_fingerprints`](rustsymbol-load-fingerprints.md)
- `function` [`prune_snapshots`](rustsymbol-prune-snapshots.md)
- `function` [`run`](rustsymbol-run-d09318f4.md)
- `function` [`save_fingerprints`](rustsymbol-save-fingerprints.md)

## ekos/crates/cli/src/commands/clean.rs

- `function` [`run`](rustsymbol-run-20c4c150.md)

## ekos/crates/cli/src/commands/commit.rs

- `function` [`ckm_object_to_kir`](rustsymbol-ckm-object-to-kir.md)
- `function` [`ckm_rel_to_kir`](rustsymbol-ckm-rel-to-kir.md)
- `function` [`evidence_record_to_kir`](rustsymbol-evidence-record-to-kir.md)
- `function` [`open_ledger`](rustsymbol-open-ledger.md)
- `function` [`run`](rustsymbol-run-5eff14dd.md)

## ekos/crates/cli/src/commands/compile.rs

- `function` [`knowledge_artifact_ids`](rustsymbol-knowledge-artifact-ids.md)
- `function` [`run`](rustsymbol-run.md)

## ekos/crates/cli/src/commands/dbt.rs

- `function` [`generate`](rustsymbol-generate.md)
- `function` [`resolve_output_dir`](rustsymbol-resolve-output-dir-730ab45b.md)
- `function` [`write_model`](rustsymbol-write-model.md)

## ekos/crates/cli/src/commands/diff.rs

- `function` [`run`](rustsymbol-run-b769e9f2.md)

## ekos/crates/cli/src/commands/docs.rs

- `enum` [`Format`](rustsymbol-format.md)
- `method` [`Format::parse`](rustsymbol-format-parse.md)
- `enum` [`Layout`](rustsymbol-layout.md)
- `method` [`Layout::parse`](rustsymbol-layout-parse.md)
- `function` [`confirm_prose_spend`](rustsymbol-confirm-prose-spend.md)
- `function` [`enrich_with_prose`](rustsymbol-enrich-with-prose.md)
- `function` [`estimate_prompt_tokens`](rustsymbol-estimate-prompt-tokens.md)
- `function` [`generate`](rustsymbol-generate-9628a7cf.md)
- `function` [`generate_curated`](rustsymbol-generate-curated.md)
- `function` [`render_er_diagram_page`](rustsymbol-render-er-diagram-page.md)
- `function` [`resolve_output_dir`](rustsymbol-resolve-output-dir.md)
- `function` [`select_llm_provider_for_prose`](rustsymbol-select-llm-provider-for-prose.md)
- `function` [`write_page`](rustsymbol-write-page.md)

## ekos/crates/cli/src/commands/doctor.rs

- `struct` [`Check`](rustsymbol-check.md)
- `method` [`Check::fail`](rustsymbol-check-fail.md)
- `method` [`Check::ok`](rustsymbol-check-ok.md)
- `function` [`run`](rustsymbol-run-a0c94dcf.md)

## ekos/crates/cli/src/commands/ekl.rs

- `function` [`render_cell`](rustsymbol-render-cell.md)
- `function` [`run`](rustsymbol-run-682eaf6b.md)

## ekos/crates/cli/src/commands/identity.rs

- `function` [`scan`](rustsymbol-scan.md)

## ekos/crates/cli/src/commands/init.rs

- `function` [`run`](rustsymbol-run-2a325902.md)

## ekos/crates/cli/src/commands/ledger.rs

- `function` [`dir_size`](rustsymbol-dir-size.md)
- `function` [`human_bytes`](rustsymbol-human-bytes.md)
- `function` [`migrate`](rustsymbol-migrate.md)
- `function` [`migrate_v3`](rustsymbol-migrate-v3.md)
- `function` [`print_storage_report`](rustsymbol-print-storage-report.md)
- `function` [`status`](rustsymbol-status.md)

## ekos/crates/cli/src/commands/marketing.rs

- `function` [`approve`](rustsymbol-approve.md)
- `function` [`log_line`](rustsymbol-log-line.md)
- `function` [`publish`](rustsymbol-publish.md)
- `function` [`resolve_devlog_path`](rustsymbol-resolve-devlog-path.md)
- `function` [`select_llm_provider`](rustsymbol-select-llm-provider.md)

## ekos/crates/cli/src/commands/mcp.rs

- `function` [`call_tool`](rustsymbol-call-tool.md)
- `function` [`diff_chains`](rustsymbol-diff-chains.md)
- `function` [`error_response`](rustsymbol-error-response.md)
- `function` [`explain_node`](rustsymbol-explain-node.md)
- `function` [`handle_message`](rustsymbol-handle-message.md)
- `function` [`initialize_result`](rustsymbol-initialize-result.md)
- `function` [`node_comparable`](rustsymbol-node-comparable.md)
- `function` [`node_summary`](rustsymbol-node-summary.md)
- `function` [`ok_response`](rustsymbol-ok-response.md)
- `function` [`required_id`](rustsymbol-required-id.md)
- `function` [`required_str`](rustsymbol-required-str.md)
- `function` [`run`](rustsymbol-run-6891f75c.md)
- `function` [`tool_definitions`](rustsymbol-tool-definitions.md)
- `function` [`tools_call`](rustsymbol-tools-call.md)
- `function` [`transformation_chain`](rustsymbol-transformation-chain.md)

## ekos/crates/cli/src/commands/mod.rs

- `function` [`init_logging`](rustsymbol-init-logging.md)
- `function` [`init_logging_stderr`](rustsymbol-init-logging-stderr.md)

## ekos/crates/cli/src/commands/query.rs

- `function` [`find`](rustsymbol-find.md)
- `function` [`neighbourhood`](rustsymbol-neighbourhood.md)
- `function` [`object`](rustsymbol-object-b6e1ea7f.md)
- `function` [`open_ledger`](rustsymbol-open-ledger-fce4a499.md)

## ekos/crates/cli/src/commands/recover.rs

- `function` [`build_llm_provider`](rustsymbol-build-llm-provider.md)
- `function` [`collect_confluence_artifact_ids`](rustsymbol-collect-confluence-artifact-ids.md)
- `function` [`collect_crypto_artifact_ids`](rustsymbol-collect-crypto-artifact-ids.md)
- `function` [`collect_git_artifact_ids`](rustsymbol-collect-git-artifact-ids.md)
- `function` [`collect_github_artifact_ids`](rustsymbol-collect-github-artifact-ids.md)
- `function` [`collect_localdocs_artifact_ids`](rustsymbol-collect-localdocs-artifact-ids.md)
- `function` [`collect_pentaho_artifact_ids`](rustsymbol-collect-pentaho-artifact-ids.md)
- `function` [`collect_python_artifact_ids`](rustsymbol-collect-python-artifact-ids.md)
- `function` [`collect_rust_artifact_ids`](rustsymbol-collect-rust-artifact-ids.md)
- `function` [`run`](rustsymbol-run-786d5225.md)
- `function` [`should_register_document_semantics`](rustsymbol-should-register-document-semantics.md)

## ekos/crates/cli/src/commands/resolve.rs

- `function` [`merge_into`](rustsymbol-merge-into.md)
- `function` [`run`](rustsymbol-run-e9261342.md)

## ekos/crates/cli/src/commands/store.rs

- `function` [`facts_dir`](rustsymbol-facts-dir.md)
- `function` [`open_store`](rustsymbol-open-store.md)
- `function` [`store_display`](rustsymbol-store-display.md)
- `function` [`uses_fact_engine`](rustsymbol-uses-fact-engine.md)

## ekos/crates/cli/tests/mcp_session.rs

- `function` [`call_tool`](rustsymbol-call-tool-79df7d9c.md)
- `function` [`claude_code_session_over_mcp`](rustsymbol-claude-code-session-over-mcp.md)
- `function` [`load_config`](rustsymbol-load-config.md)
- `function` [`setup_workspace`](rustsymbol-setup-workspace.md)

## ekos/crates/cli/tests/skeleton.rs

- `function` [`build_is_idempotent`](rustsymbol-build-is-idempotent.md)
- `function` [`build_observes_files_and_writes_ledger`](rustsymbol-build-observes-files-and-writes-ledger.md)
- `function` [`clean_removes_artifacts_not_ledger`](rustsymbol-clean-removes-artifacts-not-ledger.md)
- `function` [`init_creates_ekos_directory`](rustsymbol-init-creates-ekos-directory.md)
- `function` [`load_config`](rustsymbol-load-config-d1e71ee3.md)
- `function` [`query_object_returns_known_file`](rustsymbol-query-object-returns-known-file.md)
- `function` [`setup_workspace`](rustsymbol-setup-workspace-f8f102ad.md)

## ekos/crates/cli/tests/transformation_benchmark.rs

- `function` [`call_tool`](rustsymbol-call-tool-a762a492.md)
- `function` [`load_config`](rustsymbol-load-config-c16a7ca3.md)
- `function` [`phase7_benchmark_recover_explain_diff_over_mcp_only`](rustsymbol-phase7-benchmark-recover-explain-diff-over-mcp-only.md)
- `function` [`setup_workspace`](rustsymbol-setup-workspace-e8ff1e4b.md)

## ekos/crates/common/src/compress.rs

- `enum` [`CompressError`](rustsymbol-compresserror.md)
- `function` [`read_json_auto`](rustsymbol-read-json-auto.md)
- `function` [`read_json_zst`](rustsymbol-read-json-zst.md)
- `function` [`resolve_auto`](rustsymbol-resolve-auto.md)
- `function` [`write_json_zst`](rustsymbol-write-json-zst.md)
- `function` [`zst_sibling`](rustsymbol-zst-sibling.md)

## ekos/crates/common/src/lib.rs

- `struct` [`ContentHash`](rustsymbol-contenthash.md)
- `method` [`ContentHash::as_str`](rustsymbol-contenthash-as-str.md)
- `method` [`ContentHash::fmt`](rustsymbol-contenthash-fmt.md)
- `method` [`ContentHash::of`](rustsymbol-contenthash-of.md)
- `method` [`ContentHash::of_str`](rustsymbol-contenthash-of-str.md)

## ekos/crates/compiler-core/src/cache.rs

- `struct` [`PassManifest`](rustsymbol-passmanifest.md)
- `function` [`config_hash`](rustsymbol-config-hash.md)
- `function` [`manifest_path`](rustsymbol-manifest-path.md)
- `function` [`record_manifest`](rustsymbol-record-manifest.md)
- `function` [`should_recompute`](rustsymbol-should-recompute.md)

## ekos/crates/compiler-core/src/compiler.rs

- `struct` [`Compiler`](rustsymbol-compiler.md)
- `method` [`Compiler::new`](rustsymbol-compiler-new.md)
- `method` [`Compiler::register_pass`](rustsymbol-compiler-register-pass.md)
- `method` [`Compiler::run`](rustsymbol-compiler-run.md)
- `method` [`Compiler::with_failure_mode`](rustsymbol-compiler-with-failure-mode.md)
- `enum` [`CompilerError`](rustsymbol-compilererror.md)

## ekos/crates/compiler-core/src/config.rs

- `struct` [`AiConfig`](rustsymbol-aiconfig.md)
- `struct` [`DocumentSemanticsConfig`](rustsymbol-documentsemanticsconfig.md)
- `struct` [`EkosConfig`](rustsymbol-ekosconfig.md)
- `method` [`EkosConfig::artifact_dir`](rustsymbol-ekosconfig-artifact-dir.md)
- `method` [`EkosConfig::branch_ledger_path`](rustsymbol-ekosconfig-branch-ledger-path.md)
- `method` [`EkosConfig::default`](rustsymbol-ekosconfig-default.md)
- `method` [`EkosConfig::ekos_dir`](rustsymbol-ekosconfig-ekos-dir.md)
- `method` [`EkosConfig::from_file`](rustsymbol-ekosconfig-from-file.md)
- `method` [`EkosConfig::from_file_or_default`](rustsymbol-ekosconfig-from-file-or-default.md)
- `method` [`EkosConfig::ledger_dir`](rustsymbol-ekosconfig-ledger-dir.md)
- `method` [`EkosConfig::ledger_path`](rustsymbol-ekosconfig-ledger-path.md)
- `struct` [`LlmConfig`](rustsymbol-llmconfig.md)
- `struct` [`MarketingConfig`](rustsymbol-marketingconfig.md)
- `method` [`MarketingConfig::default`](rustsymbol-marketingconfig-default.md)
- `struct` [`ObserveConfig`](rustsymbol-observeconfig.md)
- `method` [`ObserveConfig::default`](rustsymbol-observeconfig-default.md)
- `struct` [`RecoverConfig`](rustsymbol-recoverconfig.md)
- `struct` [`SqlDialectRuleConfig`](rustsymbol-sqldialectruleconfig.md)
- `struct` [`SqlRecoverConfig`](rustsymbol-sqlrecoverconfig.md)
- `method` [`SqlRecoverConfig::default`](rustsymbol-sqlrecoverconfig-default.md)
- `struct` [`TwitterConfig`](rustsymbol-twitterconfig.md)
- `struct` [`WorkspaceConfig`](rustsymbol-workspaceconfig.md)
- `method` [`WorkspaceConfig::default`](rustsymbol-workspaceconfig-default.md)
- `function` [`default_github`](rustsymbol-default-github.md)
- `function` [`default_hashtags`](rustsymbol-default-hashtags.md)
- `function` [`default_ignore_patterns`](rustsymbol-default-ignore-patterns.md)
- `function` [`default_log_format`](rustsymbol-default-log-format.md)
- `function` [`default_log_level`](rustsymbol-default-log-level.md)
- `function` [`default_root`](rustsymbol-default-root.md)
- `function` [`default_sql_dialect`](rustsymbol-default-sql-dialect.md)

## ekos/crates/compiler-core/src/diagnostics.rs

- `struct` [`Diagnostic`](rustsymbol-diagnostic.md)
- `method` [`Diagnostic::at`](rustsymbol-diagnostic-at.md)
- `method` [`Diagnostic::error`](rustsymbol-diagnostic-error.md)
- `method` [`Diagnostic::info`](rustsymbol-diagnostic-info.md)
- `method` [`Diagnostic::warning`](rustsymbol-diagnostic-warning.md)
- `struct` [`DiagnosticSink`](rustsymbol-diagnosticsink.md)
- `method` [`DiagnosticSink::diagnostics`](rustsymbol-diagnosticsink-diagnostics.md)
- `method` [`DiagnosticSink::emit`](rustsymbol-diagnosticsink-emit.md)
- `method` [`DiagnosticSink::error`](rustsymbol-diagnosticsink-error.md)
- `method` [`DiagnosticSink::errors`](rustsymbol-diagnosticsink-errors.md)
- `method` [`DiagnosticSink::has_errors`](rustsymbol-diagnosticsink-has-errors.md)
- `method` [`DiagnosticSink::has_warnings`](rustsymbol-diagnosticsink-has-warnings.md)
- `method` [`DiagnosticSink::info`](rustsymbol-diagnosticsink-info.md)
- `method` [`DiagnosticSink::warning`](rustsymbol-diagnosticsink-warning.md)
- `method` [`DiagnosticSink::warning_count`](rustsymbol-diagnosticsink-warning-count.md)
- `enum` [`Severity`](rustsymbol-severity.md)
- `struct` [`SourceLocation`](rustsymbol-sourcelocation.md)

## ekos/crates/compiler-core/src/pass.rs

- `trait` [`CompilerPass`](rustsymbol-compilerpass.md)
- `struct` [`PassContext`](rustsymbol-passcontext.md)
- `method` [`PassContext::new`](rustsymbol-passcontext-new.md)
- `method` [`PassContext::with_artifact_store`](rustsymbol-passcontext-with-artifact-store.md)
- `enum` [`PassError`](rustsymbol-passerror.md)
- `method` [`PassError::failed`](rustsymbol-passerror-failed.md)
- `struct` [`PassManager`](rustsymbol-passmanager.md)
- `method` [`PassManager::check_unique_names`](rustsymbol-passmanager-check-unique-names.md)
- `method` [`PassManager::default`](rustsymbol-passmanager-default.md)
- `method` [`PassManager::execution_levels`](rustsymbol-passmanager-execution-levels.md)
- `method` [`PassManager::execution_order`](rustsymbol-passmanager-execution-order.md)
- `method` [`PassManager::is_empty`](rustsymbol-passmanager-is-empty.md)
- `method` [`PassManager::len`](rustsymbol-passmanager-len.md)
- `method` [`PassManager::new`](rustsymbol-passmanager-new.md)
- `method` [`PassManager::register`](rustsymbol-passmanager-register.md)
- `method` [`PassManager::run_all`](rustsymbol-passmanager-run-all.md)
- `method` [`PassManager::run_all_parallel`](rustsymbol-passmanager-run-all-parallel.md)
- `enum` [`SchedulerError`](rustsymbol-schedulererror.md)

## ekos/crates/compiler-core/src/scheduler.rs

- `struct` [`ExecutionReport`](rustsymbol-executionreport.md)
- `method` [`ExecutionReport::error_count`](rustsymbol-executionreport-error-count.md)
- `method` [`ExecutionReport::error_outcomes`](rustsymbol-executionreport-error-outcomes.md)
- `method` [`ExecutionReport::has_errors`](rustsymbol-executionreport-has-errors.md)
- `method` [`ExecutionReport::passes_run`](rustsymbol-executionreport-passes-run.md)
- `method` [`ExecutionReport::passes_skipped`](rustsymbol-executionreport-passes-skipped.md)
- `enum` [`FailureMode`](rustsymbol-failuremode.md)
- `struct` [`PassOutcome`](rustsymbol-passoutcome.md)
- `method` [`PassOutcome::ran`](rustsymbol-passoutcome-ran.md)
- `method` [`PassOutcome::skipped`](rustsymbol-passoutcome-skipped.md)
- `struct` [`Scheduler`](rustsymbol-scheduler.md)
- `method` [`Scheduler::new`](rustsymbol-scheduler-new.md)
- `method` [`Scheduler::register`](rustsymbol-scheduler-register.md)
- `method` [`Scheduler::run`](rustsymbol-scheduler-run.md)
- `method` [`Scheduler::run_parallel`](rustsymbol-scheduler-run-parallel.md)

## ekos/crates/dbt-gen/src/lib.rs

- `struct` [`AggExprRow`](rustsymbol-aggexprrow.md)
- `struct` [`DbtModelFile`](rustsymbol-dbtmodelfile.md)
- `function` [`comment_block`](rustsymbol-comment-block.md)
- `function` [`dbt_model_name`](rustsymbol-dbt-model-name.md)
- `function` [`get_aggs`](rustsymbol-get-aggs.md)
- `function` [`get_pairs`](rustsymbol-get-pairs.md)
- `function` [`get_str`](rustsymbol-get-str.md)
- `function` [`get_str_vec`](rustsymbol-get-str-vec.md)
- `function` [`is_feeds_into`](rustsymbol-is-feeds-into-af2f1802.md)
- `function` [`is_transform_node`](rustsymbol-is-transform-node.md)
- `function` [`no_upstream_placeholder`](rustsymbol-no-upstream-placeholder.md)
- `function` [`render_aggregate`](rustsymbol-render-aggregate.md)
- `function` [`render_calculate`](rustsymbol-render-calculate.md)
- `function` [`render_dbt_model`](rustsymbol-render-dbt-model.md)
- `function` [`render_filter`](rustsymbol-render-filter.md)
- `function` [`render_join`](rustsymbol-render-join.md)
- `function` [`render_schema_yml`](rustsymbol-render-schema-yml.md)
- `function` [`render_sink`](rustsymbol-render-sink.md)
- `function` [`render_source`](rustsymbol-render-source.md)
- `function` [`render_unmapped`](rustsymbol-render-unmapped.md)
- `function` [`slugify_snake`](rustsymbol-slugify-snake.md)
- `function` [`upstream_model_names`](rustsymbol-upstream-model-names.md)

## ekos/crates/docs-gen/src/lib.rs

- `struct` [`EvidenceRow`](rustsymbol-evidencerow.md)
- `struct` [`ObjectPageModel`](rustsymbol-objectpagemodel.md)
- `struct` [`ProseSection`](rustsymbol-prosesection.md)
- `struct` [`RelationshipRow`](rustsymbol-relationshiprow.md)
- `struct` [`RenderedPage`](rustsymbol-renderedpage.md)
- `enum` [`RowEvidence`](rustsymbol-rowevidence.md)
- `function` [`build_object_page_model`](rustsymbol-build-object-page-model.md)
- `function` [`components_cross_reference`](rustsymbol-components-cross-reference.md)
- `function` [`count_by_kind`](rustsymbol-count-by-kind.md)
- `function` [`format_value`](rustsymbol-format-value.md)
- `function` [`html_document`](rustsymbol-html-document.md)
- `function` [`html_escape`](rustsymbol-html-escape.md)
- `function` [`is_feeds_into`](rustsymbol-is-feeds-into.md)
- `function` [`is_module_kind`](rustsymbol-is-module-kind.md)
- `function` [`is_significant`](rustsymbol-is-significant.md)
- `function` [`is_symbol_kind`](rustsymbol-is-symbol-kind.md)
- `function` [`mermaid_arrow`](rustsymbol-mermaid-arrow.md)
- `function` [`mermaid_escape_label`](rustsymbol-mermaid-escape-label.md)
- `function` [`mermaid_node_id`](rustsymbol-mermaid-node-id.md)
- `function` [`page_file_name`](rustsymbol-page-file-name.md)
- `function` [`render_api`](rustsymbol-render-api.md)
- `function` [`render_api_from_legacy_file_symbols`](rustsymbol-render-api-from-legacy-file-symbols.md)
- `function` [`render_architecture`](rustsymbol-render-architecture.md)
- `function` [`render_call_sequences_section`](rustsymbol-render-call-sequences-section.md)
- `function` [`render_er_diagram`](rustsymbol-render-er-diagram.md)
- `function` [`render_html_er_diagram_page`](rustsymbol-render-html-er-diagram-page.md)
- `function` [`render_html_index_page`](rustsymbol-render-html-index-page.md)
- `function` [`render_html_object_page`](rustsymbol-render-html-object-page.md)
- `function` [`render_index_page`](rustsymbol-render-index-page.md)
- `function` [`render_markdown_object_page`](rustsymbol-render-markdown-object-page.md)
- `function` [`render_mermaid_graph`](rustsymbol-render-mermaid-graph.md)
- `function` [`render_object_page`](rustsymbol-render-object-page.md)
- `function` [`render_readme`](rustsymbol-render-readme.md)
- `function` [`render_relationship_kind_graph`](rustsymbol-render-relationship-kind-graph.md)
- `function` [`render_sequence_diagrams`](rustsymbol-render-sequence-diagrams.md)
- `function` [`sequence_participant_line`](rustsymbol-sequence-participant-line.md)
- `function` [`slugify`](rustsymbol-slugify.md)
- `function` [`strip_mermaid_fence`](rustsymbol-strip-mermaid-fence.md)
- `function` [`transform_node_origin`](rustsymbol-transform-node-origin.md)
- `function` [`unique_page_file_names`](rustsymbol-unique-page-file-names.md)

## ekos/crates/ekl/src/interpreter.rs

- `enum` [`EklError`](rustsymbol-eklerror.md)
- `struct` [`EklInterpreter`](rustsymbol-eklinterpreter.md)
- `method` [`EklInterpreter::candidate_rows`](rustsymbol-eklinterpreter-candidate-rows.md)
- `method` [`EklInterpreter::execute`](rustsymbol-eklinterpreter-execute.md)
- `method` [`EklInterpreter::expand_from_anchor`](rustsymbol-eklinterpreter-expand-from-anchor.md)
- `method` [`EklInterpreter::new`](rustsymbol-eklinterpreter-new.md)
- `method` [`EklInterpreter::resolve_anchor`](rustsymbol-eklinterpreter-resolve-anchor.md)
- `struct` [`EklResult`](rustsymbol-eklresult.md)
- `function` [`compare_rows`](rustsymbol-compare-rows.md)
- `function` [`default_returns`](rustsymbol-default-returns.md)
- `function` [`eval_predicate`](rustsymbol-eval-predicate.md)
- `function` [`literal_as_f64`](rustsymbol-literal-as-f64.md)
- `function` [`literal_to_string`](rustsymbol-literal-to-string.md)
- `function` [`object_row`](rustsymbol-object-row.md)
- `function` [`project`](rustsymbol-project-990853c7.md)
- `function` [`relationship_row`](rustsymbol-relationship-row.md)
- `function` [`value_as_f64`](rustsymbol-value-as-f64.md)
- `function` [`value_eq`](rustsymbol-value-eq.md)
- `function` [`value_to_string`](rustsymbol-value-to-string.md)

## ekos/crates/ekl/src/parser.rs

- `struct` [`EklAst`](rustsymbol-eklast.md)
- `enum` [`Entity`](rustsymbol-entity.md)
- `struct` [`Lexer`](rustsymbol-lexer.md)
- `method` [`Lexer::match_symbol_op`](rustsymbol-lexer-match-symbol-op.md)
- `method` [`Lexer::new`](rustsymbol-lexer-new.md)
- `method` [`Lexer::read_ident`](rustsymbol-lexer-read-ident.md)
- `method` [`Lexer::read_number`](rustsymbol-lexer-read-number.md)
- `method` [`Lexer::read_string`](rustsymbol-lexer-read-string.md)
- `method` [`Lexer::skip_whitespace`](rustsymbol-lexer-skip-whitespace.md)
- `method` [`Lexer::tokenize`](rustsymbol-lexer-tokenize.md)
- `enum` [`Literal`](rustsymbol-literal.md)
- `enum` [`Op`](rustsymbol-op.md)
- `enum` [`Order`](rustsymbol-order.md)
- `struct` [`ParseError`](rustsymbol-parseerror.md)
- `method` [`ParseError::fmt`](rustsymbol-parseerror-fmt.md)
- `struct` [`Parser`](rustsymbol-parser.md)
- `method` [`Parser::advance`](rustsymbol-parser-advance.md)
- `method` [`Parser::expect_ident`](rustsymbol-parser-expect-ident.md)
- `method` [`Parser::expect_keyword`](rustsymbol-parser-expect-keyword.md)
- `method` [`Parser::expect_num`](rustsymbol-parser-expect-num.md)
- `method` [`Parser::expect_string`](rustsymbol-parser-expect-string.md)
- `method` [`Parser::new`](rustsymbol-parser-new.md)
- `method` [`Parser::parse_entity`](rustsymbol-parser-parse-entity.md)
- `method` [`Parser::parse_literal`](rustsymbol-parser-parse-literal.md)
- `method` [`Parser::parse_op`](rustsymbol-parser-parse-op.md)
- `method` [`Parser::parse_predicate`](rustsymbol-parser-parse-predicate.md)
- `method` [`Parser::parse_query`](rustsymbol-parser-parse-query.md)
- `method` [`Parser::peek`](rustsymbol-parser-peek.md)
- `method` [`Parser::peek_keyword`](rustsymbol-parser-peek-keyword.md)
- `method` [`Parser::peek_pos`](rustsymbol-parser-peek-pos.md)
- `struct` [`Predicate`](rustsymbol-predicate.md)
- `enum` [`Token`](rustsymbol-token.md)
- `function` [`describe`](rustsymbol-describe.md)
- `function` [`ekl_parse`](rustsymbol-ekl-parse.md)

## ekos/crates/identity/src/cross_system.rs

- `struct` [`CrossSystemCandidate`](rustsymbol-crosssystemcandidate.md)
- `struct` [`CrossSystemSignals`](rustsymbol-crosssystemsignals.md)
- `function` [`column_overlap_score`](rustsymbol-column-overlap-score.md)
- `function` [`column_types`](rustsymbol-column-types.md)
- `function` [`combine_signals`](rustsymbol-combine-signals.md)
- `function` [`find_cross_system_candidates`](rustsymbol-find-cross-system-candidates.md)
- `function` [`matchable_name`](rustsymbol-matchable-name.md)
- `function` [`normalize_cross_system`](rustsymbol-normalize-cross-system.md)
- `function` [`type_compat_score`](rustsymbol-type-compat-score.md)
- `function` [`type_family`](rustsymbol-type-family.md)

## ekos/crates/identity/src/lib.rs

- `enum` [`ConflictKind`](rustsymbol-conflictkind.md)
- `struct` [`ConflictReport`](rustsymbol-conflictreport.md)
- `struct` [`DefaultResolver`](rustsymbol-defaultresolver.md)
- `method` [`DefaultResolver::default`](rustsymbol-defaultresolver-default.md)
- `method` [`DefaultResolver::new`](rustsymbol-defaultresolver-new.md)
- `method` [`DefaultResolver::resolve`](rustsymbol-defaultresolver-resolve.md)
- `method` [`DefaultResolver::score`](rustsymbol-defaultresolver-score.md)
- `method` [`DefaultResolver::threshold_for`](rustsymbol-defaultresolver-threshold-for.md)
- `method` [`DefaultResolver::with_kind_threshold`](rustsymbol-defaultresolver-with-kind-threshold.md)
- `method` [`DefaultResolver::with_threshold`](rustsymbol-defaultresolver-with-threshold.md)
- `trait` [`IdentityResolver`](rustsymbol-identityresolver.md)
- `struct` [`MergeProposal`](rustsymbol-mergeproposal.md)
- `struct` [`ResolutionResult`](rustsymbol-resolutionresult.md)
- `struct` [`ResolutionStats`](rustsymbol-resolutionstats.md)
- `struct` [`ResolverConfig`](rustsymbol-resolverconfig.md)
- `method` [`ResolverConfig::default`](rustsymbol-resolverconfig-default.md)
- `struct` [`SimilarityScore`](rustsymbol-similarityscore.md)
- `struct` [`UnionFind`](rustsymbol-unionfind.md)
- `method` [`UnionFind::find`](rustsymbol-unionfind-find.md)
- `method` [`UnionFind::new`](rustsymbol-unionfind-new.md)
- `method` [`UnionFind::union`](rustsymbol-unionfind-union.md)
- `function` [`structural_score`](rustsymbol-structural-score.md)

## ekos/crates/identity/src/similarity.rs

- `function` [`column_names`](rustsymbol-column-names.md)
- `function` [`jaccard`](rustsymbol-jaccard.md)
- `function` [`jaro`](rustsymbol-jaro.md)
- `function` [`jaro_winkler`](rustsymbol-jaro-winkler.md)
- `function` [`normalize`](rustsymbol-normalize.md)

## ekos/crates/kir/src/lib.rs

- `enum` [`EventKind`](rustsymbol-eventkind.md)
- `struct` [`KirEvent`](rustsymbol-kirevent.md)
- `struct` [`KirEvidence`](rustsymbol-kirevidence.md)
- `method` [`KirEvidence::new`](rustsymbol-kirevidence-new.md)
- `method` [`KirEvidence::with_confidence`](rustsymbol-kirevidence-with-confidence.md)
- `struct` [`KirGraph`](rustsymbol-kirgraph.md)
- `method` [`KirGraph::add_evidence`](rustsymbol-kirgraph-add-evidence.md)
- `method` [`KirGraph::add_object`](rustsymbol-kirgraph-add-object.md)
- `method` [`KirGraph::add_relationship`](rustsymbol-kirgraph-add-relationship.md)
- `method` [`KirGraph::get_evidence`](rustsymbol-kirgraph-get-evidence.md)
- `method` [`KirGraph::get_object`](rustsymbol-kirgraph-get-object.md)
- `method` [`KirGraph::new`](rustsymbol-kirgraph-new.md)
- `struct` [`KirId`](rustsymbol-kirid.md)
- `method` [`KirId::as_str`](rustsymbol-kirid-as-str.md)
- `method` [`KirId::default`](rustsymbol-kirid-default.md)
- `method` [`KirId::fmt`](rustsymbol-kirid-fmt.md)
- `method` [`KirId::from_str`](rustsymbol-kirid-from-str.md)
- `method` [`KirId::new`](rustsymbol-kirid-new.md)
- `struct` [`KirObject`](rustsymbol-kirobject.md)
- `method` [`KirObject::indexed_content`](rustsymbol-kirobject-indexed-content.md)
- `method` [`KirObject::new`](rustsymbol-kirobject-new.md)
- `method` [`KirObject::with_evidence`](rustsymbol-kirobject-with-evidence.md)
- `method` [`KirObject::with_property`](rustsymbol-kirobject-with-property.md)
- `struct` [`KirRelationship`](rustsymbol-kirrelationship.md)
- `method` [`KirRelationship::is_pending_review`](rustsymbol-kirrelationship-is-pending-review.md)
- `method` [`KirRelationship::new`](rustsymbol-kirrelationship-new.md)
- `enum` [`ObjectKind`](rustsymbol-objectkind.md)
- `method` [`ObjectKind::fmt`](rustsymbol-objectkind-fmt.md)
- `enum` [`RelationshipKind`](rustsymbol-relationshipkind.md)
- `method` [`RelationshipKind::fmt`](rustsymbol-relationshipkind-fmt.md)
- `method` [`RelationshipKind::from_str`](rustsymbol-relationshipkind-from-str.md)
- `struct` [`SourceLocation`](rustsymbol-sourcelocation-f4972231.md)
- `method` [`SourceLocation::at`](rustsymbol-sourcelocation-at.md)
- `method` [`SourceLocation::file`](rustsymbol-sourcelocation-file.md)

## ekos/crates/ledger/src/fact.rs

- `struct` [`AttrId`](rustsymbol-attrid.md)
- `struct` [`AttributeRegistry`](rustsymbol-attributeregistry.md)
- `method` [`AttributeRegistry::get`](rustsymbol-attributeregistry-get.md)
- `method` [`AttributeRegistry::intern`](rustsymbol-attributeregistry-intern.md)
- `method` [`AttributeRegistry::is_empty`](rustsymbol-attributeregistry-is-empty.md)
- `method` [`AttributeRegistry::len`](rustsymbol-attributeregistry-len.md)
- `method` [`AttributeRegistry::name`](rustsymbol-attributeregistry-name.md)
- `method` [`AttributeRegistry::new`](rustsymbol-attributeregistry-new.md)
- `method` [`AttributeRegistry::reindex`](rustsymbol-attributeregistry-reindex.md)
- `struct` [`Fact`](rustsymbol-fact.md)
- `enum` [`FactError`](rustsymbol-facterror.md)
- `enum` [`FactOp`](rustsymbol-factop.md)
- `enum` [`FactValue`](rustsymbol-factvalue.md)
- `struct` [`TxId`](rustsymbol-txid.md)
- `function` [`canonical_uuid`](rustsymbol-canonical-uuid.md)
- `function` [`decompose`](rustsymbol-decompose.md)
- `function` [`diff`](rustsymbol-diff.md)
- `function` [`escape_segment`](rustsymbol-escape-segment.md)
- `function` [`flatten`](rustsymbol-flatten.md)
- `function` [`insert_path`](rustsymbol-insert-path.md)
- `function` [`reconstruct`](rustsymbol-reconstruct.md)
- `function` [`split_path`](rustsymbol-split-path.md)
- `function` [`type_name`](rustsymbol-type-name.md)
- `function` [`value_to_json`](rustsymbol-value-to-json.md)

## ekos/crates/ledger/src/fact_ledger.rs

- `enum` [`EntityKind`](rustsymbol-entitykind.md)
- `struct` [`FactLedger`](rustsymbol-factledger.md)
- `method` [`FactLedger::all_objects`](rustsymbol-factledger-all-objects.md)
- `method` [`FactLedger::all_of_kind`](rustsymbol-factledger-all-of-kind.md)
- `method` [`FactLedger::all_relationships`](rustsymbol-factledger-all-relationships.md)
- `method` [`FactLedger::append_event`](rustsymbol-factledger-append-event.md)
- `method` [`FactLedger::append_evidence`](rustsymbol-factledger-append-evidence.md)
- `method` [`FactLedger::append_inner`](rustsymbol-factledger-append-inner.md)
- `method` [`FactLedger::append_object`](rustsymbol-factledger-append-object.md)
- `method` [`FactLedger::append_payload`](rustsymbol-factledger-append-payload.md)
- `method` [`FactLedger::append_relationship`](rustsymbol-factledger-append-relationship.md)
- `method` [`FactLedger::append_version`](rustsymbol-factledger-append-version.md)
- `method` [`FactLedger::current_signature`](rustsymbol-factledger-current-signature.md)
- `method` [`FactLedger::diff`](rustsymbol-factledger-diff.md)
- `method` [`FactLedger::entry_count`](rustsymbol-factledger-entry-count.md)
- `method` [`FactLedger::find_objects`](rustsymbol-factledger-find-objects.md)
- `method` [`FactLedger::get_event`](rustsymbol-factledger-get-event.md)
- `method` [`FactLedger::get_evidence`](rustsymbol-factledger-get-evidence.md)
- `method` [`FactLedger::get_object`](rustsymbol-factledger-get-object.md)
- `method` [`FactLedger::get_relationship`](rustsymbol-factledger-get-relationship.md)
- `method` [`FactLedger::merge_from`](rustsymbol-factledger-merge-from.md)
- `method` [`FactLedger::object_at`](rustsymbol-factledger-object-at.md)
- `method` [`FactLedger::object_count`](rustsymbol-factledger-object-count.md)
- `method` [`FactLedger::open`](rustsymbol-factledger-open.md)
- `method` [`FactLedger::open_with_seal_threshold`](rustsymbol-factledger-open-with-seal-threshold.md)
- `method` [`FactLedger::relationship_count`](rustsymbol-factledger-relationship-count.md)
- `method` [`FactLedger::relationships_at`](rustsymbol-factledger-relationships-at.md)
- `method` [`FactLedger::relationships_for`](rustsymbol-factledger-relationships-for.md)
- `method` [`FactLedger::run_count`](rustsymbol-factledger-run-count.md)
- `method` [`FactLedger::seal_and_flush`](rustsymbol-factledger-seal-and-flush.md)
- `method` [`FactLedger::set_segment_dictionary`](rustsymbol-factledger-set-segment-dictionary.md)
- `method` [`FactLedger::typed_current`](rustsymbol-factledger-typed-current.md)
- `method` [`FactLedger::vacuum_into`](rustsymbol-factledger-vacuum-into.md)
- `struct` [`Inner`](rustsymbol-inner.md)
- `method` [`Inner::all_current_payloads`](rustsymbol-inner-all-current-payloads.md)
- `method` [`Inner::current_sig`](rustsymbol-inner-current-sig.md)
- `method` [`Inner::entities_with_attr`](rustsymbol-inner-entities-with-attr.md)
- `method` [`Inner::entity_entries`](rustsymbol-inner-entity-entries.md)
- `method` [`Inner::flush_memtable`](rustsymbol-inner-flush-memtable.md)
- `method` [`Inner::index_object`](rustsymbol-inner-index-object.md)
- `method` [`Inner::reconstruct_at`](rustsymbol-inner-reconstruct-at.md)
- `method` [`Inner::relationship_candidates`](rustsymbol-inner-relationship-candidates.md)
- `method` [`Inner::runs_dir`](rustsymbol-inner-runs-dir.md)
- `method` [`Inner::state_at`](rustsymbol-inner-state-at.md)
- `method` [`Inner::tx_at`](rustsymbol-inner-tx-at.md)
- `method` [`LedgerError::from`](rustsymbol-ledgererror-from.md)
- `function` [`copy_dir`](rustsymbol-copy-dir.md)
- `function` [`fold_state`](rustsymbol-fold-state.md)
- `function` [`kind_of_payload`](rustsymbol-kind-of-payload.md)
- `function` [`self_counts`](rustsymbol-self-counts.md)

## ekos/crates/ledger/src/index.rs

- `struct` [`BlockMeta`](rustsymbol-blockmeta.md)
- `struct` [`FactIndexes`](rustsymbol-factindexes.md)
- `method` [`FactIndexes::add_runs`](rustsymbol-factindexes-add-runs.md)
- `method` [`FactIndexes::build_from_batches`](rustsymbol-factindexes-build-from-batches.md)
- `method` [`FactIndexes::merge_runs`](rustsymbol-factindexes-merge-runs.md)
- `method` [`FactIndexes::open`](rustsymbol-factindexes-open.md)
- `method` [`FactIndexes::run_count`](rustsymbol-factindexes-run-count.md)
- `method` [`FactIndexes::runs_of`](rustsymbol-factindexes-runs-of.md)
- `method` [`FactIndexes::scan`](rustsymbol-factindexes-scan.md)
- `struct` [`IndexEntry`](rustsymbol-indexentry.md)
- `method` [`IndexEntry::from_fact`](rustsymbol-indexentry-from-fact.md)
- `struct` [`IndexRun`](rustsymbol-indexrun.md)
- `method` [`IndexRun::all`](rustsymbol-indexrun-all.md)
- `method` [`IndexRun::all_raw`](rustsymbol-indexrun-all-raw.md)
- `method` [`IndexRun::entry_count`](rustsymbol-indexrun-entry-count.md)
- `method` [`IndexRun::open`](rustsymbol-indexrun-open.md)
- `method` [`IndexRun::order`](rustsymbol-indexrun-order.md)
- `method` [`IndexRun::read_block_raw`](rustsymbol-indexrun-read-block-raw.md)
- `method` [`IndexRun::scan`](rustsymbol-indexrun-scan.md)
- `struct` [`RunDirectory`](rustsymbol-rundirectory.md)
- `enum` [`ScanPrefix`](rustsymbol-scanprefix.md)
- `method` [`ScanPrefix::bytes`](rustsymbol-scanprefix-bytes.md)
- `method` [`ScanPrefix::order`](rustsymbol-scanprefix-order.md)
- `enum` [`SortOrder`](rustsymbol-sortorder.md)
- `method` [`SortOrder::prefix`](rustsymbol-sortorder-prefix.md)
- `function` [`decode_block`](rustsymbol-decode-block.md)
- `function` [`encode_block`](rustsymbol-encode-block.md)
- `function` [`encode_key`](rustsymbol-encode-key.md)
- `function` [`entries_from_batches`](rustsymbol-entries-from-batches.md)
- `function` [`in_prefix`](rustsymbol-in-prefix.md)
- `function` [`project`](rustsymbol-project.md)
- `function` [`push_escaped`](rustsymbol-push-escaped.md)
- `function` [`push_pos`](rustsymbol-push-pos.md)
- `function` [`stores_values`](rustsymbol-stores-values.md)
- `function` [`value_order_key`](rustsymbol-value-order-key.md)
- `function` [`write_run`](rustsymbol-write-run.md)
- `function` [`write_run_raw`](rustsymbol-write-run-raw.md)

## ekos/crates/ledger/src/lib.rs

- `enum` [`Codec`](rustsymbol-codec.md)
- `method` [`Codec::compress`](rustsymbol-codec-compress.md)
- `method` [`Codec::decompress`](rustsymbol-codec-decompress.md)
- `method` [`Codec::zstd`](rustsymbol-codec-zstd.md)
- `struct` [`Dict`](rustsymbol-dict.md)
- `enum` [`EntryType`](rustsymbol-entrytype.md)
- `method` [`EntryType::as_str`](rustsymbol-entrytype-as-str.md)
- `method` [`FactLedger::diff_impl`](rustsymbol-factledger-diff-impl.md)
- `enum` [`Format`](rustsymbol-format-2bc470e0.md)
- `trait` [`KnowledgeStore`](rustsymbol-knowledgestore.md)
- `struct` [`Ledger`](rustsymbol-ledger.md)
- `method` [`Ledger::all_objects`](rustsymbol-ledger-all-objects.md)
- `method` [`Ledger::all_objects_with_rowids`](rustsymbol-ledger-all-objects-with-rowids.md)
- `method` [`Ledger::all_relationships`](rustsymbol-ledger-all-relationships.md)
- `method` [`Ledger::append`](rustsymbol-ledger-append.md)
- `method` [`Ledger::append_event`](rustsymbol-ledger-append-event.md)
- `method` [`Ledger::append_evidence`](rustsymbol-ledger-append-evidence.md)
- `method` [`Ledger::append_object`](rustsymbol-ledger-append-object.md)
- `method` [`Ledger::append_relationship`](rustsymbol-ledger-append-relationship.md)
- `method` [`Ledger::append_versioned`](rustsymbol-ledger-append-versioned.md)
- `method` [`Ledger::create_v2`](rustsymbol-ledger-create-v2.md)
- `method` [`Ledger::diff_impl`](rustsymbol-ledger-diff-impl.md)
- `method` [`Ledger::entry_count`](rustsymbol-ledger-entry-count.md)
- `method` [`Ledger::export_versions`](rustsymbol-ledger-export-versions.md)
- `method` [`Ledger::find_objects`](rustsymbol-ledger-find-objects.md)
- `method` [`Ledger::find_objects_v1`](rustsymbol-ledger-find-objects-v1.md)
- `method` [`Ledger::find_objects_v2`](rustsymbol-ledger-find-objects-v2.md)
- `method` [`Ledger::get_event`](rustsymbol-ledger-get-event.md)
- `method` [`Ledger::get_evidence`](rustsymbol-ledger-get-evidence.md)
- `method` [`Ledger::get_object`](rustsymbol-ledger-get-object.md)
- `method` [`Ledger::get_relationship`](rustsymbol-ledger-get-relationship.md)
- `method` [`Ledger::id_param`](rustsymbol-ledger-id-param.md)
- `method` [`Ledger::index_object_fts_v1`](rustsymbol-ledger-index-object-fts-v1.md)
- `method` [`Ledger::index_object_fts_v2`](rustsymbol-ledger-index-object-fts-v2.md)
- `method` [`Ledger::migrate_fts_v2`](rustsymbol-ledger-migrate-fts-v2.md)
- `method` [`Ledger::object_at`](rustsymbol-ledger-object-at.md)
- `method` [`Ledger::object_count`](rustsymbol-ledger-object-count.md)
- `method` [`Ledger::open`](rustsymbol-ledger-open.md)
- `method` [`Ledger::payload_param`](rustsymbol-ledger-payload-param.md)
- `method` [`Ledger::payload_to_string`](rustsymbol-ledger-payload-to-string.md)
- `method` [`Ledger::query_payloads`](rustsymbol-ledger-query-payloads.md)
- `method` [`Ledger::relationship_count`](rustsymbol-ledger-relationship-count.md)
- `method` [`Ledger::relationships_at`](rustsymbol-ledger-relationships-at.md)
- `method` [`Ledger::relationships_for`](rustsymbol-ledger-relationships-for.md)
- `method` [`Ledger::sig_param`](rustsymbol-ledger-sig-param.md)
- `method` [`Ledger::storage_stats`](rustsymbol-ledger-storage-stats.md)
- `method` [`Ledger::ts_param`](rustsymbol-ledger-ts-param.md)
- `method` [`Ledger::vacuum_into`](rustsymbol-ledger-vacuum-into.md)
- `method` [`Ledger::versions_in_window`](rustsymbol-ledger-versions-in-window.md)
- `struct` [`LedgerDiff`](rustsymbol-ledgerdiff.md)
- `struct` [`LedgerEntry`](rustsymbol-ledgerentry.md)
- `struct` [`LedgerEntryId`](rustsymbol-ledgerentryid.md)
- `enum` [`LedgerError`](rustsymbol-ledgererror.md)
- `struct` [`MergeConflict`](rustsymbol-mergeconflict.md)
- `struct` [`MergeReport`](rustsymbol-mergereport.md)
- `struct` [`MigrateReport`](rustsymbol-migratereport.md)
- `struct` [`MigrateV3Report`](rustsymbol-migratev3report.md)
- `struct` [`VersionRow`](rustsymbol-versionrow.md)
- `function` [`content_signature`](rustsymbol-content-signature.md)
- `function` [`diff_ledger`](rustsymbol-diff-ledger.md)
- `function` [`dir_bytes`](rustsymbol-dir-bytes.md)
- `function` [`id_value_to_string`](rustsymbol-id-value-to-string.md)
- `function` [`init_schema_v2`](rustsymbol-init-schema-v2.md)
- `function` [`load_dictionary`](rustsymbol-load-dictionary.md)
- `function` [`merge_branch`](rustsymbol-merge-branch.md)
- `function` [`merge_stores`](rustsymbol-merge-stores.md)
- `function` [`migrate_to_v2`](rustsymbol-migrate-to-v2.md)
- `function` [`migrate_to_v3`](rustsymbol-migrate-to-v3.md)
- `function` [`payload_samples`](rustsymbol-payload-samples.md)
- `function` [`sibling_path`](rustsymbol-sibling-path.md)
- `function` [`sig_value_to_hex`](rustsymbol-sig-value-to-hex.md)
- `function` [`ts_value_to_datetime`](rustsymbol-ts-value-to-datetime.md)

## ekos/crates/ledger/src/search.rs

- `struct` [`SearchIndex`](rustsymbol-searchindex.md)
- `method` [`SearchIndex::commit`](rustsymbol-searchindex-commit.md)
- `method` [`SearchIndex::open`](rustsymbol-searchindex-open.md)
- `method` [`SearchIndex::query`](rustsymbol-searchindex-query.md)
- `method` [`SearchIndex::upsert`](rustsymbol-searchindex-upsert.md)
- `function` [`terr`](rustsymbol-terr.md)

## ekos/crates/ledger/src/segment/map.rs

- `struct` [`MappedSegment`](rustsymbol-mappedsegment.md)
- `method` [`MappedSegment::bytes`](rustsymbol-mappedsegment-bytes.md)
- `method` [`MappedSegment::open`](rustsymbol-mappedsegment-open.md)

## ekos/crates/ledger/src/segment/mod.rs

- `struct` [`Batch`](rustsymbol-batch.md)
- `struct` [`Head`](rustsymbol-head.md)
- `struct` [`Manifest`](rustsymbol-manifest.md)
- `struct` [`SealedSegment`](rustsymbol-sealedsegment.md)
- `struct` [`SegDict`](rustsymbol-segdict.md)
- `enum` [`SegmentError`](rustsymbol-segmenterror.md)
- `struct` [`SegmentStore`](rustsymbol-segmentstore.md)
- `method` [`SegmentStore::active_batches`](rustsymbol-segmentstore-active-batches.md)
- `method` [`SegmentStore::append`](rustsymbol-segmentstore-append.md)
- `method` [`SegmentStore::append_with_seal`](rustsymbol-segmentstore-append-with-seal.md)
- `method` [`SegmentStore::batch_headers`](rustsymbol-segmentstore-batch-headers.md)
- `method` [`SegmentStore::batches`](rustsymbol-segmentstore-batches.md)
- `method` [`SegmentStore::batches_after`](rustsymbol-segmentstore-batches-after.md)
- `method` [`SegmentStore::committed_len`](rustsymbol-segmentstore-committed-len.md)
- `method` [`SegmentStore::encode_frame`](rustsymbol-segmentstore-encode-frame.md)
- `method` [`SegmentStore::next_tx`](rustsymbol-segmentstore-next-tx.md)
- `method` [`SegmentStore::open`](rustsymbol-segmentstore-open.md)
- `method` [`SegmentStore::open_with_seal_threshold`](rustsymbol-segmentstore-open-with-seal-threshold.md)
- `method` [`SegmentStore::persist_manifest`](rustsymbol-segmentstore-persist-manifest.md)
- `method` [`SegmentStore::read_active_committed`](rustsymbol-segmentstore-read-active-committed.md)
- `method` [`SegmentStore::root`](rustsymbol-segmentstore-root.md)
- `method` [`SegmentStore::seal_active`](rustsymbol-segmentstore-seal-active.md)
- `method` [`SegmentStore::set_dictionary`](rustsymbol-segmentstore-set-dictionary.md)
- `method` [`SegmentStore::verify_sealed`](rustsymbol-segmentstore-verify-sealed.md)
- `function` [`atomic_write`](rustsymbol-atomic-write.md)
- `function` [`build_dict`](rustsymbol-build-dict.md)
- `function` [`decode_frame`](rustsymbol-decode-frame.md)
- `function` [`decode_header`](rustsymbol-decode-header.md)
- `function` [`hash_file`](rustsymbol-hash-file.md)
- `function` [`load_manifest`](rustsymbol-load-manifest.md)
- `function` [`save_manifest`](rustsymbol-save-manifest.md)
- `function` [`scan_batches_filtered`](rustsymbol-scan-batches-filtered.md)
- `function` [`scan_headers_slice`](rustsymbol-scan-headers-slice.md)
- `function` [`scan_slice`](rustsymbol-scan-slice.md)
- `function` [`segment_path`](rustsymbol-segment-path.md)
- `function` [`walk_frames`](rustsymbol-walk-frames.md)
- `function` [`write_head`](rustsymbol-write-head.md)

## ekos/crates/ledger/tests/estate_migration.rs

- `function` [`dir_bytes`](rustsymbol-dir-bytes-a1c5e8ff.md)
- `function` [`mb`](rustsymbol-mb.md)
- `function` [`migrate_estate_and_report_sizes`](rustsymbol-migrate-estate-and-report-sizes.md)

## ekos/crates/marketing/src/devlog.rs

- `enum` [`DevlogParseError`](rustsymbol-devlogparseerror.md)
- `struct` [`DevlogSummary`](rustsymbol-devlogsummary.md)
- `function` [`extract_section`](rustsymbol-extract-section.md)
- `function` [`find_latest`](rustsymbol-find-latest.md)
- `function` [`number_from_filename`](rustsymbol-number-from-filename.md)
- `function` [`parse`](rustsymbol-parse.md)
- `function` [`split_once_any_dash`](rustsymbol-split-once-any-dash.md)

## ekos/crates/marketing/src/importance.rs

- `enum` [`Importance`](rustsymbol-importance.md)
- `function` [`classify`](rustsymbol-classify.md)

## ekos/crates/marketing/src/oauth1.rs

- `struct` [`OauthCredentials`](rustsymbol-oauthcredentials.md)
- `function` [`authorization_header`](rustsymbol-authorization-header.md)
- `function` [`generate_nonce`](rustsymbol-generate-nonce.md)
- `function` [`normalized_param_string`](rustsymbol-normalized-param-string.md)
- `function` [`percent_encode`](rustsymbol-percent-encode.md)
- `function` [`sign`](rustsymbol-sign.md)
- `function` [`signature_base_string`](rustsymbol-signature-base-string.md)
- `function` [`unix_timestamp`](rustsymbol-unix-timestamp.md)

## ekos/crates/marketing/src/prompt.rs

- `function` [`build_retry_suffix`](rustsymbol-build-retry-suffix.md)
- `function` [`build_user_prompt`](rustsymbol-build-user-prompt.md)
- `function` [`overage_from_too_long_reason`](rustsymbol-overage-from-too-long-reason.md)

## ekos/crates/marketing/src/publisher.rs

- `struct` [`NoopPublisher`](rustsymbol-nooppublisher.md)
- `method` [`NoopPublisher::publish`](rustsymbol-nooppublisher-publish.md)
- `enum` [`PublishError`](rustsymbol-publisherror.md)
- `trait` [`Publisher`](rustsymbol-publisher.md)
- `struct` [`TweetCreateData`](rustsymbol-tweetcreatedata.md)
- `struct` [`TweetCreateResponse`](rustsymbol-tweetcreateresponse.md)
- `struct` [`TwitterPublisher`](rustsymbol-twitterpublisher.md)
- `method` [`TwitterPublisher::from_env`](rustsymbol-twitterpublisher-from-env.md)
- `method` [`TwitterPublisher::new`](rustsymbol-twitterpublisher-new.md)
- `method` [`TwitterPublisher::publish`](rustsymbol-twitterpublisher-publish.md)

## ekos/crates/marketing/src/store.rs

- `struct` [`PostedStore`](rustsymbol-postedstore.md)
- `method` [`PostedStore::is_posted`](rustsymbol-postedstore-is-posted.md)
- `method` [`PostedStore::load`](rustsymbol-postedstore-load.md)
- `method` [`PostedStore::record`](rustsymbol-postedstore-record.md)
- `method` [`PostedStore::save`](rustsymbol-postedstore-save.md)
- `struct` [`PostedTweet`](rustsymbol-postedtweet.md)
- `enum` [`StoreError`](rustsymbol-storeerror-e1b41824.md)

## ekos/crates/marketing/src/tweet.rs

- `struct` [`LlmTweetOutput`](rustsymbol-llmtweetoutput.md)
- `enum` [`MarketingError`](rustsymbol-marketingerror.md)
- `struct` [`TweetDraft`](rustsymbol-tweetdraft.md)
- `enum` [`TweetValidationError`](rustsymbol-tweetvalidationerror.md)
- `function` [`draft_once`](rustsymbol-draft-once.md)
- `function` [`generate_tweet`](rustsymbol-generate-tweet.md)
- `function` [`validate_tweet`](rustsymbol-validate-tweet.md)

## ekos/crates/observation-sdk/src/lib.rs

- `struct` [`ConnectorConfig`](rustsymbol-connectorconfig.md)
- `method` [`ConnectorConfig::get_bool`](rustsymbol-connectorconfig-get-bool.md)
- `method` [`ConnectorConfig::get_str`](rustsymbol-connectorconfig-get-str.md)
- `struct` [`Fingerprint`](rustsymbol-fingerprint.md)
- `struct` [`ObservationPackage`](rustsymbol-observationpackage.md)
- `method` [`ObservationPackage::is_empty`](rustsymbol-observationpackage-is-empty.md)
- `method` [`ObservationPackage::len`](rustsymbol-observationpackage-len.md)
- `method` [`ObservationPackage::new`](rustsymbol-observationpackage-new.md)
- `method` [`ObservationPackage::push`](rustsymbol-observationpackage-push.md)
- `enum` [`ObserveError`](rustsymbol-observeerror.md)
- `method` [`ObserveError::connector`](rustsymbol-observeerror-connector.md)
- `trait` [`Observer`](rustsymbol-observer.md)
- `struct` [`PackageMeta`](rustsymbol-packagemeta.md)
- `struct` [`ScanContext`](rustsymbol-scancontext.md)
- `method` [`ScanContext::is_ignored`](rustsymbol-scancontext-is-ignored.md)
- `method` [`ScanContext::new`](rustsymbol-scancontext-new.md)
- `method` [`ScanContext::with_config`](rustsymbol-scancontext-with-config.md)
- `method` [`ScanContext::with_ignore_patterns`](rustsymbol-scancontext-with-ignore-patterns.md)
- `function` [`source_fingerprint`](rustsymbol-source-fingerprint.md)

## ekos/crates/recovery/src/anthropic.rs

- `struct` [`AnthropicProvider`](rustsymbol-anthropicprovider.md)
- `method` [`AnthropicProvider::complete`](rustsymbol-anthropicprovider-complete.md)
- `method` [`AnthropicProvider::from_env`](rustsymbol-anthropicprovider-from-env.md)
- `method` [`AnthropicProvider::from_env_var`](rustsymbol-anthropicprovider-from-env-var.md)
- `method` [`AnthropicProvider::model_name`](rustsymbol-anthropicprovider-model-name.md)
- `method` [`AnthropicProvider::new`](rustsymbol-anthropicprovider-new.md)
- `struct` [`ApiContent`](rustsymbol-apicontent.md)
- `struct` [`ApiMessage`](rustsymbol-apimessage.md)
- `struct` [`ApiRequest`](rustsymbol-apirequest-d7b913bf.md)
- `struct` [`ApiResponse`](rustsymbol-apiresponse.md)
- `struct` [`ApiUsage`](rustsymbol-apiusage.md)

## ekos/crates/recovery/src/cache.rs

- `struct` [`CachedLlmProvider`](rustsymbol-cachedllmprovider.md)
- `method` [`CachedLlmProvider::cache_root`](rustsymbol-cachedllmprovider-cache-root.md)
- `method` [`CachedLlmProvider::complete`](rustsymbol-cachedllmprovider-complete.md)
- `method` [`CachedLlmProvider::model_name`](rustsymbol-cachedllmprovider-model-name.md)
- `method` [`CachedLlmProvider::new`](rustsymbol-cachedllmprovider-new.md)
- `function` [`cache_key`](rustsymbol-cache-key.md)
- `function` [`cache_path`](rustsymbol-cache-path.md)

## ekos/crates/recovery/src/cicd_analyzer.rs

- `struct` [`CicdAnalyzerPass`](rustsymbol-cicdanalyzerpass.md)
- `method` [`CicdAnalyzerPass::cache_inputs`](rustsymbol-cicdanalyzerpass-cache-inputs.md)
- `method` [`CicdAnalyzerPass::name`](rustsymbol-cicdanalyzerpass-name.md)
- `method` [`CicdAnalyzerPass::new`](rustsymbol-cicdanalyzerpass-new.md)
- `method` [`CicdAnalyzerPass::run`](rustsymbol-cicdanalyzerpass-run.md)
- `function` [`extract_jobs`](rustsymbol-extract-jobs.md)
- `function` [`extract_triggers`](rustsymbol-extract-triggers.md)
- `function` [`pipeline_kir_id`](rustsymbol-pipeline-kir-id.md)

## ekos/crates/recovery/src/confluence_analyzer.rs

- `struct` [`ConfluenceAnalyzerPass`](rustsymbol-confluenceanalyzerpass.md)
- `method` [`ConfluenceAnalyzerPass::cache_inputs`](rustsymbol-confluenceanalyzerpass-cache-inputs.md)
- `method` [`ConfluenceAnalyzerPass::name`](rustsymbol-confluenceanalyzerpass-name.md)
- `method` [`ConfluenceAnalyzerPass::new`](rustsymbol-confluenceanalyzerpass-new.md)
- `method` [`ConfluenceAnalyzerPass::run`](rustsymbol-confluenceanalyzerpass-run.md)
- `struct` [`PageData`](rustsymbol-pagedata.md)
- `function` [`body_excerpt`](rustsymbol-body-excerpt.md)
- `function` [`find_linked_titles`](rustsymbol-find-linked-titles.md)
- `function` [`page_kir_id`](rustsymbol-page-kir-id.md)

## ekos/crates/recovery/src/crate_topology_analyzer.rs

- `struct` [`CrateTopologyAnalyzerPass`](rustsymbol-cratetopologyanalyzerpass.md)
- `method` [`CrateTopologyAnalyzerPass::cache_inputs`](rustsymbol-cratetopologyanalyzerpass-cache-inputs.md)
- `method` [`CrateTopologyAnalyzerPass::name`](rustsymbol-cratetopologyanalyzerpass-name.md)
- `method` [`CrateTopologyAnalyzerPass::new`](rustsymbol-cratetopologyanalyzerpass-new.md)
- `method` [`CrateTopologyAnalyzerPass::run`](rustsymbol-cratetopologyanalyzerpass-run.md)
- `enum` [`DepResolution`](rustsymbol-depresolution.md)
- `enum` [`WorkspaceDep`](rustsymbol-workspacedep.md)
- `function` [`crate_kir_id`](rustsymbol-crate-kir-id.md)
- `function` [`normalize_rel_path`](rustsymbol-normalize-rel-path.md)
- `function` [`resolve_dep_entry`](rustsymbol-resolve-dep-entry.md)
- `function` [`technology_kir_id`](rustsymbol-technology-kir-id-84387622.md)

## ekos/crates/recovery/src/crypto_analyzer.rs

- `struct` [`BatchData`](rustsymbol-batchdata.md)
- `struct` [`CryptoAnalyzerPass`](rustsymbol-cryptoanalyzerpass.md)
- `method` [`CryptoAnalyzerPass::cache_inputs`](rustsymbol-cryptoanalyzerpass-cache-inputs.md)
- `method` [`CryptoAnalyzerPass::name`](rustsymbol-cryptoanalyzerpass-name.md)
- `method` [`CryptoAnalyzerPass::new`](rustsymbol-cryptoanalyzerpass-new.md)
- `method` [`CryptoAnalyzerPass::run`](rustsymbol-cryptoanalyzerpass-run.md)
- `struct` [`EntityRow`](rustsymbol-entityrow.md)
- `struct` [`EvidenceRow`](rustsymbol-evidencerow-21ccac89.md)
- `struct` [`RelationshipRow`](rustsymbol-relationshiprow-5e78a376.md)
- `function` [`deterministic_id`](rustsymbol-deterministic-id.md)
- `function` [`parse_attrs`](rustsymbol-parse-attrs.md)

## ekos/crates/recovery/src/dependency_analyzer.rs

- `struct` [`DependencyAnalyzerPass`](rustsymbol-dependencyanalyzerpass.md)
- `method` [`DependencyAnalyzerPass::cache_inputs`](rustsymbol-dependencyanalyzerpass-cache-inputs.md)
- `method` [`DependencyAnalyzerPass::name`](rustsymbol-dependencyanalyzerpass-name.md)
- `method` [`DependencyAnalyzerPass::new`](rustsymbol-dependencyanalyzerpass-new.md)
- `method` [`DependencyAnalyzerPass::run`](rustsymbol-dependencyanalyzerpass-run.md)
- `function` [`file_kir_id`](rustsymbol-file-kir-id.md)
- `function` [`technology_kir_id`](rustsymbol-technology-kir-id.md)

## ekos/crates/recovery/src/document_semantics_analyzer.rs

- `struct` [`DocumentSemanticsAnalyzerPass`](rustsymbol-documentsemanticsanalyzerpass.md)
- `method` [`DocumentSemanticsAnalyzerPass::collect_sections`](rustsymbol-documentsemanticsanalyzerpass-collect-sections.md)
- `method` [`DocumentSemanticsAnalyzerPass::dependencies`](rustsymbol-documentsemanticsanalyzerpass-dependencies.md)
- `method` [`DocumentSemanticsAnalyzerPass::name`](rustsymbol-documentsemanticsanalyzerpass-name.md)
- `method` [`DocumentSemanticsAnalyzerPass::new`](rustsymbol-documentsemanticsanalyzerpass-new.md)
- `method` [`DocumentSemanticsAnalyzerPass::run`](rustsymbol-documentsemanticsanalyzerpass-run.md)
- `method` [`DocumentSemanticsAnalyzerPass::stats_handle`](rustsymbol-documentsemanticsanalyzerpass-stats-handle.md)
- `method` [`DocumentSemanticsAnalyzerPass::with_max_sections`](rustsymbol-documentsemanticsanalyzerpass-with-max-sections.md)
- `struct` [`DocumentSemanticsStats`](rustsymbol-documentsemanticsstats.md)
- `struct` [`LlmConcept`](rustsymbol-llmconcept.md)
- `struct` [`LlmOutput`](rustsymbol-llmoutput.md)
- `struct` [`LlmRelationship`](rustsymbol-llmrelationship-a8e764aa.md)
- `struct` [`SectionInput`](rustsymbol-sectioninput.md)
- `function` [`concept_kir_id`](rustsymbol-concept-kir-id.md)
- `function` [`normalize_concept_name`](rustsymbol-normalize-concept-name.md)
- `function` [`sections_from_graph`](rustsymbol-sections-from-graph.md)

## ekos/crates/recovery/src/git_analyzer.rs

- `struct` [`GitAnalyzerPass`](rustsymbol-gitanalyzerpass.md)
- `method` [`GitAnalyzerPass::cache_inputs`](rustsymbol-gitanalyzerpass-cache-inputs.md)
- `method` [`GitAnalyzerPass::name`](rustsymbol-gitanalyzerpass-name.md)
- `method` [`GitAnalyzerPass::new`](rustsymbol-gitanalyzerpass-new.md)
- `method` [`GitAnalyzerPass::run`](rustsymbol-gitanalyzerpass-run.md)
- `method` [`GitAnalyzerPass::version`](rustsymbol-gitanalyzerpass-version.md)
- `method` [`GitAnalyzerPass::with_max_coupling_commit_files`](rustsymbol-gitanalyzerpass-with-max-coupling-commit-files.md)
- `method` [`GitAnalyzerPass::with_min_coupling`](rustsymbol-gitanalyzerpass-with-min-coupling.md)
- `function` [`contributor_kir_id`](rustsymbol-contributor-kir-id.md)

## ekos/crates/recovery/src/github_analyzer.rs

- `struct` [`GitHubAnalyzerPass`](rustsymbol-githubanalyzerpass.md)
- `method` [`GitHubAnalyzerPass::cache_inputs`](rustsymbol-githubanalyzerpass-cache-inputs.md)
- `method` [`GitHubAnalyzerPass::name`](rustsymbol-githubanalyzerpass-name.md)
- `method` [`GitHubAnalyzerPass::new`](rustsymbol-githubanalyzerpass-new.md)
- `method` [`GitHubAnalyzerPass::run`](rustsymbol-githubanalyzerpass-run.md)
- `struct` [`ItemData`](rustsymbol-itemdata.md)
- `function` [`body_excerpt`](rustsymbol-body-excerpt-4f4ffc8a.md)
- `function` [`file_kir_id`](rustsymbol-file-kir-id-d36e01ce.md)
- `function` [`find_closed_issue_numbers`](rustsymbol-find-closed-issue-numbers.md)
- `function` [`item_kir_id`](rustsymbol-item-kir-id.md)

## ekos/crates/recovery/src/llm.rs

- `enum` [`LlmError`](rustsymbol-llmerror.md)
- `method` [`LlmError::other`](rustsymbol-llmerror-other.md)
- `trait` [`LlmProvider`](rustsymbol-llmprovider.md)
- `struct` [`LlmRequest`](rustsymbol-llmrequest.md)
- `struct` [`LlmResponse`](rustsymbol-llmresponse.md)
- `struct` [`MockLlmProvider`](rustsymbol-mockllmprovider.md)
- `method` [`MockLlmProvider::complete`](rustsymbol-mockllmprovider-complete.md)
- `method` [`MockLlmProvider::model_name`](rustsymbol-mockllmprovider-model-name.md)
- `method` [`MockLlmProvider::new`](rustsymbol-mockllmprovider-new.md)

## ekos/crates/recovery/src/llm_json.rs

- `function` [`strip_json_fences`](rustsymbol-strip-json-fences.md)

## ekos/crates/recovery/src/local_docs_analyzer.rs

- `struct` [`DocumentData`](rustsymbol-documentdata.md)
- `struct` [`LocalDocAnalyzerPass`](rustsymbol-localdocanalyzerpass.md)
- `method` [`LocalDocAnalyzerPass::cache_inputs`](rustsymbol-localdocanalyzerpass-cache-inputs.md)
- `method` [`LocalDocAnalyzerPass::name`](rustsymbol-localdocanalyzerpass-name.md)
- `method` [`LocalDocAnalyzerPass::new`](rustsymbol-localdocanalyzerpass-new.md)
- `method` [`LocalDocAnalyzerPass::run`](rustsymbol-localdocanalyzerpass-run.md)
- `struct` [`SectionData`](rustsymbol-sectiondata.md)
- `struct` [`TableData`](rustsymbol-tabledata.md)
- `function` [`document_kir_id`](rustsymbol-document-kir-id.md)
- `function` [`section_kir_id`](rustsymbol-section-kir-id.md)
- `function` [`table_kir_id`](rustsymbol-table-kir-id.md)

## ekos/crates/recovery/src/ollama.rs

- `struct` [`ApiMessage`](rustsymbol-apimessage-e405f66b.md)
- `struct` [`ApiOptions`](rustsymbol-apioptions.md)
- `struct` [`ApiRequest`](rustsymbol-apirequest.md)
- `struct` [`ApiResponse`](rustsymbol-apiresponse-57f09378.md)
- `struct` [`ApiResponseMessage`](rustsymbol-apiresponsemessage.md)
- `struct` [`OllamaProvider`](rustsymbol-ollamaprovider.md)
- `method` [`OllamaProvider::build_request`](rustsymbol-ollamaprovider-build-request.md)
- `method` [`OllamaProvider::complete`](rustsymbol-ollamaprovider-complete.md)
- `method` [`OllamaProvider::from_env`](rustsymbol-ollamaprovider-from-env.md)
- `method` [`OllamaProvider::model_name`](rustsymbol-ollamaprovider-model-name.md)
- `method` [`OllamaProvider::new`](rustsymbol-ollamaprovider-new.md)

## ekos/crates/recovery/src/pentaho_analyzer.rs

- `struct` [`PentahoAnalyzerPass`](rustsymbol-pentahoanalyzerpass.md)
- `method` [`PentahoAnalyzerPass::cache_inputs`](rustsymbol-pentahoanalyzerpass-cache-inputs.md)
- `method` [`PentahoAnalyzerPass::name`](rustsymbol-pentahoanalyzerpass-name.md)
- `method` [`PentahoAnalyzerPass::new`](rustsymbol-pentahoanalyzerpass-new.md)
- `method` [`PentahoAnalyzerPass::run`](rustsymbol-pentahoanalyzerpass-run.md)
- `method` [`PentahoAnalyzerPass::stats_handle`](rustsymbol-pentahoanalyzerpass-stats-handle.md)
- `struct` [`PentahoArtifactData`](rustsymbol-pentahoartifactdata.md)
- `struct` [`PentahoStats`](rustsymbol-pentahostats.md)
- `method` [`PentahoStats::coverage_percent`](rustsymbol-pentahostats-coverage-percent.md)
- `function` [`child_text`](rustsymbol-child-text.md)
- `function` [`extract_calculator`](rustsymbol-extract-calculator.md)
- `function` [`extract_filter_condition`](rustsymbol-extract-filter-condition.md)
- `function` [`extract_group_by`](rustsymbol-extract-group-by.md)
- `function` [`extract_join`](rustsymbol-extract-join.md)
- `function` [`extract_join_keys`](rustsymbol-extract-join-keys.md)
- `function` [`extract_stream_lookup`](rustsymbol-extract-stream-lookup.md)
- `function` [`extract_table_from_sql`](rustsymbol-extract-table-from-sql.md)
- `function` [`map_step`](rustsymbol-map-step.md)
- `function` [`parse_kettle_xml`](rustsymbol-parse-kettle-xml.md)
- `function` [`parse_kjb`](rustsymbol-parse-kjb.md)
- `function` [`parse_ktr`](rustsymbol-parse-ktr.md)
- `function` [`xml_slice`](rustsymbol-xml-slice.md)

## ekos/crates/recovery/src/python_analyzer.rs

- `struct` [`PythonAnalyzerPass`](rustsymbol-pythonanalyzerpass.md)
- `method` [`PythonAnalyzerPass::cache_inputs`](rustsymbol-pythonanalyzerpass-cache-inputs.md)
- `method` [`PythonAnalyzerPass::name`](rustsymbol-pythonanalyzerpass-name.md)
- `method` [`PythonAnalyzerPass::new`](rustsymbol-pythonanalyzerpass-new.md)
- `method` [`PythonAnalyzerPass::run`](rustsymbol-pythonanalyzerpass-run.md)
- `method` [`PythonAnalyzerPass::stats_handle`](rustsymbol-pythonanalyzerpass-stats-handle.md)
- `struct` [`PythonArtifactData`](rustsymbol-pythonartifactdata.md)
- `struct` [`PythonFileResult`](rustsymbol-pythonfileresult.md)
- `struct` [`PythonStats`](rustsymbol-pythonstats.md)
- `method` [`PythonStats::coverage_percent`](rustsymbol-pythonstats-coverage-percent.md)
- `struct` [`RawCall`](rustsymbol-rawcall.md)
- `function` [`add_import`](rustsymbol-add-import-89c6ca8d.md)
- `function` [`add_symbol`](rustsymbol-add-symbol-458e9ef2.md)
- `function` [`agg_expr_from_arg`](rustsymbol-agg-expr-from-arg.md)
- `function` [`calls_to_nodes`](rustsymbol-calls-to-nodes.md)
- `function` [`join_keys_from_on`](rustsymbol-join-keys-from-on.md)
- `function` [`join_kind_from_how`](rustsymbol-join-kind-from-how.md)
- `function` [`keyword_arg`](rustsymbol-keyword-arg.md)
- `function` [`linearize_chain`](rustsymbol-linearize-chain.md)
- `function` [`parse_python_file`](rustsymbol-parse-python-file.md)
- `function` [`positional_string_arg`](rustsymbol-positional-string-arg.md)
- `function` [`python_module_kir_id`](rustsymbol-python-module-kir-id.md)
- `function` [`source_slice`](rustsymbol-source-slice.md)
- `function` [`string_constant`](rustsymbol-string-constant.md)
- `function` [`try_recognize_chain_statement`](rustsymbol-try-recognize-chain-statement.md)
- `function` [`walk_top_level_statement`](rustsymbol-walk-top-level-statement.md)

## ekos/crates/recovery/src/rust_analyzer.rs

- `struct` [`CallVisitor`](rustsymbol-callvisitor.md)
- `method` [`CallVisitor::visit_expr_call`](rustsymbol-callvisitor-visit-expr-call.md)
- `method` [`CallVisitor::visit_expr_method_call`](rustsymbol-callvisitor-visit-expr-method-call.md)
- `struct` [`RustAnalyzerPass`](rustsymbol-rustanalyzerpass.md)
- `method` [`RustAnalyzerPass::cache_inputs`](rustsymbol-rustanalyzerpass-cache-inputs.md)
- `method` [`RustAnalyzerPass::name`](rustsymbol-rustanalyzerpass-name.md)
- `method` [`RustAnalyzerPass::new`](rustsymbol-rustanalyzerpass-new.md)
- `method` [`RustAnalyzerPass::run`](rustsymbol-rustanalyzerpass-run.md)
- `method` [`RustAnalyzerPass::stats_handle`](rustsymbol-rustanalyzerpass-stats-handle.md)
- `struct` [`RustArtifactData`](rustsymbol-rustartifactdata.md)
- `struct` [`RustFileResult`](rustsymbol-rustfileresult.md)
- `struct` [`RustStats`](rustsymbol-ruststats.md)
- `function` [`add_import`](rustsymbol-add-import.md)
- `function` [`add_symbol`](rustsymbol-add-symbol.md)
- `function` [`flatten_use_tree`](rustsymbol-flatten-use-tree.md)
- `function` [`parse_rust_file`](rustsymbol-parse-rust-file.md)
- `function` [`rust_module_kir_id`](rustsymbol-rust-module-kir-id.md)
- `function` [`type_name`](rustsymbol-type-name-b2c88510.md)

## ekos/crates/recovery/src/sql_analyzer.rs

- `struct` [`LlmEntity`](rustsymbol-llmentity.md)
- `struct` [`LlmOutput`](rustsymbol-llmoutput-771440cb.md)
- `struct` [`LlmRelationship`](rustsymbol-llmrelationship.md)
- `struct` [`SqlAnalyzerPass`](rustsymbol-sqlanalyzerpass.md)
- `method` [`SqlAnalyzerPass::cache_inputs`](rustsymbol-sqlanalyzerpass-cache-inputs.md)
- `method` [`SqlAnalyzerPass::name`](rustsymbol-sqlanalyzerpass-name.md)
- `method` [`SqlAnalyzerPass::new`](rustsymbol-sqlanalyzerpass-new.md)
- `method` [`SqlAnalyzerPass::run`](rustsymbol-sqlanalyzerpass-run.md)
- `function` [`add_fk_relationship`](rustsymbol-add-fk-relationship.md)
- `function` [`apply_llm_enrichment`](rustsymbol-apply-llm-enrichment.md)
- `function` [`col_names`](rustsymbol-col-names.md)
- `function` [`columns_json`](rustsymbol-columns-json.md)
- `function` [`parse_ddl_structural`](rustsymbol-parse-ddl-structural.md)

## ekos/crates/recovery/src/sql_dialect_registry.rs

- `struct` [`DialectRule`](rustsymbol-dialectrule.md)
- `struct` [`GenericDialectParser`](rustsymbol-genericdialectparser.md)
- `method` [`GenericDialectParser::name`](rustsymbol-genericdialectparser-name.md)
- `method` [`GenericDialectParser::sqlparser_dialect`](rustsymbol-genericdialectparser-sqlparser-dialect.md)
- `function` [`build_dialect_registry`](rustsymbol-build-dialect-registry.md)
- `function` [`resolve_dialect_name`](rustsymbol-resolve-dialect-name.md)

## ekos/crates/recovery/src/sql_transform_analyzer.rs

- `struct` [`SqlTransformAnalyzerPass`](rustsymbol-sqltransformanalyzerpass.md)
- `method` [`SqlTransformAnalyzerPass::cache_inputs`](rustsymbol-sqltransformanalyzerpass-cache-inputs.md)
- `method` [`SqlTransformAnalyzerPass::name`](rustsymbol-sqltransformanalyzerpass-name.md)
- `method` [`SqlTransformAnalyzerPass::new`](rustsymbol-sqltransformanalyzerpass-new.md)
- `method` [`SqlTransformAnalyzerPass::run`](rustsymbol-sqltransformanalyzerpass-run.md)
- `method` [`SqlTransformAnalyzerPass::stats_handle`](rustsymbol-sqltransformanalyzerpass-stats-handle.md)
- `struct` [`SqlTransformStats`](rustsymbol-sqltransformstats.md)
- `method` [`SqlTransformStats::coverage_percent`](rustsymbol-sqltransformstats-coverage-percent.md)
- `function` [`append_fragment`](rustsymbol-append-fragment.md)
- `function` [`as_aggregate_function`](rustsymbol-as-aggregate-function.md)
- `function` [`calculated_projection`](rustsymbol-calculated-projection.md)
- `function` [`collect_equi_keys`](rustsymbol-collect-equi-keys.md)
- `function` [`dispatch_one_statement`](rustsymbol-dispatch-one-statement.md)
- `function` [`extract_aggregates`](rustsymbol-extract-aggregates.md)
- `function` [`extract_equi_keys`](rustsymbol-extract-equi-keys.md)
- `function` [`function_body_text`](rustsymbol-function-body-text.md)
- `function` [`function_to_graph`](rustsymbol-function-to-graph.md)
- `function` [`is_plain_column`](rustsymbol-is-plain-column.md)
- `function` [`join_node`](rustsymbol-join-node.md)
- `function` [`parse_sql_statement_by_statement`](rustsymbol-parse-sql-statement-by-statement.md)
- `function` [`parse_sql_to_transform_graphs`](rustsymbol-parse-sql-to-transform-graphs.md)
- `function` [`procedure_body_to_graph`](rustsymbol-procedure-body-to-graph.md)
- `function` [`push`](rustsymbol-push.md)
- `function` [`query_to_graph`](rustsymbol-query-to-graph.md)
- `function` [`select_to_graph`](rustsymbol-select-to-graph.md)
- `function` [`source_kind_for`](rustsymbol-source-kind-for.md)
- `function` [`table_factor_node`](rustsymbol-table-factor-node.md)

## ekos/crates/recovery/src/statement_repair.rs

- `function` [`ends_with_set_op_keyword`](rustsymbol-ends-with-set-op-keyword.md)
- `function` [`ensure_statement_separators`](rustsymbol-ensure-statement-separators.md)
- `function` [`starts_with_keyword`](rustsymbol-starts-with-keyword.md)

## ekos/crates/runtime/src/ai.rs

- `struct` [`AiAnswer`](rustsymbol-aianswer.md)
- `enum` [`AiError`](rustsymbol-aierror.md)
- `struct` [`AiRuntime`](rustsymbol-airuntime.md)
- `method` [`AiRuntime::ask`](rustsymbol-airuntime-ask.md)
- `method` [`AiRuntime::gather_context`](rustsymbol-airuntime-gather-context.md)
- `method` [`AiRuntime::new`](rustsymbol-airuntime-new.md)
- `struct` [`AiRuntimeConfig`](rustsymbol-airuntimeconfig.md)
- `method` [`AiRuntimeConfig::default`](rustsymbol-airuntimeconfig-default.md)
- `struct` [`CitationBlock`](rustsymbol-citationblock.md)
- `function` [`extract_citations`](rustsymbol-extract-citations.md)

## ekos/crates/runtime/src/lib.rs

- `enum` [`ImpactDirection`](rustsymbol-impactdirection.md)
- `struct` [`ImpactHop`](rustsymbol-impacthop.md)
- `struct` [`ObjectState`](rustsymbol-objectstate.md)
- `struct` [`Runtime`](rustsymbol-runtime.md)
- `method` [`Runtime::find_objects`](rustsymbol-runtime-find-objects.md)
- `method` [`Runtime::list_objects`](rustsymbol-runtime-list-objects.md)
- `method` [`Runtime::list_relationships`](rustsymbol-runtime-list-relationships.md)
- `method` [`Runtime::load_neighborhood`](rustsymbol-runtime-load-neighborhood.md)
- `method` [`Runtime::load_object`](rustsymbol-runtime-load-object.md)
- `method` [`Runtime::new`](rustsymbol-runtime-new.md)
- `method` [`Runtime::over`](rustsymbol-runtime-over.md)
- `method` [`Runtime::reconstruct_state`](rustsymbol-runtime-reconstruct-state.md)
- `method` [`Runtime::reconstruct_state_at`](rustsymbol-runtime-reconstruct-state-at.md)
- `method` [`Runtime::relationships_for`](rustsymbol-runtime-relationships-for.md)
- `method` [`Runtime::trace_impact`](rustsymbol-runtime-trace-impact.md)
- `enum` [`RuntimeError`](rustsymbol-runtimeerror.md)

## ekos/crates/semantic/src/lib.rs

- `struct` [`CkModel`](rustsymbol-ckmodel.md)
- `method` [`CkModel::validate`](rustsymbol-ckmodel-validate.md)
- `struct` [`CkmObject`](rustsymbol-ckmobject.md)
- `struct` [`CkmRelationship`](rustsymbol-ckmrelationship.md)
- `struct` [`EvidenceRecord`](rustsymbol-evidencerecord-dba444b3.md)
- `struct` [`SemanticCompilerPass`](rustsymbol-semanticcompilerpass.md)
- `method` [`SemanticCompilerPass::cache_inputs`](rustsymbol-semanticcompilerpass-cache-inputs.md)
- `method` [`SemanticCompilerPass::name`](rustsymbol-semanticcompilerpass-name.md)
- `method` [`SemanticCompilerPass::new`](rustsymbol-semanticcompilerpass-new.md)
- `method` [`SemanticCompilerPass::run`](rustsymbol-semanticcompilerpass-run.md)
- `method` [`SemanticCompilerPass::with_cache_inputs`](rustsymbol-semanticcompilerpass-with-cache-inputs.md)
- `function` [`apply_merges`](rustsymbol-apply-merges.md)
- `function` [`build_ckm`](rustsymbol-build-ckm.md)
- `function` [`dedup_relationships`](rustsymbol-dedup-relationships.md)
- `function` [`merge_graphs`](rustsymbol-merge-graphs.md)

## ekos/crates/semantic/src/transform_ir.rs

- `struct` [`AggExpr`](rustsymbol-aggexpr.md)
- `enum` [`JoinKind`](rustsymbol-joinkind.md)
- `struct` [`NodeId`](rustsymbol-nodeid.md)
- `struct` [`TransformGraph`](rustsymbol-transformgraph.md)
- `enum` [`TransformNode`](rustsymbol-transformnode.md)
- `method` [`TransformNode::evidence_fragment`](rustsymbol-transformnode-evidence-fragment.md)
- `method` [`TransformNode::node_type`](rustsymbol-transformnode-node-type.md)
- `method` [`TransformNode::properties`](rustsymbol-transformnode-properties.md)
- `struct` [`TransformOrigin`](rustsymbol-transformorigin.md)
- `function` [`lower_to_kir`](rustsymbol-lower-to-kir.md)
- `function` [`transform_evidence_kir_id`](rustsymbol-transform-evidence-kir-id.md)
- `function` [`transform_node_kir_id`](rustsymbol-transform-node-kir-id.md)

## ekos/crates/sql-dialect-sdk/src/lib.rs

- `trait` [`SqlDialectParser`](rustsymbol-sqldialectparser.md)

## ekos/plugins/confluence/src/lib.rs

- `struct` [`ConfluenceApiClient`](rustsymbol-confluenceapiclient.md)
- `method` [`ConfluenceApiClient::list_pages`](rustsymbol-confluenceapiclient-list-pages.md)
- `method` [`ConfluenceApiClient::new`](rustsymbol-confluenceapiclient-new.md)
- `method` [`ConfluenceApiClient::request`](rustsymbol-confluenceapiclient-request.md)
- `trait` [`ConfluenceClient`](rustsymbol-confluenceclient.md)
- `enum` [`ConfluenceClientError`](rustsymbol-confluenceclienterror.md)
- `struct` [`ConfluenceObserver`](rustsymbol-confluenceobserver.md)
- `method` [`ConfluenceObserver::name`](rustsymbol-confluenceobserver-name.md)
- `method` [`ConfluenceObserver::new`](rustsymbol-confluenceobserver-new.md)
- `method` [`ConfluenceObserver::scan`](rustsymbol-confluenceobserver-scan.md)
- `struct` [`ConfluencePage`](rustsymbol-confluencepage.md)
- `struct` [`MockConfluenceClient`](rustsymbol-mockconfluenceclient.md)
- `method` [`MockConfluenceClient::list_pages`](rustsymbol-mockconfluenceclient-list-pages.md)
- `method` [`MockConfluenceClient::new`](rustsymbol-mockconfluenceclient-new.md)

## ekos/plugins/crypto/src/lib.rs

- `trait` [`CryptoExportReader`](rustsymbol-cryptoexportreader.md)
- `struct` [`CryptoObserver`](rustsymbol-cryptoobserver.md)
- `method` [`CryptoObserver::name`](rustsymbol-cryptoobserver-name.md)
- `method` [`CryptoObserver::new`](rustsymbol-cryptoobserver-new.md)
- `method` [`CryptoObserver::scan`](rustsymbol-cryptoobserver-scan.md)
- `enum` [`CryptoReaderError`](rustsymbol-cryptoreadererror.md)
- `struct` [`EntityRecord`](rustsymbol-entityrecord.md)
- `struct` [`EvidenceRecord`](rustsymbol-evidencerecord.md)
- `struct` [`ExportBatch`](rustsymbol-exportbatch.md)
- `struct` [`MockCryptoExportReader`](rustsymbol-mockcryptoexportreader.md)
- `method` [`MockCryptoExportReader::new`](rustsymbol-mockcryptoexportreader-new.md)
- `method` [`MockCryptoExportReader::read_latest_batch`](rustsymbol-mockcryptoexportreader-read-latest-batch.md)
- `struct` [`ParquetExportReader`](rustsymbol-parquetexportreader.md)
- `method` [`ParquetExportReader::latest_batch_dir`](rustsymbol-parquetexportreader-latest-batch-dir.md)
- `method` [`ParquetExportReader::read_entities`](rustsymbol-parquetexportreader-read-entities.md)
- `method` [`ParquetExportReader::read_evidence`](rustsymbol-parquetexportreader-read-evidence.md)
- `method` [`ParquetExportReader::read_latest_batch`](rustsymbol-parquetexportreader-read-latest-batch.md)
- `method` [`ParquetExportReader::read_relationships`](rustsymbol-parquetexportreader-read-relationships.md)
- `struct` [`RelationshipRecord`](rustsymbol-relationshiprecord.md)
- `function` [`get_string`](rustsymbol-get-string.md)
- `function` [`get_string_list`](rustsymbol-get-string-list.md)
- `function` [`read_rows`](rustsymbol-read-rows.md)

## ekos/plugins/fabric/src/lib.rs

- `struct` [`FabricApiClient`](rustsymbol-fabricapiclient.md)
- `method` [`FabricApiClient::items_for_workspace`](rustsymbol-fabricapiclient-items-for-workspace.md)
- `method` [`FabricApiClient::list_items`](rustsymbol-fabricapiclient-list-items.md)
- `method` [`FabricApiClient::new`](rustsymbol-fabricapiclient-new.md)
- `trait` [`FabricClient`](rustsymbol-fabricclient.md)
- `enum` [`FabricClientError`](rustsymbol-fabricclienterror.md)
- `struct` [`FabricItem`](rustsymbol-fabricitem.md)
- `struct` [`FabricObserver`](rustsymbol-fabricobserver.md)
- `method` [`FabricObserver::name`](rustsymbol-fabricobserver-name.md)
- `method` [`FabricObserver::new`](rustsymbol-fabricobserver-new.md)
- `method` [`FabricObserver::scan`](rustsymbol-fabricobserver-scan.md)
- `struct` [`MockFabricClient`](rustsymbol-mockfabricclient.md)
- `method` [`MockFabricClient::list_items`](rustsymbol-mockfabricclient-list-items.md)
- `method` [`MockFabricClient::new`](rustsymbol-mockfabricclient-new.md)

## ekos/plugins/file/src/lib.rs

- `struct` [`FileObserver`](rustsymbol-fileobserver.md)
- `method` [`FileObserver::default`](rustsymbol-fileobserver-default.md)
- `method` [`FileObserver::name`](rustsymbol-fileobserver-name.md)
- `method` [`FileObserver::new`](rustsymbol-fileobserver-new.md)
- `method` [`FileObserver::scan`](rustsymbol-fileobserver-scan.md)
- `function` [`harvest_symbols`](rustsymbol-harvest-symbols.md)
- `function` [`text_excerpt`](rustsymbol-text-excerpt.md)

## ekos/plugins/git/src/lib.rs

- `struct` [`GitObserver`](rustsymbol-gitobserver.md)
- `method` [`GitObserver::default`](rustsymbol-gitobserver-default.md)
- `method` [`GitObserver::name`](rustsymbol-gitobserver-name.md)
- `method` [`GitObserver::new`](rustsymbol-gitobserver-new.md)
- `method` [`GitObserver::scan`](rustsymbol-gitobserver-scan.md)
- `method` [`GitObserver::with_max_commits`](rustsymbol-gitobserver-with-max-commits.md)
- `function` [`git_output`](rustsymbol-git-output.md)
- `function` [`is_git_repo`](rustsymbol-is-git-repo.md)
- `function` [`parse_stat_summary`](rustsymbol-parse-stat-summary.md)

## ekos/plugins/github/src/lib.rs

- `struct` [`GitHubApiClient`](rustsymbol-githubapiclient.md)
- `method` [`GitHubApiClient::list_files`](rustsymbol-githubapiclient-list-files.md)
- `method` [`GitHubApiClient::list_items`](rustsymbol-githubapiclient-list-items.md)
- `method` [`GitHubApiClient::new`](rustsymbol-githubapiclient-new.md)
- `method` [`GitHubApiClient::request`](rustsymbol-githubapiclient-request.md)
- `trait` [`GitHubClient`](rustsymbol-githubclient.md)
- `enum` [`GitHubClientError`](rustsymbol-githubclienterror.md)
- `struct` [`GitHubItem`](rustsymbol-githubitem.md)
- `struct` [`GitHubObserver`](rustsymbol-githubobserver.md)
- `method` [`GitHubObserver::name`](rustsymbol-githubobserver-name.md)
- `method` [`GitHubObserver::new`](rustsymbol-githubobserver-new.md)
- `method` [`GitHubObserver::scan`](rustsymbol-githubobserver-scan.md)
- `struct` [`MockGitHubClient`](rustsymbol-mockgithubclient.md)
- `method` [`MockGitHubClient::list_items`](rustsymbol-mockgithubclient-list-items.md)
- `method` [`MockGitHubClient::new`](rustsymbol-mockgithubclient-new.md)

## ekos/plugins/localdocs/src/docx.rs

- `struct` [`DocxParser`](rustsymbol-docxparser.md)
- `method` [`DocxParser::parse`](rustsymbol-docxparser-parse.md)
- `method` [`DocxParser::supported_extension`](rustsymbol-docxparser-supported-extension.md)
- `function` [`extract_media_images`](rustsymbol-extract-media-images.md)
- `function` [`paragraph_text`](rustsymbol-paragraph-text.md)
- `function` [`table_rows`](rustsymbol-table-rows.md)

## ekos/plugins/localdocs/src/email.rs

- `struct` [`EmailParser`](rustsymbol-emailparser.md)
- `method` [`EmailParser::parse`](rustsymbol-emailparser-parse.md)
- `method` [`EmailParser::supported_extension`](rustsymbol-emailparser-supported-extension.md)
- `function` [`body_text`](rustsymbol-body-text.md)
- `function` [`header_block`](rustsymbol-header-block.md)
- `function` [`render_address`](rustsymbol-render-address.md)

## ekos/plugins/localdocs/src/html.rs

- `struct` [`HtmlParser`](rustsymbol-htmlparser.md)
- `method` [`HtmlParser::new`](rustsymbol-htmlparser-new.md)
- `method` [`HtmlParser::parse`](rustsymbol-htmlparser-parse.md)
- `method` [`HtmlParser::supported_extension`](rustsymbol-htmlparser-supported-extension.md)
- `function` [`html_to_text`](rustsymbol-html-to-text.md)

## ekos/plugins/localdocs/src/lib.rs

- `trait` [`DocumentParser`](rustsymbol-documentparser.md)
- `struct` [`DocumentSection`](rustsymbol-documentsection.md)
- `struct` [`EmbeddedImage`](rustsymbol-embeddedimage.md)
- `struct` [`ExtractedTable`](rustsymbol-extractedtable.md)
- `enum` [`ImageFormat`](rustsymbol-imageformat.md)
- `struct` [`LocalDocsObserver`](rustsymbol-localdocsobserver.md)
- `method` [`LocalDocsObserver::name`](rustsymbol-localdocsobserver-name.md)
- `method` [`LocalDocsObserver::new`](rustsymbol-localdocsobserver-new.md)
- `method` [`LocalDocsObserver::parser_for`](rustsymbol-localdocsobserver-parser-for.md)
- `method` [`LocalDocsObserver::scan`](rustsymbol-localdocsobserver-scan.md)
- `method` [`LocalDocsObserver::with_defaults`](rustsymbol-localdocsobserver-with-defaults.md)
- `trait` [`OcrEngine`](rustsymbol-ocrengine.md)
- `enum` [`OcrError`](rustsymbol-ocrerror.md)
- `enum` [`ParseError`](rustsymbol-parseerror-cfecf937.md)
- `struct` [`ParsedDocument`](rustsymbol-parseddocument.md)

## ekos/plugins/localdocs/src/ocr.rs

- `struct` [`MockOcr`](rustsymbol-mockocr.md)
- `method` [`MockOcr::new`](rustsymbol-mockocr-new.md)
- `method` [`MockOcr::recognize`](rustsymbol-mockocr-recognize.md)
- `struct` [`TesseractOcr`](rustsymbol-tesseractocr.md)
- `method` [`TesseractOcr::recognize`](rustsymbol-tesseractocr-recognize.md)

## ekos/plugins/localdocs/src/pdf.rs

- `struct` [`PdfParser`](rustsymbol-pdfparser.md)
- `method` [`PdfParser::parse`](rustsymbol-pdfparser-parse.md)
- `method` [`PdfParser::parse_inner`](rustsymbol-pdfparser-parse-inner.md)
- `method` [`PdfParser::supported_extension`](rustsymbol-pdfparser-supported-extension.md)
- `function` [`extract_sections`](rustsymbol-extract-sections.md)
- `function` [`extract_tables`](rustsymbol-extract-tables.md)
- `function` [`has_uniform_column_count`](rustsymbol-has-uniform-column-count.md)
- `function` [`split_table_row`](rustsymbol-split-table-row.md)

## ekos/plugins/localdocs/src/sanitize.rs

- `struct` [`Sanitized`](rustsymbol-sanitized.md)
- `function` [`is_sanitized_char`](rustsymbol-is-sanitized-char.md)
- `function` [`sanitize_text`](rustsymbol-sanitize-text.md)

## ekos/plugins/localdocs/src/text.rs

- `struct` [`TextParser`](rustsymbol-textparser.md)
- `method` [`TextParser::new`](rustsymbol-textparser-new.md)
- `method` [`TextParser::parse`](rustsymbol-textparser-parse.md)
- `method` [`TextParser::supported_extension`](rustsymbol-textparser-supported-extension.md)
- `function` [`chunk_text`](rustsymbol-chunk-text.md)
- `function` [`split_to_budget`](rustsymbol-split-to-budget.md)

## ekos/plugins/oracle/src/lib.rs

- `struct` [`ColumnMetadata`](rustsymbol-columnmetadata.md)
- `struct` [`ConstraintMetadata`](rustsymbol-constraintmetadata.md)
- `struct` [`MockOracleClient`](rustsymbol-mockoracleclient.md)
- `method` [`MockOracleClient::list_constraints`](rustsymbol-mockoracleclient-list-constraints.md)
- `method` [`MockOracleClient::list_tables`](rustsymbol-mockoracleclient-list-tables.md)
- `method` [`MockOracleClient::list_views`](rustsymbol-mockoracleclient-list-views.md)
- `method` [`MockOracleClient::new`](rustsymbol-mockoracleclient-new.md)
- `trait` [`OracleClient`](rustsymbol-oracleclient.md)
- `enum` [`OracleClientError`](rustsymbol-oracleclienterror.md)
- `struct` [`OracleDbClient`](rustsymbol-oracledbclient.md)
- `method` [`OracleDbClient::list_constraints`](rustsymbol-oracledbclient-list-constraints.md)
- `method` [`OracleDbClient::list_tables`](rustsymbol-oracledbclient-list-tables.md)
- `method` [`OracleDbClient::list_views`](rustsymbol-oracledbclient-list-views.md)
- `method` [`OracleDbClient::new`](rustsymbol-oracledbclient-new.md)
- `struct` [`OracleObserver`](rustsymbol-oracleobserver.md)
- `method` [`OracleObserver::name`](rustsymbol-oracleobserver-name.md)
- `method` [`OracleObserver::new`](rustsymbol-oracleobserver-new.md)
- `method` [`OracleObserver::scan`](rustsymbol-oracleobserver-scan.md)
- `struct` [`TableMetadata`](rustsymbol-tablemetadata.md)
- `struct` [`ViewMetadata`](rustsymbol-viewmetadata.md)

## ekos/plugins/pentaho/src/lib.rs

- `struct` [`PentahoObserver`](rustsymbol-pentahoobserver.md)
- `method` [`PentahoObserver::name`](rustsymbol-pentahoobserver-name.md)
- `method` [`PentahoObserver::new`](rustsymbol-pentahoobserver-new.md)
- `method` [`PentahoObserver::scan`](rustsymbol-pentahoobserver-scan.md)
- `function` [`kettle_kind`](rustsymbol-kettle-kind.md)

## ekos/plugins/python/src/lib.rs

- `struct` [`PythonObserver`](rustsymbol-pythonobserver.md)
- `method` [`PythonObserver::name`](rustsymbol-pythonobserver-name.md)
- `method` [`PythonObserver::new`](rustsymbol-pythonobserver-new.md)
- `method` [`PythonObserver::scan`](rustsymbol-pythonobserver-scan.md)

## ekos/plugins/rust/src/lib.rs

- `struct` [`RustObserver`](rustsymbol-rustobserver.md)
- `method` [`RustObserver::name`](rustsymbol-rustobserver-name.md)
- `method` [`RustObserver::new`](rustsymbol-rustobserver-new.md)
- `method` [`RustObserver::scan`](rustsymbol-rustobserver-scan.md)

## ekos/plugins/salesforce/src/lib.rs

- `struct` [`MockSalesforceClient`](rustsymbol-mocksalesforceclient.md)
- `method` [`MockSalesforceClient::list_sobjects`](rustsymbol-mocksalesforceclient-list-sobjects.md)
- `method` [`MockSalesforceClient::new`](rustsymbol-mocksalesforceclient-new.md)
- `struct` [`SObjectField`](rustsymbol-sobjectfield.md)
- `struct` [`SObjectMetadata`](rustsymbol-sobjectmetadata.md)
- `struct` [`SalesforceApiClient`](rustsymbol-salesforceapiclient.md)
- `method` [`SalesforceApiClient::describe`](rustsymbol-salesforceapiclient-describe.md)
- `method` [`SalesforceApiClient::list_sobjects`](rustsymbol-salesforceapiclient-list-sobjects.md)
- `method` [`SalesforceApiClient::new`](rustsymbol-salesforceapiclient-new.md)
- `trait` [`SalesforceClient`](rustsymbol-salesforceclient.md)
- `enum` [`SalesforceClientError`](rustsymbol-salesforceclienterror.md)
- `struct` [`SalesforceObserver`](rustsymbol-salesforceobserver.md)
- `method` [`SalesforceObserver::name`](rustsymbol-salesforceobserver-name.md)
- `method` [`SalesforceObserver::new`](rustsymbol-salesforceobserver-new.md)
- `method` [`SalesforceObserver::scan`](rustsymbol-salesforceobserver-scan.md)

## ekos/plugins/sap/src/lib.rs

- `struct` [`BusinessObject`](rustsymbol-businessobject.md)
- `struct` [`MockSapClient`](rustsymbol-mocksapclient.md)
- `method` [`MockSapClient::list_business_objects`](rustsymbol-mocksapclient-list-business-objects.md)
- `method` [`MockSapClient::list_organizational_units`](rustsymbol-mocksapclient-list-organizational-units.md)
- `method` [`MockSapClient::new`](rustsymbol-mocksapclient-new.md)
- `struct` [`OrganizationalUnit`](rustsymbol-organizationalunit.md)
- `trait` [`SapClient`](rustsymbol-sapclient.md)
- `enum` [`SapClientError`](rustsymbol-sapclienterror.md)
- `struct` [`SapODataClient`](rustsymbol-sapodataclient.md)
- `method` [`SapODataClient::get_json`](rustsymbol-sapodataclient-get-json.md)
- `method` [`SapODataClient::list_business_objects`](rustsymbol-sapodataclient-list-business-objects.md)
- `method` [`SapODataClient::list_organizational_units`](rustsymbol-sapodataclient-list-organizational-units.md)
- `method` [`SapODataClient::new`](rustsymbol-sapodataclient-new.md)
- `struct` [`SapObserver`](rustsymbol-sapobserver.md)
- `method` [`SapObserver::name`](rustsymbol-sapobserver-name.md)
- `method` [`SapObserver::new`](rustsymbol-sapobserver-new.md)
- `method` [`SapObserver::scan`](rustsymbol-sapobserver-scan.md)

## ekos/plugins/snowflake/src/lib.rs

- `struct` [`MockSnowflakeClient`](rustsymbol-mocksnowflakeclient.md)
- `method` [`MockSnowflakeClient::list_schema_objects`](rustsymbol-mocksnowflakeclient-list-schema-objects.md)
- `method` [`MockSnowflakeClient::new`](rustsymbol-mocksnowflakeclient-new.md)
- `struct` [`SchemaObject`](rustsymbol-schemaobject.md)
- `struct` [`SnowflakeApiClient`](rustsymbol-snowflakeapiclient.md)
- `method` [`SnowflakeApiClient::list_schema_objects`](rustsymbol-snowflakeapiclient-list-schema-objects.md)
- `method` [`SnowflakeApiClient::new`](rustsymbol-snowflakeapiclient-new.md)
- `method` [`SnowflakeApiClient::run_statement`](rustsymbol-snowflakeapiclient-run-statement.md)
- `trait` [`SnowflakeClient`](rustsymbol-snowflakeclient.md)
- `enum` [`SnowflakeClientError`](rustsymbol-snowflakeclienterror.md)
- `struct` [`SnowflakeObserver`](rustsymbol-snowflakeobserver.md)
- `method` [`SnowflakeObserver::name`](rustsymbol-snowflakeobserver-name.md)
- `method` [`SnowflakeObserver::new`](rustsymbol-snowflakeobserver-new.md)
- `method` [`SnowflakeObserver::scan`](rustsymbol-snowflakeobserver-scan.md)

## ekos/plugins/sql-dialect-databricks/src/lib.rs

- `struct` [`DatabricksDialectParser`](rustsymbol-databricksdialectparser.md)
- `method` [`DatabricksDialectParser::name`](rustsymbol-databricksdialectparser-name.md)
- `method` [`DatabricksDialectParser::sqlparser_dialect`](rustsymbol-databricksdialectparser-sqlparser-dialect.md)

## ekos/plugins/sql-dialect-mssql/src/lib.rs

- `struct` [`MsSqlDialectParser`](rustsymbol-mssqldialectparser.md)
- `method` [`MsSqlDialectParser::name`](rustsymbol-mssqldialectparser-name.md)
- `method` [`MsSqlDialectParser::new`](rustsymbol-mssqldialectparser-new.md)
- `method` [`MsSqlDialectParser::sqlparser_dialect`](rustsymbol-mssqldialectparser-sqlparser-dialect.md)

## ekos/plugins/sql-dialect-mysql/src/lib.rs

- `struct` [`MySqlDialectParser`](rustsymbol-mysqldialectparser.md)
- `method` [`MySqlDialectParser::name`](rustsymbol-mysqldialectparser-name.md)
- `method` [`MySqlDialectParser::preprocess`](rustsymbol-mysqldialectparser-preprocess.md)
- `method` [`MySqlDialectParser::sqlparser_dialect`](rustsymbol-mysqldialectparser-sqlparser-dialect.md)
- `function` [`strip_delimiter_directives`](rustsymbol-strip-delimiter-directives.md)

## ekos/plugins/sql-dialect-postgres/src/lib.rs

- `struct` [`PostgresDialectParser`](rustsymbol-postgresdialectparser.md)
- `method` [`PostgresDialectParser::name`](rustsymbol-postgresdialectparser-name.md)
- `method` [`PostgresDialectParser::sqlparser_dialect`](rustsymbol-postgresdialectparser-sqlparser-dialect.md)

## ekos/plugins/sql-dialect-snowflake/src/lib.rs

- `struct` [`SnowflakeDialectParser`](rustsymbol-snowflakedialectparser.md)
- `method` [`SnowflakeDialectParser::name`](rustsymbol-snowflakedialectparser-name.md)
- `method` [`SnowflakeDialectParser::sqlparser_dialect`](rustsymbol-snowflakedialectparser-sqlparser-dialect.md)

## tests/fixtures/sample_project/src/lib.rs

- `function` [`add`](rustsymbol-add.md)

## tests/fixtures/sample_project/src/main.rs

- `function` [`main`](rustsymbol-main.md)

## tests/integration/tests/integration.rs

- `function` [`copy_dir`](rustsymbol-copy-dir-7496161f.md)
- `function` [`ecommerce_pipeline_end_to_end`](rustsymbol-ecommerce-pipeline-end-to-end.md)
- `function` [`fixtures_dir`](rustsymbol-fixtures-dir.md)
- `function` [`northwind_pipeline_end_to_end`](rustsymbol-northwind-pipeline-end-to-end.md)
- `function` [`odoo_git_fixture_pipeline_end_to_end`](rustsymbol-odoo-git-fixture-pipeline-end-to-end.md)
- `function` [`run_pipeline`](rustsymbol-run-pipeline.md)
- `function` [`table_count`](rustsymbol-table-count.md)

