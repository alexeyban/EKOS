# API

_Program entities (functions, structs, enums, traits, classes, …) compiled from real Rust/Python source analysis, grouped by containing file. Each entity links to its own detail page (relationships, evidence, 1-hop diagram), written alongside this file. Real `Api`/`Service` objects, if a future connector ever compiles them, would render here directly._

## benchmark/benches/fact_ledger.rs

- `function` [`bench_fact_ledger`](entities/rustsymbol/be/bench-fact-ledger.md)
- `function` [`object`](entities/rustsymbol/ob/object.md)

## benchmark/benches/fact_model.rs

- `function` [`bench_fact_model`](entities/rustsymbol/be/bench-fact-model.md)
- `function` [`realistic_object`](entities/rustsymbol/re/realistic-object-0dcf9d8f.md)

## benchmark/benches/identity_resolver.rs

- `function` [`bench_identity_resolver`](entities/rustsymbol/be/bench-identity-resolver.md)
- `function` [`fixture_graph`](entities/rustsymbol/fi/fixture-graph-e3802af0.md)

## benchmark/benches/index_runs.rs

- `function` [`bench_index_runs`](entities/rustsymbol/be/bench-index-runs.md)
- `function` [`build_indexes`](entities/rustsymbol/bu/build-indexes.md)

## benchmark/benches/ledger_write.rs

- `function` [`bench_ledger_write`](entities/rustsymbol/be/bench-ledger-write.md)

## benchmark/benches/observation_git.rs

- `function` [`bench_observation_git`](entities/rustsymbol/be/bench-observation-git.md)
- `function` [`fixture_repo`](entities/rustsymbol/fi/fixture-repo.md)

## benchmark/benches/runtime_load_neighborhood.rs

- `function` [`bench_load_neighborhood`](entities/rustsymbol/be/bench-load-neighborhood.md)
- `function` [`seed_ledger`](entities/rustsymbol/se/seed-ledger.md)

## benchmark/benches/segment_store.rs

- `function` [`bench_segment_store`](entities/rustsymbol/be/bench-segment-store.md)
- `function` [`ops`](entities/rustsymbol/op/ops.md)

## benchmark/benches/semantic_compiler.rs

- `function` [`bench_semantic_compiler`](entities/rustsymbol/be/bench-semantic-compiler.md)
- `function` [`fixture_graph`](entities/rustsymbol/fi/fixture-graph.md)

## benchmark/benches/sql_analyzer.rs

- `function` [`bench_sql_analyzer`](entities/rustsymbol/be/bench-sql-analyzer.md)

## benchmark/benches/storage_compaction.rs

- `function` [`bench_storage`](entities/rustsymbol/be/bench-storage.md)
- `function` [`ledger_file_bytes`](entities/rustsymbol/le/ledger-file-bytes.md)
- `function` [`populated_ledger`](entities/rustsymbol/po/populated-ledger.md)
- `function` [`realistic_object`](entities/rustsymbol/re/realistic-object.md)

## docs/spikes/recovery_spike.py

- `function` [`call_claude`](entities/pythonsymbol/ca/call-claude.md)
- `function` [`evaluate`](entities/pythonsymbol/ev/evaluate.md)
- `function` [`main`](entities/pythonsymbol/ma/main.md)

## ekos/crates/artifact/src/lib.rs

- `struct` [`ArtifactId`](entities/rustsymbol/ar/artifactid.md)
- `method` [`ArtifactId::as_str`](entities/rustsymbol/ar/artifactid-as-str.md)
- `method` [`ArtifactId::compute`](entities/rustsymbol/ar/artifactid-compute.md)
- `method` [`ArtifactId::fmt`](entities/rustsymbol/ar/artifactid-fmt.md)
- `method` [`ArtifactId::prefix`](entities/rustsymbol/ar/artifactid-prefix.md)
- `struct` [`ArtifactMeta`](entities/rustsymbol/ar/artifactmeta.md)
- `method` [`ArtifactMeta::default`](entities/rustsymbol/ar/artifactmeta-default.md)
- `method` [`ArtifactMeta::new`](entities/rustsymbol/ar/artifactmeta-new.md)
- `enum` [`ArtifactType`](entities/rustsymbol/ar/artifacttype.md)
- `struct` [`DiagnosticArtifact`](entities/rustsymbol/di/diagnosticartifact.md)
- `method` [`DiagnosticArtifact::new`](entities/rustsymbol/di/diagnosticartifact-new.md)
- `struct` [`DiagnosticContent`](entities/rustsymbol/di/diagnosticcontent.md)
- `struct` [`DiagnosticRecord`](entities/rustsymbol/di/diagnosticrecord.md)
- `struct` [`EvidenceArtifact`](entities/rustsymbol/ev/evidenceartifact.md)
- `method` [`EvidenceArtifact::new`](entities/rustsymbol/ev/evidenceartifact-new.md)
- `struct` [`EvidenceContent`](entities/rustsymbol/ev/evidencecontent.md)
- `struct` [`IndexArtifact`](entities/rustsymbol/in/indexartifact.md)
- `method` [`IndexArtifact::new`](entities/rustsymbol/in/indexartifact-new.md)
- `struct` [`IndexContent`](entities/rustsymbol/in/indexcontent.md)
- `struct` [`KnowledgeArtifact`](entities/rustsymbol/kn/knowledgeartifact.md)
- `method` [`KnowledgeArtifact::new`](entities/rustsymbol/kn/knowledgeartifact-new.md)
- `struct` [`KnowledgeContent`](entities/rustsymbol/kn/knowledgecontent.md)
- `struct` [`ObservationArtifact`](entities/rustsymbol/ob/observationartifact.md)
- `method` [`ObservationArtifact::new`](entities/rustsymbol/ob/observationartifact-new.md)
- `method` [`ObservationArtifact::with_producer`](entities/rustsymbol/ob/observationartifact-with-producer.md)
- `struct` [`ObservationContent`](entities/rustsymbol/ob/observationcontent.md)
- `function` [`canonicalize`](entities/rustsymbol/ca/canonicalize.md)
- `function` [`compute_content_id`](entities/rustsymbol/co/compute-content-id.md)

## ekos/crates/artifact/src/pack.rs

- `struct` [`FrameLoc`](entities/rustsymbol/fr/frameloc.md)
- `struct` [`PackArtifactStore`](entities/rustsymbol/pa/packartifactstore.md)
- `method` [`PackArtifactStore::drop`](entities/rustsymbol/pa/packartifactstore-drop.md)
- `method` [`PackArtifactStore::exists`](entities/rustsymbol/pa/packartifactstore-exists.md)
- `method` [`PackArtifactStore::list`](entities/rustsymbol/pa/packartifactstore-list.md)
- `method` [`PackArtifactStore::loose_path`](entities/rustsymbol/pa/packartifactstore-loose-path.md)
- `method` [`PackArtifactStore::open`](entities/rustsymbol/pa/packartifactstore-open.md)
- `method` [`PackArtifactStore::packed_count`](entities/rustsymbol/pa/packartifactstore-packed-count.md)
- `method` [`PackArtifactStore::read`](entities/rustsymbol/pa/packartifactstore-read.md)
- `method` [`PackArtifactStore::repack_loose`](entities/rustsymbol/pa/packartifactstore-repack-loose.md)
- `method` [`PackArtifactStore::segment_path`](entities/rustsymbol/pa/packartifactstore-segment-path.md)
- `method` [`PackArtifactStore::sync`](entities/rustsymbol/pa/packartifactstore-sync.md)
- `method` [`PackArtifactStore::write`](entities/rustsymbol/pa/packartifactstore-write.md)
- `method` [`PackArtifactStore::write_packed`](entities/rustsymbol/pa/packartifactstore-write-packed.md)
- `struct` [`PackInner`](entities/rustsymbol/pa/packinner.md)
- `function` [`compress_frame_body`](entities/rustsymbol/co/compress-frame-body.md)
- `function` [`hex_id_to_raw`](entities/rustsymbol/he/hex-id-to-raw.md)
- `function` [`prune_empty_dirs`](entities/rustsymbol/pr/prune-empty-dirs.md)
- `function` [`scan_segment`](entities/rustsymbol/sc/scan-segment.md)
- `function` [`segment_paths`](entities/rustsymbol/se/segment-paths.md)

## ekos/crates/artifact/src/store.rs

- `trait` [`ArtifactStore`](entities/rustsymbol/ar/artifactstore.md)
- `struct` [`FileSystemArtifactStore`](entities/rustsymbol/fi/filesystemartifactstore.md)
- `method` [`FileSystemArtifactStore::artifact_path`](entities/rustsymbol/fi/filesystemartifactstore-artifact-path.md)
- `method` [`FileSystemArtifactStore::exists`](entities/rustsymbol/fi/filesystemartifactstore-exists.md)
- `method` [`FileSystemArtifactStore::list`](entities/rustsymbol/fi/filesystemartifactstore-list.md)
- `method` [`FileSystemArtifactStore::new`](entities/rustsymbol/fi/filesystemartifactstore-new.md)
- `method` [`FileSystemArtifactStore::read`](entities/rustsymbol/fi/filesystemartifactstore-read.md)
- `method` [`FileSystemArtifactStore::root`](entities/rustsymbol/fi/filesystemartifactstore-root.md)
- `method` [`FileSystemArtifactStore::write`](entities/rustsymbol/fi/filesystemartifactstore-write.md)
- `enum` [`StoreError`](entities/rustsymbol/st/storeerror.md)

## ekos/crates/cli/src/bin/ekos.rs

- `enum` [`ArtifactCommands`](entities/rustsymbol/ar/artifactcommands.md)
- `enum` [`BranchCommands`](entities/rustsymbol/br/branchcommands.md)
- `struct` [`Cli`](entities/rustsymbol/cl/cli.md)
- `enum` [`Commands`](entities/rustsymbol/co/commands.md)
- `enum` [`DbtCommands`](entities/rustsymbol/db/dbtcommands.md)
- `enum` [`DocsCommands`](entities/rustsymbol/do/docscommands.md)
- `enum` [`IdentityCommands`](entities/rustsymbol/id/identitycommands.md)
- `enum` [`LedgerCommands`](entities/rustsymbol/le/ledgercommands.md)
- `enum` [`MarketingCommands`](entities/rustsymbol/ma/marketingcommands.md)
- `enum` [`McpCommands`](entities/rustsymbol/mc/mcpcommands.md)
- `enum` [`QueryCommands`](entities/rustsymbol/qu/querycommands.md)
- `function` [`main`](entities/rustsymbol/ma/main-caa3d7b4.md)

## ekos/crates/cli/src/commands/artifact.rs

- `function` [`repack`](entities/rustsymbol/re/repack.md)

## ekos/crates/cli/src/commands/ask.rs

- `function` [`ai_config`](entities/rustsymbol/ai/ai-config.md)
- `function` [`run`](entities/rustsymbol/ru/run-9c8ba43a.md)

## ekos/crates/cli/src/commands/branch.rs

- `function` [`branch_path`](entities/rustsymbol/br/branch-path.md)
- `function` [`create`](entities/rustsymbol/cr/create.md)
- `function` [`delete`](entities/rustsymbol/de/delete.md)
- `function` [`list`](entities/rustsymbol/li/list.md)
- `function` [`merge`](entities/rustsymbol/me/merge.md)
- `function` [`open_branch`](entities/rustsymbol/op/open-branch.md)

## ekos/crates/cli/src/commands/build.rs

- `function` [`load_fingerprints`](entities/rustsymbol/lo/load-fingerprints.md)
- `function` [`prune_snapshots`](entities/rustsymbol/pr/prune-snapshots.md)
- `function` [`run`](entities/rustsymbol/ru/run-d09318f4.md)
- `function` [`save_fingerprints`](entities/rustsymbol/sa/save-fingerprints.md)

## ekos/crates/cli/src/commands/clean.rs

- `function` [`run`](entities/rustsymbol/ru/run-20c4c150.md)

## ekos/crates/cli/src/commands/commit.rs

- `function` [`ckm_object_to_kir`](entities/rustsymbol/ck/ckm-object-to-kir.md)
- `function` [`ckm_rel_to_kir`](entities/rustsymbol/ck/ckm-rel-to-kir.md)
- `function` [`evidence_record_to_kir`](entities/rustsymbol/ev/evidence-record-to-kir.md)
- `function` [`open_ledger`](entities/rustsymbol/op/open-ledger.md)
- `function` [`run`](entities/rustsymbol/ru/run-5eff14dd.md)

## ekos/crates/cli/src/commands/compile.rs

- `function` [`knowledge_artifact_ids`](entities/rustsymbol/kn/knowledge-artifact-ids.md)
- `function` [`run`](entities/rustsymbol/ru/run.md)

## ekos/crates/cli/src/commands/dbt.rs

- `function` [`generate`](entities/rustsymbol/ge/generate.md)
- `function` [`resolve_output_dir`](entities/rustsymbol/re/resolve-output-dir-730ab45b.md)
- `function` [`write_model`](entities/rustsymbol/wr/write-model.md)

## ekos/crates/cli/src/commands/diff.rs

- `function` [`run`](entities/rustsymbol/ru/run-b769e9f2.md)

## ekos/crates/cli/src/commands/docs.rs

- `enum` [`Format`](entities/rustsymbol/fo/format.md)
- `method` [`Format::parse`](entities/rustsymbol/fo/format-parse.md)
- `enum` [`Layout`](entities/rustsymbol/la/layout.md)
- `method` [`Layout::parse`](entities/rustsymbol/la/layout-parse.md)
- `function` [`confirm_prose_spend`](entities/rustsymbol/co/confirm-prose-spend.md)
- `function` [`enrich_with_prose`](entities/rustsymbol/en/enrich-with-prose.md)
- `function` [`estimate_prompt_tokens`](entities/rustsymbol/es/estimate-prompt-tokens.md)
- `function` [`generate`](entities/rustsymbol/ge/generate-9628a7cf.md)
- `function` [`generate_curated`](entities/rustsymbol/ge/generate-curated.md)
- `function` [`render_er_diagram_page`](entities/rustsymbol/re/render-er-diagram-page.md)
- `function` [`resolve_output_dir`](entities/rustsymbol/re/resolve-output-dir.md)
- `function` [`select_llm_provider_for_prose`](entities/rustsymbol/se/select-llm-provider-for-prose.md)
- `function` [`write_page`](entities/rustsymbol/wr/write-page.md)

## ekos/crates/cli/src/commands/doctor.rs

- `struct` [`Check`](entities/rustsymbol/ch/check.md)
- `method` [`Check::fail`](entities/rustsymbol/ch/check-fail.md)
- `method` [`Check::ok`](entities/rustsymbol/ch/check-ok.md)
- `function` [`run`](entities/rustsymbol/ru/run-a0c94dcf.md)

## ekos/crates/cli/src/commands/ekl.rs

- `function` [`render_cell`](entities/rustsymbol/re/render-cell.md)
- `function` [`run`](entities/rustsymbol/ru/run-682eaf6b.md)

## ekos/crates/cli/src/commands/identity.rs

- `function` [`scan`](entities/rustsymbol/sc/scan.md)

## ekos/crates/cli/src/commands/init.rs

- `function` [`run`](entities/rustsymbol/ru/run-2a325902.md)

## ekos/crates/cli/src/commands/ledger.rs

- `function` [`dir_size`](entities/rustsymbol/di/dir-size.md)
- `function` [`human_bytes`](entities/rustsymbol/hu/human-bytes.md)
- `function` [`migrate`](entities/rustsymbol/mi/migrate.md)
- `function` [`migrate_v3`](entities/rustsymbol/mi/migrate-v3.md)
- `function` [`print_storage_report`](entities/rustsymbol/pr/print-storage-report.md)
- `function` [`status`](entities/rustsymbol/st/status.md)

## ekos/crates/cli/src/commands/marketing.rs

- `function` [`approve`](entities/rustsymbol/ap/approve.md)
- `function` [`log_line`](entities/rustsymbol/lo/log-line.md)
- `function` [`publish`](entities/rustsymbol/pu/publish.md)
- `function` [`resolve_devlog_path`](entities/rustsymbol/re/resolve-devlog-path.md)
- `function` [`select_llm_provider`](entities/rustsymbol/se/select-llm-provider.md)

## ekos/crates/cli/src/commands/mcp.rs

- `function` [`call_tool`](entities/rustsymbol/ca/call-tool.md)
- `function` [`diff_chains`](entities/rustsymbol/di/diff-chains.md)
- `function` [`error_response`](entities/rustsymbol/er/error-response.md)
- `function` [`explain_node`](entities/rustsymbol/ex/explain-node.md)
- `function` [`handle_message`](entities/rustsymbol/ha/handle-message.md)
- `function` [`initialize_result`](entities/rustsymbol/in/initialize-result.md)
- `function` [`node_comparable`](entities/rustsymbol/no/node-comparable.md)
- `function` [`node_summary`](entities/rustsymbol/no/node-summary.md)
- `function` [`ok_response`](entities/rustsymbol/ok/ok-response.md)
- `function` [`required_id`](entities/rustsymbol/re/required-id.md)
- `function` [`required_str`](entities/rustsymbol/re/required-str.md)
- `function` [`run`](entities/rustsymbol/ru/run-6891f75c.md)
- `function` [`tool_definitions`](entities/rustsymbol/to/tool-definitions.md)
- `function` [`tools_call`](entities/rustsymbol/to/tools-call.md)
- `function` [`transformation_chain`](entities/rustsymbol/tr/transformation-chain.md)

## ekos/crates/cli/src/commands/mod.rs

- `function` [`init_logging`](entities/rustsymbol/in/init-logging.md)
- `function` [`init_logging_stderr`](entities/rustsymbol/in/init-logging-stderr.md)

## ekos/crates/cli/src/commands/query.rs

- `function` [`find`](entities/rustsymbol/fi/find.md)
- `function` [`neighbourhood`](entities/rustsymbol/ne/neighbourhood.md)
- `function` [`object`](entities/rustsymbol/ob/object-b6e1ea7f.md)
- `function` [`open_ledger`](entities/rustsymbol/op/open-ledger-fce4a499.md)

## ekos/crates/cli/src/commands/recover.rs

- `function` [`build_llm_provider`](entities/rustsymbol/bu/build-llm-provider.md)
- `function` [`collect_confluence_artifact_ids`](entities/rustsymbol/co/collect-confluence-artifact-ids.md)
- `function` [`collect_crypto_artifact_ids`](entities/rustsymbol/co/collect-crypto-artifact-ids.md)
- `function` [`collect_git_artifact_ids`](entities/rustsymbol/co/collect-git-artifact-ids.md)
- `function` [`collect_github_artifact_ids`](entities/rustsymbol/co/collect-github-artifact-ids.md)
- `function` [`collect_localdocs_artifact_ids`](entities/rustsymbol/co/collect-localdocs-artifact-ids.md)
- `function` [`collect_pentaho_artifact_ids`](entities/rustsymbol/co/collect-pentaho-artifact-ids.md)
- `function` [`collect_python_artifact_ids`](entities/rustsymbol/co/collect-python-artifact-ids.md)
- `function` [`collect_rust_artifact_ids`](entities/rustsymbol/co/collect-rust-artifact-ids.md)
- `function` [`run`](entities/rustsymbol/ru/run-786d5225.md)
- `function` [`should_register_document_semantics`](entities/rustsymbol/sh/should-register-document-semantics.md)

## ekos/crates/cli/src/commands/resolve.rs

- `function` [`merge_into`](entities/rustsymbol/me/merge-into.md)
- `function` [`run`](entities/rustsymbol/ru/run-e9261342.md)

## ekos/crates/cli/src/commands/store.rs

- `function` [`facts_dir`](entities/rustsymbol/fa/facts-dir.md)
- `function` [`open_store`](entities/rustsymbol/op/open-store.md)
- `function` [`store_display`](entities/rustsymbol/st/store-display.md)
- `function` [`uses_fact_engine`](entities/rustsymbol/us/uses-fact-engine.md)

## ekos/crates/cli/tests/mcp_session.rs

- `function` [`call_tool`](entities/rustsymbol/ca/call-tool-79df7d9c.md)
- `function` [`claude_code_session_over_mcp`](entities/rustsymbol/cl/claude-code-session-over-mcp.md)
- `function` [`load_config`](entities/rustsymbol/lo/load-config.md)
- `function` [`setup_workspace`](entities/rustsymbol/se/setup-workspace.md)

## ekos/crates/cli/tests/skeleton.rs

- `function` [`build_is_idempotent`](entities/rustsymbol/bu/build-is-idempotent.md)
- `function` [`build_observes_files_and_writes_ledger`](entities/rustsymbol/bu/build-observes-files-and-writes-ledger.md)
- `function` [`clean_removes_artifacts_not_ledger`](entities/rustsymbol/cl/clean-removes-artifacts-not-ledger.md)
- `function` [`init_creates_ekos_directory`](entities/rustsymbol/in/init-creates-ekos-directory.md)
- `function` [`load_config`](entities/rustsymbol/lo/load-config-d1e71ee3.md)
- `function` [`query_object_returns_known_file`](entities/rustsymbol/qu/query-object-returns-known-file.md)
- `function` [`setup_workspace`](entities/rustsymbol/se/setup-workspace-f8f102ad.md)

## ekos/crates/cli/tests/transformation_benchmark.rs

- `function` [`call_tool`](entities/rustsymbol/ca/call-tool-a762a492.md)
- `function` [`load_config`](entities/rustsymbol/lo/load-config-c16a7ca3.md)
- `function` [`phase7_benchmark_recover_explain_diff_over_mcp_only`](entities/rustsymbol/ph/phase7-benchmark-recover-explain-diff-over-mcp-only.md)
- `function` [`setup_workspace`](entities/rustsymbol/se/setup-workspace-e8ff1e4b.md)

## ekos/crates/common/src/compress.rs

- `enum` [`CompressError`](entities/rustsymbol/co/compresserror.md)
- `function` [`read_json_auto`](entities/rustsymbol/re/read-json-auto.md)
- `function` [`read_json_zst`](entities/rustsymbol/re/read-json-zst.md)
- `function` [`resolve_auto`](entities/rustsymbol/re/resolve-auto.md)
- `function` [`write_json_zst`](entities/rustsymbol/wr/write-json-zst.md)
- `function` [`zst_sibling`](entities/rustsymbol/zs/zst-sibling.md)

## ekos/crates/common/src/lib.rs

- `struct` [`ContentHash`](entities/rustsymbol/co/contenthash.md)
- `method` [`ContentHash::as_str`](entities/rustsymbol/co/contenthash-as-str.md)
- `method` [`ContentHash::fmt`](entities/rustsymbol/co/contenthash-fmt.md)
- `method` [`ContentHash::of`](entities/rustsymbol/co/contenthash-of.md)
- `method` [`ContentHash::of_str`](entities/rustsymbol/co/contenthash-of-str.md)

## ekos/crates/compiler-core/src/cache.rs

- `struct` [`PassManifest`](entities/rustsymbol/pa/passmanifest.md)
- `function` [`config_hash`](entities/rustsymbol/co/config-hash.md)
- `function` [`manifest_path`](entities/rustsymbol/ma/manifest-path.md)
- `function` [`record_manifest`](entities/rustsymbol/re/record-manifest.md)
- `function` [`should_recompute`](entities/rustsymbol/sh/should-recompute.md)

## ekos/crates/compiler-core/src/compiler.rs

- `struct` [`Compiler`](entities/rustsymbol/co/compiler.md)
- `method` [`Compiler::new`](entities/rustsymbol/co/compiler-new.md)
- `method` [`Compiler::register_pass`](entities/rustsymbol/co/compiler-register-pass.md)
- `method` [`Compiler::run`](entities/rustsymbol/co/compiler-run.md)
- `method` [`Compiler::with_failure_mode`](entities/rustsymbol/co/compiler-with-failure-mode.md)
- `enum` [`CompilerError`](entities/rustsymbol/co/compilererror.md)

## ekos/crates/compiler-core/src/config.rs

- `struct` [`AiConfig`](entities/rustsymbol/ai/aiconfig.md)
- `struct` [`DocumentSemanticsConfig`](entities/rustsymbol/do/documentsemanticsconfig.md)
- `struct` [`EkosConfig`](entities/rustsymbol/ek/ekosconfig.md)
- `method` [`EkosConfig::artifact_dir`](entities/rustsymbol/ek/ekosconfig-artifact-dir.md)
- `method` [`EkosConfig::branch_ledger_path`](entities/rustsymbol/ek/ekosconfig-branch-ledger-path.md)
- `method` [`EkosConfig::default`](entities/rustsymbol/ek/ekosconfig-default.md)
- `method` [`EkosConfig::ekos_dir`](entities/rustsymbol/ek/ekosconfig-ekos-dir.md)
- `method` [`EkosConfig::from_file`](entities/rustsymbol/ek/ekosconfig-from-file.md)
- `method` [`EkosConfig::from_file_or_default`](entities/rustsymbol/ek/ekosconfig-from-file-or-default.md)
- `method` [`EkosConfig::ledger_dir`](entities/rustsymbol/ek/ekosconfig-ledger-dir.md)
- `method` [`EkosConfig::ledger_path`](entities/rustsymbol/ek/ekosconfig-ledger-path.md)
- `struct` [`LlmConfig`](entities/rustsymbol/ll/llmconfig.md)
- `struct` [`MarketingConfig`](entities/rustsymbol/ma/marketingconfig.md)
- `method` [`MarketingConfig::default`](entities/rustsymbol/ma/marketingconfig-default.md)
- `struct` [`ObserveConfig`](entities/rustsymbol/ob/observeconfig.md)
- `method` [`ObserveConfig::default`](entities/rustsymbol/ob/observeconfig-default.md)
- `struct` [`RecoverConfig`](entities/rustsymbol/re/recoverconfig.md)
- `struct` [`SqlDialectRuleConfig`](entities/rustsymbol/sq/sqldialectruleconfig.md)
- `struct` [`SqlRecoverConfig`](entities/rustsymbol/sq/sqlrecoverconfig.md)
- `method` [`SqlRecoverConfig::default`](entities/rustsymbol/sq/sqlrecoverconfig-default.md)
- `struct` [`TwitterConfig`](entities/rustsymbol/tw/twitterconfig.md)
- `struct` [`WorkspaceConfig`](entities/rustsymbol/wo/workspaceconfig.md)
- `method` [`WorkspaceConfig::default`](entities/rustsymbol/wo/workspaceconfig-default.md)
- `function` [`default_github`](entities/rustsymbol/de/default-github.md)
- `function` [`default_hashtags`](entities/rustsymbol/de/default-hashtags.md)
- `function` [`default_ignore_patterns`](entities/rustsymbol/de/default-ignore-patterns.md)
- `function` [`default_log_format`](entities/rustsymbol/de/default-log-format.md)
- `function` [`default_log_level`](entities/rustsymbol/de/default-log-level.md)
- `function` [`default_root`](entities/rustsymbol/de/default-root.md)
- `function` [`default_sql_dialect`](entities/rustsymbol/de/default-sql-dialect.md)

## ekos/crates/compiler-core/src/diagnostics.rs

- `struct` [`Diagnostic`](entities/rustsymbol/di/diagnostic.md)
- `method` [`Diagnostic::at`](entities/rustsymbol/di/diagnostic-at.md)
- `method` [`Diagnostic::error`](entities/rustsymbol/di/diagnostic-error.md)
- `method` [`Diagnostic::info`](entities/rustsymbol/di/diagnostic-info.md)
- `method` [`Diagnostic::warning`](entities/rustsymbol/di/diagnostic-warning.md)
- `struct` [`DiagnosticSink`](entities/rustsymbol/di/diagnosticsink.md)
- `method` [`DiagnosticSink::diagnostics`](entities/rustsymbol/di/diagnosticsink-diagnostics.md)
- `method` [`DiagnosticSink::emit`](entities/rustsymbol/di/diagnosticsink-emit.md)
- `method` [`DiagnosticSink::error`](entities/rustsymbol/di/diagnosticsink-error.md)
- `method` [`DiagnosticSink::errors`](entities/rustsymbol/di/diagnosticsink-errors.md)
- `method` [`DiagnosticSink::has_errors`](entities/rustsymbol/di/diagnosticsink-has-errors.md)
- `method` [`DiagnosticSink::has_warnings`](entities/rustsymbol/di/diagnosticsink-has-warnings.md)
- `method` [`DiagnosticSink::info`](entities/rustsymbol/di/diagnosticsink-info.md)
- `method` [`DiagnosticSink::warning`](entities/rustsymbol/di/diagnosticsink-warning.md)
- `method` [`DiagnosticSink::warning_count`](entities/rustsymbol/di/diagnosticsink-warning-count.md)
- `enum` [`Severity`](entities/rustsymbol/se/severity.md)
- `struct` [`SourceLocation`](entities/rustsymbol/so/sourcelocation.md)

## ekos/crates/compiler-core/src/pass.rs

- `trait` [`CompilerPass`](entities/rustsymbol/co/compilerpass.md)
- `struct` [`PassContext`](entities/rustsymbol/pa/passcontext.md)
- `method` [`PassContext::new`](entities/rustsymbol/pa/passcontext-new.md)
- `method` [`PassContext::with_artifact_store`](entities/rustsymbol/pa/passcontext-with-artifact-store.md)
- `enum` [`PassError`](entities/rustsymbol/pa/passerror.md)
- `method` [`PassError::failed`](entities/rustsymbol/pa/passerror-failed.md)
- `struct` [`PassManager`](entities/rustsymbol/pa/passmanager.md)
- `method` [`PassManager::check_unique_names`](entities/rustsymbol/pa/passmanager-check-unique-names.md)
- `method` [`PassManager::default`](entities/rustsymbol/pa/passmanager-default.md)
- `method` [`PassManager::execution_levels`](entities/rustsymbol/pa/passmanager-execution-levels.md)
- `method` [`PassManager::execution_order`](entities/rustsymbol/pa/passmanager-execution-order.md)
- `method` [`PassManager::is_empty`](entities/rustsymbol/pa/passmanager-is-empty.md)
- `method` [`PassManager::len`](entities/rustsymbol/pa/passmanager-len.md)
- `method` [`PassManager::new`](entities/rustsymbol/pa/passmanager-new.md)
- `method` [`PassManager::register`](entities/rustsymbol/pa/passmanager-register.md)
- `method` [`PassManager::run_all`](entities/rustsymbol/pa/passmanager-run-all.md)
- `method` [`PassManager::run_all_parallel`](entities/rustsymbol/pa/passmanager-run-all-parallel.md)
- `enum` [`SchedulerError`](entities/rustsymbol/sc/schedulererror.md)

## ekos/crates/compiler-core/src/scheduler.rs

- `struct` [`ExecutionReport`](entities/rustsymbol/ex/executionreport.md)
- `method` [`ExecutionReport::error_count`](entities/rustsymbol/ex/executionreport-error-count.md)
- `method` [`ExecutionReport::error_outcomes`](entities/rustsymbol/ex/executionreport-error-outcomes.md)
- `method` [`ExecutionReport::has_errors`](entities/rustsymbol/ex/executionreport-has-errors.md)
- `method` [`ExecutionReport::passes_run`](entities/rustsymbol/ex/executionreport-passes-run.md)
- `method` [`ExecutionReport::passes_skipped`](entities/rustsymbol/ex/executionreport-passes-skipped.md)
- `enum` [`FailureMode`](entities/rustsymbol/fa/failuremode.md)
- `struct` [`PassOutcome`](entities/rustsymbol/pa/passoutcome.md)
- `method` [`PassOutcome::ran`](entities/rustsymbol/pa/passoutcome-ran.md)
- `method` [`PassOutcome::skipped`](entities/rustsymbol/pa/passoutcome-skipped.md)
- `struct` [`Scheduler`](entities/rustsymbol/sc/scheduler.md)
- `method` [`Scheduler::new`](entities/rustsymbol/sc/scheduler-new.md)
- `method` [`Scheduler::register`](entities/rustsymbol/sc/scheduler-register.md)
- `method` [`Scheduler::run`](entities/rustsymbol/sc/scheduler-run.md)
- `method` [`Scheduler::run_parallel`](entities/rustsymbol/sc/scheduler-run-parallel.md)

## ekos/crates/dbt-gen/src/lib.rs

- `struct` [`AggExprRow`](entities/rustsymbol/ag/aggexprrow.md)
- `struct` [`DbtModelFile`](entities/rustsymbol/db/dbtmodelfile.md)
- `function` [`comment_block`](entities/rustsymbol/co/comment-block.md)
- `function` [`dbt_model_name`](entities/rustsymbol/db/dbt-model-name.md)
- `function` [`get_aggs`](entities/rustsymbol/ge/get-aggs.md)
- `function` [`get_pairs`](entities/rustsymbol/ge/get-pairs.md)
- `function` [`get_str`](entities/rustsymbol/ge/get-str.md)
- `function` [`get_str_vec`](entities/rustsymbol/ge/get-str-vec.md)
- `function` [`is_feeds_into`](entities/rustsymbol/is/is-feeds-into-af2f1802.md)
- `function` [`is_transform_node`](entities/rustsymbol/is/is-transform-node.md)
- `function` [`no_upstream_placeholder`](entities/rustsymbol/no/no-upstream-placeholder.md)
- `function` [`render_aggregate`](entities/rustsymbol/re/render-aggregate.md)
- `function` [`render_calculate`](entities/rustsymbol/re/render-calculate.md)
- `function` [`render_dbt_model`](entities/rustsymbol/re/render-dbt-model.md)
- `function` [`render_filter`](entities/rustsymbol/re/render-filter.md)
- `function` [`render_join`](entities/rustsymbol/re/render-join.md)
- `function` [`render_schema_yml`](entities/rustsymbol/re/render-schema-yml.md)
- `function` [`render_sink`](entities/rustsymbol/re/render-sink.md)
- `function` [`render_source`](entities/rustsymbol/re/render-source.md)
- `function` [`render_unmapped`](entities/rustsymbol/re/render-unmapped.md)
- `function` [`slugify_snake`](entities/rustsymbol/sl/slugify-snake.md)
- `function` [`upstream_model_names`](entities/rustsymbol/up/upstream-model-names.md)

## ekos/crates/docs-gen/src/lib.rs

- `struct` [`EvidenceRow`](entities/rustsymbol/ev/evidencerow.md)
- `struct` [`ObjectPageModel`](entities/rustsymbol/ob/objectpagemodel.md)
- `struct` [`ProseSection`](entities/rustsymbol/pr/prosesection.md)
- `struct` [`RelationshipRow`](entities/rustsymbol/re/relationshiprow.md)
- `struct` [`RenderedPage`](entities/rustsymbol/re/renderedpage.md)
- `enum` [`RowEvidence`](entities/rustsymbol/ro/rowevidence.md)
- `function` [`build_object_page_model`](entities/rustsymbol/bu/build-object-page-model.md)
- `function` [`components_cross_reference`](entities/rustsymbol/co/components-cross-reference.md)
- `function` [`count_by_kind`](entities/rustsymbol/co/count-by-kind.md)
- `function` [`format_value`](entities/rustsymbol/fo/format-value.md)
- `function` [`html_document`](entities/rustsymbol/ht/html-document.md)
- `function` [`html_escape`](entities/rustsymbol/ht/html-escape.md)
- `function` [`is_feeds_into`](entities/rustsymbol/is/is-feeds-into.md)
- `function` [`is_module_kind`](entities/rustsymbol/is/is-module-kind.md)
- `function` [`is_significant`](entities/rustsymbol/is/is-significant.md)
- `function` [`is_symbol_kind`](entities/rustsymbol/is/is-symbol-kind.md)
- `function` [`mermaid_arrow`](entities/rustsymbol/me/mermaid-arrow.md)
- `function` [`mermaid_escape_label`](entities/rustsymbol/me/mermaid-escape-label.md)
- `function` [`mermaid_node_id`](entities/rustsymbol/me/mermaid-node-id.md)
- `function` [`page_file_name`](entities/rustsymbol/pa/page-file-name.md)
- `function` [`render_api`](entities/rustsymbol/re/render-api.md)
- `function` [`render_api_from_legacy_file_symbols`](entities/rustsymbol/re/render-api-from-legacy-file-symbols.md)
- `function` [`render_architecture`](entities/rustsymbol/re/render-architecture.md)
- `function` [`render_call_sequences_section`](entities/rustsymbol/re/render-call-sequences-section.md)
- `function` [`render_er_diagram`](entities/rustsymbol/re/render-er-diagram.md)
- `function` [`render_html_er_diagram_page`](entities/rustsymbol/re/render-html-er-diagram-page.md)
- `function` [`render_html_index_page`](entities/rustsymbol/re/render-html-index-page.md)
- `function` [`render_html_object_page`](entities/rustsymbol/re/render-html-object-page.md)
- `function` [`render_index_page`](entities/rustsymbol/re/render-index-page.md)
- `function` [`render_markdown_object_page`](entities/rustsymbol/re/render-markdown-object-page.md)
- `function` [`render_mermaid_graph`](entities/rustsymbol/re/render-mermaid-graph.md)
- `function` [`render_object_page`](entities/rustsymbol/re/render-object-page.md)
- `function` [`render_readme`](entities/rustsymbol/re/render-readme.md)
- `function` [`render_relationship_kind_graph`](entities/rustsymbol/re/render-relationship-kind-graph.md)
- `function` [`render_sequence_diagrams`](entities/rustsymbol/re/render-sequence-diagrams.md)
- `function` [`sequence_participant_line`](entities/rustsymbol/se/sequence-participant-line.md)
- `function` [`slugify`](entities/rustsymbol/sl/slugify.md)
- `function` [`strip_mermaid_fence`](entities/rustsymbol/st/strip-mermaid-fence.md)
- `function` [`transform_node_origin`](entities/rustsymbol/tr/transform-node-origin.md)
- `function` [`unique_page_file_names`](entities/rustsymbol/un/unique-page-file-names.md)

## ekos/crates/ekl/src/interpreter.rs

- `enum` [`EklError`](entities/rustsymbol/ek/eklerror.md)
- `struct` [`EklInterpreter`](entities/rustsymbol/ek/eklinterpreter.md)
- `method` [`EklInterpreter::candidate_rows`](entities/rustsymbol/ek/eklinterpreter-candidate-rows.md)
- `method` [`EklInterpreter::execute`](entities/rustsymbol/ek/eklinterpreter-execute.md)
- `method` [`EklInterpreter::expand_from_anchor`](entities/rustsymbol/ek/eklinterpreter-expand-from-anchor.md)
- `method` [`EklInterpreter::new`](entities/rustsymbol/ek/eklinterpreter-new.md)
- `method` [`EklInterpreter::resolve_anchor`](entities/rustsymbol/ek/eklinterpreter-resolve-anchor.md)
- `struct` [`EklResult`](entities/rustsymbol/ek/eklresult.md)
- `function` [`compare_rows`](entities/rustsymbol/co/compare-rows.md)
- `function` [`default_returns`](entities/rustsymbol/de/default-returns.md)
- `function` [`eval_predicate`](entities/rustsymbol/ev/eval-predicate.md)
- `function` [`literal_as_f64`](entities/rustsymbol/li/literal-as-f64.md)
- `function` [`literal_to_string`](entities/rustsymbol/li/literal-to-string.md)
- `function` [`object_row`](entities/rustsymbol/ob/object-row.md)
- `function` [`project`](entities/rustsymbol/pr/project-990853c7.md)
- `function` [`relationship_row`](entities/rustsymbol/re/relationship-row.md)
- `function` [`value_as_f64`](entities/rustsymbol/va/value-as-f64.md)
- `function` [`value_eq`](entities/rustsymbol/va/value-eq.md)
- `function` [`value_to_string`](entities/rustsymbol/va/value-to-string.md)

## ekos/crates/ekl/src/parser.rs

- `struct` [`EklAst`](entities/rustsymbol/ek/eklast.md)
- `enum` [`Entity`](entities/rustsymbol/en/entity.md)
- `struct` [`Lexer`](entities/rustsymbol/le/lexer.md)
- `method` [`Lexer::match_symbol_op`](entities/rustsymbol/le/lexer-match-symbol-op.md)
- `method` [`Lexer::new`](entities/rustsymbol/le/lexer-new.md)
- `method` [`Lexer::read_ident`](entities/rustsymbol/le/lexer-read-ident.md)
- `method` [`Lexer::read_number`](entities/rustsymbol/le/lexer-read-number.md)
- `method` [`Lexer::read_string`](entities/rustsymbol/le/lexer-read-string.md)
- `method` [`Lexer::skip_whitespace`](entities/rustsymbol/le/lexer-skip-whitespace.md)
- `method` [`Lexer::tokenize`](entities/rustsymbol/le/lexer-tokenize.md)
- `enum` [`Literal`](entities/rustsymbol/li/literal.md)
- `enum` [`Op`](entities/rustsymbol/op/op.md)
- `enum` [`Order`](entities/rustsymbol/or/order.md)
- `struct` [`ParseError`](entities/rustsymbol/pa/parseerror.md)
- `method` [`ParseError::fmt`](entities/rustsymbol/pa/parseerror-fmt.md)
- `struct` [`Parser`](entities/rustsymbol/pa/parser.md)
- `method` [`Parser::advance`](entities/rustsymbol/pa/parser-advance.md)
- `method` [`Parser::expect_ident`](entities/rustsymbol/pa/parser-expect-ident.md)
- `method` [`Parser::expect_keyword`](entities/rustsymbol/pa/parser-expect-keyword.md)
- `method` [`Parser::expect_num`](entities/rustsymbol/pa/parser-expect-num.md)
- `method` [`Parser::expect_string`](entities/rustsymbol/pa/parser-expect-string.md)
- `method` [`Parser::new`](entities/rustsymbol/pa/parser-new.md)
- `method` [`Parser::parse_entity`](entities/rustsymbol/pa/parser-parse-entity.md)
- `method` [`Parser::parse_literal`](entities/rustsymbol/pa/parser-parse-literal.md)
- `method` [`Parser::parse_op`](entities/rustsymbol/pa/parser-parse-op.md)
- `method` [`Parser::parse_predicate`](entities/rustsymbol/pa/parser-parse-predicate.md)
- `method` [`Parser::parse_query`](entities/rustsymbol/pa/parser-parse-query.md)
- `method` [`Parser::peek`](entities/rustsymbol/pa/parser-peek.md)
- `method` [`Parser::peek_keyword`](entities/rustsymbol/pa/parser-peek-keyword.md)
- `method` [`Parser::peek_pos`](entities/rustsymbol/pa/parser-peek-pos.md)
- `struct` [`Predicate`](entities/rustsymbol/pr/predicate.md)
- `enum` [`Token`](entities/rustsymbol/to/token.md)
- `function` [`describe`](entities/rustsymbol/de/describe.md)
- `function` [`ekl_parse`](entities/rustsymbol/ek/ekl-parse.md)

## ekos/crates/identity/src/cross_system.rs

- `struct` [`CrossSystemCandidate`](entities/rustsymbol/cr/crosssystemcandidate.md)
- `struct` [`CrossSystemSignals`](entities/rustsymbol/cr/crosssystemsignals.md)
- `function` [`column_overlap_score`](entities/rustsymbol/co/column-overlap-score.md)
- `function` [`column_types`](entities/rustsymbol/co/column-types.md)
- `function` [`combine_signals`](entities/rustsymbol/co/combine-signals.md)
- `function` [`find_cross_system_candidates`](entities/rustsymbol/fi/find-cross-system-candidates.md)
- `function` [`matchable_name`](entities/rustsymbol/ma/matchable-name.md)
- `function` [`normalize_cross_system`](entities/rustsymbol/no/normalize-cross-system.md)
- `function` [`type_compat_score`](entities/rustsymbol/ty/type-compat-score.md)
- `function` [`type_family`](entities/rustsymbol/ty/type-family.md)

## ekos/crates/identity/src/lib.rs

- `enum` [`ConflictKind`](entities/rustsymbol/co/conflictkind.md)
- `struct` [`ConflictReport`](entities/rustsymbol/co/conflictreport.md)
- `struct` [`DefaultResolver`](entities/rustsymbol/de/defaultresolver.md)
- `method` [`DefaultResolver::default`](entities/rustsymbol/de/defaultresolver-default.md)
- `method` [`DefaultResolver::new`](entities/rustsymbol/de/defaultresolver-new.md)
- `method` [`DefaultResolver::resolve`](entities/rustsymbol/de/defaultresolver-resolve.md)
- `method` [`DefaultResolver::score`](entities/rustsymbol/de/defaultresolver-score.md)
- `method` [`DefaultResolver::threshold_for`](entities/rustsymbol/de/defaultresolver-threshold-for.md)
- `method` [`DefaultResolver::with_kind_threshold`](entities/rustsymbol/de/defaultresolver-with-kind-threshold.md)
- `method` [`DefaultResolver::with_threshold`](entities/rustsymbol/de/defaultresolver-with-threshold.md)
- `trait` [`IdentityResolver`](entities/rustsymbol/id/identityresolver.md)
- `struct` [`MergeProposal`](entities/rustsymbol/me/mergeproposal.md)
- `struct` [`ResolutionResult`](entities/rustsymbol/re/resolutionresult.md)
- `struct` [`ResolutionStats`](entities/rustsymbol/re/resolutionstats.md)
- `struct` [`ResolverConfig`](entities/rustsymbol/re/resolverconfig.md)
- `method` [`ResolverConfig::default`](entities/rustsymbol/re/resolverconfig-default.md)
- `struct` [`SimilarityScore`](entities/rustsymbol/si/similarityscore.md)
- `struct` [`UnionFind`](entities/rustsymbol/un/unionfind.md)
- `method` [`UnionFind::find`](entities/rustsymbol/un/unionfind-find.md)
- `method` [`UnionFind::new`](entities/rustsymbol/un/unionfind-new.md)
- `method` [`UnionFind::union`](entities/rustsymbol/un/unionfind-union.md)
- `function` [`structural_score`](entities/rustsymbol/st/structural-score.md)

## ekos/crates/identity/src/similarity.rs

- `function` [`column_names`](entities/rustsymbol/co/column-names.md)
- `function` [`jaccard`](entities/rustsymbol/ja/jaccard.md)
- `function` [`jaro`](entities/rustsymbol/ja/jaro.md)
- `function` [`jaro_winkler`](entities/rustsymbol/ja/jaro-winkler.md)
- `function` [`normalize`](entities/rustsymbol/no/normalize.md)

## ekos/crates/kir/src/lib.rs

- `enum` [`EventKind`](entities/rustsymbol/ev/eventkind.md)
- `struct` [`KirEvent`](entities/rustsymbol/ki/kirevent.md)
- `struct` [`KirEvidence`](entities/rustsymbol/ki/kirevidence.md)
- `method` [`KirEvidence::new`](entities/rustsymbol/ki/kirevidence-new.md)
- `method` [`KirEvidence::with_confidence`](entities/rustsymbol/ki/kirevidence-with-confidence.md)
- `struct` [`KirGraph`](entities/rustsymbol/ki/kirgraph.md)
- `method` [`KirGraph::add_evidence`](entities/rustsymbol/ki/kirgraph-add-evidence.md)
- `method` [`KirGraph::add_object`](entities/rustsymbol/ki/kirgraph-add-object.md)
- `method` [`KirGraph::add_relationship`](entities/rustsymbol/ki/kirgraph-add-relationship.md)
- `method` [`KirGraph::get_evidence`](entities/rustsymbol/ki/kirgraph-get-evidence.md)
- `method` [`KirGraph::get_object`](entities/rustsymbol/ki/kirgraph-get-object.md)
- `method` [`KirGraph::new`](entities/rustsymbol/ki/kirgraph-new.md)
- `struct` [`KirId`](entities/rustsymbol/ki/kirid.md)
- `method` [`KirId::as_str`](entities/rustsymbol/ki/kirid-as-str.md)
- `method` [`KirId::default`](entities/rustsymbol/ki/kirid-default.md)
- `method` [`KirId::fmt`](entities/rustsymbol/ki/kirid-fmt.md)
- `method` [`KirId::from_str`](entities/rustsymbol/ki/kirid-from-str.md)
- `method` [`KirId::new`](entities/rustsymbol/ki/kirid-new.md)
- `struct` [`KirObject`](entities/rustsymbol/ki/kirobject.md)
- `method` [`KirObject::indexed_content`](entities/rustsymbol/ki/kirobject-indexed-content.md)
- `method` [`KirObject::new`](entities/rustsymbol/ki/kirobject-new.md)
- `method` [`KirObject::with_evidence`](entities/rustsymbol/ki/kirobject-with-evidence.md)
- `method` [`KirObject::with_property`](entities/rustsymbol/ki/kirobject-with-property.md)
- `struct` [`KirRelationship`](entities/rustsymbol/ki/kirrelationship.md)
- `method` [`KirRelationship::is_pending_review`](entities/rustsymbol/ki/kirrelationship-is-pending-review.md)
- `method` [`KirRelationship::new`](entities/rustsymbol/ki/kirrelationship-new.md)
- `enum` [`ObjectKind`](entities/rustsymbol/ob/objectkind.md)
- `method` [`ObjectKind::fmt`](entities/rustsymbol/ob/objectkind-fmt.md)
- `enum` [`RelationshipKind`](entities/rustsymbol/re/relationshipkind.md)
- `method` [`RelationshipKind::fmt`](entities/rustsymbol/re/relationshipkind-fmt.md)
- `method` [`RelationshipKind::from_str`](entities/rustsymbol/re/relationshipkind-from-str.md)
- `struct` [`SourceLocation`](entities/rustsymbol/so/sourcelocation-f4972231.md)
- `method` [`SourceLocation::at`](entities/rustsymbol/so/sourcelocation-at.md)
- `method` [`SourceLocation::file`](entities/rustsymbol/so/sourcelocation-file.md)

## ekos/crates/ledger/src/fact.rs

- `struct` [`AttrId`](entities/rustsymbol/at/attrid.md)
- `struct` [`AttributeRegistry`](entities/rustsymbol/at/attributeregistry.md)
- `method` [`AttributeRegistry::get`](entities/rustsymbol/at/attributeregistry-get.md)
- `method` [`AttributeRegistry::intern`](entities/rustsymbol/at/attributeregistry-intern.md)
- `method` [`AttributeRegistry::is_empty`](entities/rustsymbol/at/attributeregistry-is-empty.md)
- `method` [`AttributeRegistry::len`](entities/rustsymbol/at/attributeregistry-len.md)
- `method` [`AttributeRegistry::name`](entities/rustsymbol/at/attributeregistry-name.md)
- `method` [`AttributeRegistry::new`](entities/rustsymbol/at/attributeregistry-new.md)
- `method` [`AttributeRegistry::reindex`](entities/rustsymbol/at/attributeregistry-reindex.md)
- `struct` [`Fact`](entities/rustsymbol/fa/fact.md)
- `enum` [`FactError`](entities/rustsymbol/fa/facterror.md)
- `enum` [`FactOp`](entities/rustsymbol/fa/factop.md)
- `enum` [`FactValue`](entities/rustsymbol/fa/factvalue.md)
- `struct` [`TxId`](entities/rustsymbol/tx/txid.md)
- `function` [`canonical_uuid`](entities/rustsymbol/ca/canonical-uuid.md)
- `function` [`decompose`](entities/rustsymbol/de/decompose.md)
- `function` [`diff`](entities/rustsymbol/di/diff.md)
- `function` [`escape_segment`](entities/rustsymbol/es/escape-segment.md)
- `function` [`flatten`](entities/rustsymbol/fl/flatten.md)
- `function` [`insert_path`](entities/rustsymbol/in/insert-path.md)
- `function` [`reconstruct`](entities/rustsymbol/re/reconstruct.md)
- `function` [`split_path`](entities/rustsymbol/sp/split-path.md)
- `function` [`type_name`](entities/rustsymbol/ty/type-name.md)
- `function` [`value_to_json`](entities/rustsymbol/va/value-to-json.md)

## ekos/crates/ledger/src/fact_ledger.rs

- `enum` [`EntityKind`](entities/rustsymbol/en/entitykind.md)
- `struct` [`FactLedger`](entities/rustsymbol/fa/factledger.md)
- `method` [`FactLedger::all_objects`](entities/rustsymbol/fa/factledger-all-objects.md)
- `method` [`FactLedger::all_of_kind`](entities/rustsymbol/fa/factledger-all-of-kind.md)
- `method` [`FactLedger::all_relationships`](entities/rustsymbol/fa/factledger-all-relationships.md)
- `method` [`FactLedger::append_event`](entities/rustsymbol/fa/factledger-append-event.md)
- `method` [`FactLedger::append_evidence`](entities/rustsymbol/fa/factledger-append-evidence.md)
- `method` [`FactLedger::append_inner`](entities/rustsymbol/fa/factledger-append-inner.md)
- `method` [`FactLedger::append_object`](entities/rustsymbol/fa/factledger-append-object.md)
- `method` [`FactLedger::append_payload`](entities/rustsymbol/fa/factledger-append-payload.md)
- `method` [`FactLedger::append_relationship`](entities/rustsymbol/fa/factledger-append-relationship.md)
- `method` [`FactLedger::append_version`](entities/rustsymbol/fa/factledger-append-version.md)
- `method` [`FactLedger::current_signature`](entities/rustsymbol/fa/factledger-current-signature.md)
- `method` [`FactLedger::diff`](entities/rustsymbol/fa/factledger-diff.md)
- `method` [`FactLedger::entry_count`](entities/rustsymbol/fa/factledger-entry-count.md)
- `method` [`FactLedger::find_objects`](entities/rustsymbol/fa/factledger-find-objects.md)
- `method` [`FactLedger::get_event`](entities/rustsymbol/fa/factledger-get-event.md)
- `method` [`FactLedger::get_evidence`](entities/rustsymbol/fa/factledger-get-evidence.md)
- `method` [`FactLedger::get_object`](entities/rustsymbol/fa/factledger-get-object.md)
- `method` [`FactLedger::get_relationship`](entities/rustsymbol/fa/factledger-get-relationship.md)
- `method` [`FactLedger::merge_from`](entities/rustsymbol/fa/factledger-merge-from.md)
- `method` [`FactLedger::object_at`](entities/rustsymbol/fa/factledger-object-at.md)
- `method` [`FactLedger::object_count`](entities/rustsymbol/fa/factledger-object-count.md)
- `method` [`FactLedger::open`](entities/rustsymbol/fa/factledger-open.md)
- `method` [`FactLedger::open_with_seal_threshold`](entities/rustsymbol/fa/factledger-open-with-seal-threshold.md)
- `method` [`FactLedger::relationship_count`](entities/rustsymbol/fa/factledger-relationship-count.md)
- `method` [`FactLedger::relationships_at`](entities/rustsymbol/fa/factledger-relationships-at.md)
- `method` [`FactLedger::relationships_for`](entities/rustsymbol/fa/factledger-relationships-for.md)
- `method` [`FactLedger::run_count`](entities/rustsymbol/fa/factledger-run-count.md)
- `method` [`FactLedger::seal_and_flush`](entities/rustsymbol/fa/factledger-seal-and-flush.md)
- `method` [`FactLedger::set_segment_dictionary`](entities/rustsymbol/fa/factledger-set-segment-dictionary.md)
- `method` [`FactLedger::typed_current`](entities/rustsymbol/fa/factledger-typed-current.md)
- `method` [`FactLedger::vacuum_into`](entities/rustsymbol/fa/factledger-vacuum-into.md)
- `struct` [`Inner`](entities/rustsymbol/in/inner.md)
- `method` [`Inner::all_current_payloads`](entities/rustsymbol/in/inner-all-current-payloads.md)
- `method` [`Inner::current_sig`](entities/rustsymbol/in/inner-current-sig.md)
- `method` [`Inner::entities_with_attr`](entities/rustsymbol/in/inner-entities-with-attr.md)
- `method` [`Inner::entity_entries`](entities/rustsymbol/in/inner-entity-entries.md)
- `method` [`Inner::flush_memtable`](entities/rustsymbol/in/inner-flush-memtable.md)
- `method` [`Inner::index_object`](entities/rustsymbol/in/inner-index-object.md)
- `method` [`Inner::reconstruct_at`](entities/rustsymbol/in/inner-reconstruct-at.md)
- `method` [`Inner::relationship_candidates`](entities/rustsymbol/in/inner-relationship-candidates.md)
- `method` [`Inner::runs_dir`](entities/rustsymbol/in/inner-runs-dir.md)
- `method` [`Inner::state_at`](entities/rustsymbol/in/inner-state-at.md)
- `method` [`Inner::tx_at`](entities/rustsymbol/in/inner-tx-at.md)
- `method` [`LedgerError::from`](entities/rustsymbol/le/ledgererror-from.md)
- `function` [`copy_dir`](entities/rustsymbol/co/copy-dir.md)
- `function` [`fold_state`](entities/rustsymbol/fo/fold-state.md)
- `function` [`kind_of_payload`](entities/rustsymbol/ki/kind-of-payload.md)
- `function` [`self_counts`](entities/rustsymbol/se/self-counts.md)

## ekos/crates/ledger/src/index.rs

- `struct` [`BlockMeta`](entities/rustsymbol/bl/blockmeta.md)
- `struct` [`FactIndexes`](entities/rustsymbol/fa/factindexes.md)
- `method` [`FactIndexes::add_runs`](entities/rustsymbol/fa/factindexes-add-runs.md)
- `method` [`FactIndexes::build_from_batches`](entities/rustsymbol/fa/factindexes-build-from-batches.md)
- `method` [`FactIndexes::merge_runs`](entities/rustsymbol/fa/factindexes-merge-runs.md)
- `method` [`FactIndexes::open`](entities/rustsymbol/fa/factindexes-open.md)
- `method` [`FactIndexes::run_count`](entities/rustsymbol/fa/factindexes-run-count.md)
- `method` [`FactIndexes::runs_of`](entities/rustsymbol/fa/factindexes-runs-of.md)
- `method` [`FactIndexes::scan`](entities/rustsymbol/fa/factindexes-scan.md)
- `struct` [`IndexEntry`](entities/rustsymbol/in/indexentry.md)
- `method` [`IndexEntry::from_fact`](entities/rustsymbol/in/indexentry-from-fact.md)
- `struct` [`IndexRun`](entities/rustsymbol/in/indexrun.md)
- `method` [`IndexRun::all`](entities/rustsymbol/in/indexrun-all.md)
- `method` [`IndexRun::all_raw`](entities/rustsymbol/in/indexrun-all-raw.md)
- `method` [`IndexRun::entry_count`](entities/rustsymbol/in/indexrun-entry-count.md)
- `method` [`IndexRun::open`](entities/rustsymbol/in/indexrun-open.md)
- `method` [`IndexRun::order`](entities/rustsymbol/in/indexrun-order.md)
- `method` [`IndexRun::read_block_raw`](entities/rustsymbol/in/indexrun-read-block-raw.md)
- `method` [`IndexRun::scan`](entities/rustsymbol/in/indexrun-scan.md)
- `struct` [`RunDirectory`](entities/rustsymbol/ru/rundirectory.md)
- `enum` [`ScanPrefix`](entities/rustsymbol/sc/scanprefix.md)
- `method` [`ScanPrefix::bytes`](entities/rustsymbol/sc/scanprefix-bytes.md)
- `method` [`ScanPrefix::order`](entities/rustsymbol/sc/scanprefix-order.md)
- `enum` [`SortOrder`](entities/rustsymbol/so/sortorder.md)
- `method` [`SortOrder::prefix`](entities/rustsymbol/so/sortorder-prefix.md)
- `function` [`decode_block`](entities/rustsymbol/de/decode-block.md)
- `function` [`encode_block`](entities/rustsymbol/en/encode-block.md)
- `function` [`encode_key`](entities/rustsymbol/en/encode-key.md)
- `function` [`entries_from_batches`](entities/rustsymbol/en/entries-from-batches.md)
- `function` [`in_prefix`](entities/rustsymbol/in/in-prefix.md)
- `function` [`project`](entities/rustsymbol/pr/project.md)
- `function` [`push_escaped`](entities/rustsymbol/pu/push-escaped.md)
- `function` [`push_pos`](entities/rustsymbol/pu/push-pos.md)
- `function` [`stores_values`](entities/rustsymbol/st/stores-values.md)
- `function` [`value_order_key`](entities/rustsymbol/va/value-order-key.md)
- `function` [`write_run`](entities/rustsymbol/wr/write-run.md)
- `function` [`write_run_raw`](entities/rustsymbol/wr/write-run-raw.md)

## ekos/crates/ledger/src/lib.rs

- `enum` [`Codec`](entities/rustsymbol/co/codec.md)
- `method` [`Codec::compress`](entities/rustsymbol/co/codec-compress.md)
- `method` [`Codec::decompress`](entities/rustsymbol/co/codec-decompress.md)
- `method` [`Codec::zstd`](entities/rustsymbol/co/codec-zstd.md)
- `struct` [`Dict`](entities/rustsymbol/di/dict.md)
- `enum` [`EntryType`](entities/rustsymbol/en/entrytype.md)
- `method` [`EntryType::as_str`](entities/rustsymbol/en/entrytype-as-str.md)
- `method` [`FactLedger::diff_impl`](entities/rustsymbol/fa/factledger-diff-impl.md)
- `enum` [`Format`](entities/rustsymbol/fo/format-2bc470e0.md)
- `trait` [`KnowledgeStore`](entities/rustsymbol/kn/knowledgestore.md)
- `struct` [`Ledger`](entities/rustsymbol/le/ledger.md)
- `method` [`Ledger::all_objects`](entities/rustsymbol/le/ledger-all-objects.md)
- `method` [`Ledger::all_objects_with_rowids`](entities/rustsymbol/le/ledger-all-objects-with-rowids.md)
- `method` [`Ledger::all_relationships`](entities/rustsymbol/le/ledger-all-relationships.md)
- `method` [`Ledger::append`](entities/rustsymbol/le/ledger-append.md)
- `method` [`Ledger::append_event`](entities/rustsymbol/le/ledger-append-event.md)
- `method` [`Ledger::append_evidence`](entities/rustsymbol/le/ledger-append-evidence.md)
- `method` [`Ledger::append_object`](entities/rustsymbol/le/ledger-append-object.md)
- `method` [`Ledger::append_relationship`](entities/rustsymbol/le/ledger-append-relationship.md)
- `method` [`Ledger::append_versioned`](entities/rustsymbol/le/ledger-append-versioned.md)
- `method` [`Ledger::create_v2`](entities/rustsymbol/le/ledger-create-v2.md)
- `method` [`Ledger::diff_impl`](entities/rustsymbol/le/ledger-diff-impl.md)
- `method` [`Ledger::entry_count`](entities/rustsymbol/le/ledger-entry-count.md)
- `method` [`Ledger::export_versions`](entities/rustsymbol/le/ledger-export-versions.md)
- `method` [`Ledger::find_objects`](entities/rustsymbol/le/ledger-find-objects.md)
- `method` [`Ledger::find_objects_v1`](entities/rustsymbol/le/ledger-find-objects-v1.md)
- `method` [`Ledger::find_objects_v2`](entities/rustsymbol/le/ledger-find-objects-v2.md)
- `method` [`Ledger::get_event`](entities/rustsymbol/le/ledger-get-event.md)
- `method` [`Ledger::get_evidence`](entities/rustsymbol/le/ledger-get-evidence.md)
- `method` [`Ledger::get_object`](entities/rustsymbol/le/ledger-get-object.md)
- `method` [`Ledger::get_relationship`](entities/rustsymbol/le/ledger-get-relationship.md)
- `method` [`Ledger::id_param`](entities/rustsymbol/le/ledger-id-param.md)
- `method` [`Ledger::index_object_fts_v1`](entities/rustsymbol/le/ledger-index-object-fts-v1.md)
- `method` [`Ledger::index_object_fts_v2`](entities/rustsymbol/le/ledger-index-object-fts-v2.md)
- `method` [`Ledger::migrate_fts_v2`](entities/rustsymbol/le/ledger-migrate-fts-v2.md)
- `method` [`Ledger::object_at`](entities/rustsymbol/le/ledger-object-at.md)
- `method` [`Ledger::object_count`](entities/rustsymbol/le/ledger-object-count.md)
- `method` [`Ledger::open`](entities/rustsymbol/le/ledger-open.md)
- `method` [`Ledger::payload_param`](entities/rustsymbol/le/ledger-payload-param.md)
- `method` [`Ledger::payload_to_string`](entities/rustsymbol/le/ledger-payload-to-string.md)
- `method` [`Ledger::query_payloads`](entities/rustsymbol/le/ledger-query-payloads.md)
- `method` [`Ledger::relationship_count`](entities/rustsymbol/le/ledger-relationship-count.md)
- `method` [`Ledger::relationships_at`](entities/rustsymbol/le/ledger-relationships-at.md)
- `method` [`Ledger::relationships_for`](entities/rustsymbol/le/ledger-relationships-for.md)
- `method` [`Ledger::sig_param`](entities/rustsymbol/le/ledger-sig-param.md)
- `method` [`Ledger::storage_stats`](entities/rustsymbol/le/ledger-storage-stats.md)
- `method` [`Ledger::ts_param`](entities/rustsymbol/le/ledger-ts-param.md)
- `method` [`Ledger::vacuum_into`](entities/rustsymbol/le/ledger-vacuum-into.md)
- `method` [`Ledger::versions_in_window`](entities/rustsymbol/le/ledger-versions-in-window.md)
- `struct` [`LedgerDiff`](entities/rustsymbol/le/ledgerdiff.md)
- `struct` [`LedgerEntry`](entities/rustsymbol/le/ledgerentry.md)
- `struct` [`LedgerEntryId`](entities/rustsymbol/le/ledgerentryid.md)
- `enum` [`LedgerError`](entities/rustsymbol/le/ledgererror.md)
- `struct` [`MergeConflict`](entities/rustsymbol/me/mergeconflict.md)
- `struct` [`MergeReport`](entities/rustsymbol/me/mergereport.md)
- `struct` [`MigrateReport`](entities/rustsymbol/mi/migratereport.md)
- `struct` [`MigrateV3Report`](entities/rustsymbol/mi/migratev3report.md)
- `struct` [`VersionRow`](entities/rustsymbol/ve/versionrow.md)
- `function` [`content_signature`](entities/rustsymbol/co/content-signature.md)
- `function` [`diff_ledger`](entities/rustsymbol/di/diff-ledger.md)
- `function` [`dir_bytes`](entities/rustsymbol/di/dir-bytes.md)
- `function` [`id_value_to_string`](entities/rustsymbol/id/id-value-to-string.md)
- `function` [`init_schema_v2`](entities/rustsymbol/in/init-schema-v2.md)
- `function` [`load_dictionary`](entities/rustsymbol/lo/load-dictionary.md)
- `function` [`merge_branch`](entities/rustsymbol/me/merge-branch.md)
- `function` [`merge_stores`](entities/rustsymbol/me/merge-stores.md)
- `function` [`migrate_to_v2`](entities/rustsymbol/mi/migrate-to-v2.md)
- `function` [`migrate_to_v3`](entities/rustsymbol/mi/migrate-to-v3.md)
- `function` [`payload_samples`](entities/rustsymbol/pa/payload-samples.md)
- `function` [`sibling_path`](entities/rustsymbol/si/sibling-path.md)
- `function` [`sig_value_to_hex`](entities/rustsymbol/si/sig-value-to-hex.md)
- `function` [`ts_value_to_datetime`](entities/rustsymbol/ts/ts-value-to-datetime.md)

## ekos/crates/ledger/src/search.rs

- `struct` [`SearchIndex`](entities/rustsymbol/se/searchindex.md)
- `method` [`SearchIndex::commit`](entities/rustsymbol/se/searchindex-commit.md)
- `method` [`SearchIndex::open`](entities/rustsymbol/se/searchindex-open.md)
- `method` [`SearchIndex::query`](entities/rustsymbol/se/searchindex-query.md)
- `method` [`SearchIndex::upsert`](entities/rustsymbol/se/searchindex-upsert.md)
- `function` [`terr`](entities/rustsymbol/te/terr.md)

## ekos/crates/ledger/src/segment/map.rs

- `struct` [`MappedSegment`](entities/rustsymbol/ma/mappedsegment.md)
- `method` [`MappedSegment::bytes`](entities/rustsymbol/ma/mappedsegment-bytes.md)
- `method` [`MappedSegment::open`](entities/rustsymbol/ma/mappedsegment-open.md)

## ekos/crates/ledger/src/segment/mod.rs

- `struct` [`Batch`](entities/rustsymbol/ba/batch.md)
- `struct` [`Head`](entities/rustsymbol/he/head.md)
- `struct` [`Manifest`](entities/rustsymbol/ma/manifest.md)
- `struct` [`SealedSegment`](entities/rustsymbol/se/sealedsegment.md)
- `struct` [`SegDict`](entities/rustsymbol/se/segdict.md)
- `enum` [`SegmentError`](entities/rustsymbol/se/segmenterror.md)
- `struct` [`SegmentStore`](entities/rustsymbol/se/segmentstore.md)
- `method` [`SegmentStore::active_batches`](entities/rustsymbol/se/segmentstore-active-batches.md)
- `method` [`SegmentStore::append`](entities/rustsymbol/se/segmentstore-append.md)
- `method` [`SegmentStore::append_with_seal`](entities/rustsymbol/se/segmentstore-append-with-seal.md)
- `method` [`SegmentStore::batch_headers`](entities/rustsymbol/se/segmentstore-batch-headers.md)
- `method` [`SegmentStore::batches`](entities/rustsymbol/se/segmentstore-batches.md)
- `method` [`SegmentStore::batches_after`](entities/rustsymbol/se/segmentstore-batches-after.md)
- `method` [`SegmentStore::committed_len`](entities/rustsymbol/se/segmentstore-committed-len.md)
- `method` [`SegmentStore::encode_frame`](entities/rustsymbol/se/segmentstore-encode-frame.md)
- `method` [`SegmentStore::next_tx`](entities/rustsymbol/se/segmentstore-next-tx.md)
- `method` [`SegmentStore::open`](entities/rustsymbol/se/segmentstore-open.md)
- `method` [`SegmentStore::open_with_seal_threshold`](entities/rustsymbol/se/segmentstore-open-with-seal-threshold.md)
- `method` [`SegmentStore::persist_manifest`](entities/rustsymbol/se/segmentstore-persist-manifest.md)
- `method` [`SegmentStore::read_active_committed`](entities/rustsymbol/se/segmentstore-read-active-committed.md)
- `method` [`SegmentStore::root`](entities/rustsymbol/se/segmentstore-root.md)
- `method` [`SegmentStore::seal_active`](entities/rustsymbol/se/segmentstore-seal-active.md)
- `method` [`SegmentStore::set_dictionary`](entities/rustsymbol/se/segmentstore-set-dictionary.md)
- `method` [`SegmentStore::verify_sealed`](entities/rustsymbol/se/segmentstore-verify-sealed.md)
- `function` [`atomic_write`](entities/rustsymbol/at/atomic-write.md)
- `function` [`build_dict`](entities/rustsymbol/bu/build-dict.md)
- `function` [`decode_frame`](entities/rustsymbol/de/decode-frame.md)
- `function` [`decode_header`](entities/rustsymbol/de/decode-header.md)
- `function` [`hash_file`](entities/rustsymbol/ha/hash-file.md)
- `function` [`load_manifest`](entities/rustsymbol/lo/load-manifest.md)
- `function` [`save_manifest`](entities/rustsymbol/sa/save-manifest.md)
- `function` [`scan_batches_filtered`](entities/rustsymbol/sc/scan-batches-filtered.md)
- `function` [`scan_headers_slice`](entities/rustsymbol/sc/scan-headers-slice.md)
- `function` [`scan_slice`](entities/rustsymbol/sc/scan-slice.md)
- `function` [`segment_path`](entities/rustsymbol/se/segment-path.md)
- `function` [`walk_frames`](entities/rustsymbol/wa/walk-frames.md)
- `function` [`write_head`](entities/rustsymbol/wr/write-head.md)

## ekos/crates/ledger/tests/estate_migration.rs

- `function` [`dir_bytes`](entities/rustsymbol/di/dir-bytes-a1c5e8ff.md)
- `function` [`mb`](entities/rustsymbol/mb/mb.md)
- `function` [`migrate_estate_and_report_sizes`](entities/rustsymbol/mi/migrate-estate-and-report-sizes.md)

## ekos/crates/marketing/src/devlog.rs

- `enum` [`DevlogParseError`](entities/rustsymbol/de/devlogparseerror.md)
- `struct` [`DevlogSummary`](entities/rustsymbol/de/devlogsummary.md)
- `function` [`extract_section`](entities/rustsymbol/ex/extract-section.md)
- `function` [`find_latest`](entities/rustsymbol/fi/find-latest.md)
- `function` [`number_from_filename`](entities/rustsymbol/nu/number-from-filename.md)
- `function` [`parse`](entities/rustsymbol/pa/parse.md)
- `function` [`split_once_any_dash`](entities/rustsymbol/sp/split-once-any-dash.md)

## ekos/crates/marketing/src/importance.rs

- `enum` [`Importance`](entities/rustsymbol/im/importance.md)
- `function` [`classify`](entities/rustsymbol/cl/classify.md)

## ekos/crates/marketing/src/oauth1.rs

- `struct` [`OauthCredentials`](entities/rustsymbol/oa/oauthcredentials.md)
- `function` [`authorization_header`](entities/rustsymbol/au/authorization-header.md)
- `function` [`generate_nonce`](entities/rustsymbol/ge/generate-nonce.md)
- `function` [`normalized_param_string`](entities/rustsymbol/no/normalized-param-string.md)
- `function` [`percent_encode`](entities/rustsymbol/pe/percent-encode.md)
- `function` [`sign`](entities/rustsymbol/si/sign.md)
- `function` [`signature_base_string`](entities/rustsymbol/si/signature-base-string.md)
- `function` [`unix_timestamp`](entities/rustsymbol/un/unix-timestamp.md)

## ekos/crates/marketing/src/prompt.rs

- `function` [`build_retry_suffix`](entities/rustsymbol/bu/build-retry-suffix.md)
- `function` [`build_user_prompt`](entities/rustsymbol/bu/build-user-prompt.md)
- `function` [`overage_from_too_long_reason`](entities/rustsymbol/ov/overage-from-too-long-reason.md)

## ekos/crates/marketing/src/publisher.rs

- `struct` [`NoopPublisher`](entities/rustsymbol/no/nooppublisher.md)
- `method` [`NoopPublisher::publish`](entities/rustsymbol/no/nooppublisher-publish.md)
- `enum` [`PublishError`](entities/rustsymbol/pu/publisherror.md)
- `trait` [`Publisher`](entities/rustsymbol/pu/publisher.md)
- `struct` [`TweetCreateData`](entities/rustsymbol/tw/tweetcreatedata.md)
- `struct` [`TweetCreateResponse`](entities/rustsymbol/tw/tweetcreateresponse.md)
- `struct` [`TwitterPublisher`](entities/rustsymbol/tw/twitterpublisher.md)
- `method` [`TwitterPublisher::from_env`](entities/rustsymbol/tw/twitterpublisher-from-env.md)
- `method` [`TwitterPublisher::new`](entities/rustsymbol/tw/twitterpublisher-new.md)
- `method` [`TwitterPublisher::publish`](entities/rustsymbol/tw/twitterpublisher-publish.md)

## ekos/crates/marketing/src/store.rs

- `struct` [`PostedStore`](entities/rustsymbol/po/postedstore.md)
- `method` [`PostedStore::is_posted`](entities/rustsymbol/po/postedstore-is-posted.md)
- `method` [`PostedStore::load`](entities/rustsymbol/po/postedstore-load.md)
- `method` [`PostedStore::record`](entities/rustsymbol/po/postedstore-record.md)
- `method` [`PostedStore::save`](entities/rustsymbol/po/postedstore-save.md)
- `struct` [`PostedTweet`](entities/rustsymbol/po/postedtweet.md)
- `enum` [`StoreError`](entities/rustsymbol/st/storeerror-e1b41824.md)

## ekos/crates/marketing/src/tweet.rs

- `struct` [`LlmTweetOutput`](entities/rustsymbol/ll/llmtweetoutput.md)
- `enum` [`MarketingError`](entities/rustsymbol/ma/marketingerror.md)
- `struct` [`TweetDraft`](entities/rustsymbol/tw/tweetdraft.md)
- `enum` [`TweetValidationError`](entities/rustsymbol/tw/tweetvalidationerror.md)
- `function` [`draft_once`](entities/rustsymbol/dr/draft-once.md)
- `function` [`generate_tweet`](entities/rustsymbol/ge/generate-tweet.md)
- `function` [`validate_tweet`](entities/rustsymbol/va/validate-tweet.md)

## ekos/crates/observation-sdk/src/lib.rs

- `struct` [`ConnectorConfig`](entities/rustsymbol/co/connectorconfig.md)
- `method` [`ConnectorConfig::get_bool`](entities/rustsymbol/co/connectorconfig-get-bool.md)
- `method` [`ConnectorConfig::get_str`](entities/rustsymbol/co/connectorconfig-get-str.md)
- `struct` [`Fingerprint`](entities/rustsymbol/fi/fingerprint.md)
- `struct` [`ObservationPackage`](entities/rustsymbol/ob/observationpackage.md)
- `method` [`ObservationPackage::is_empty`](entities/rustsymbol/ob/observationpackage-is-empty.md)
- `method` [`ObservationPackage::len`](entities/rustsymbol/ob/observationpackage-len.md)
- `method` [`ObservationPackage::new`](entities/rustsymbol/ob/observationpackage-new.md)
- `method` [`ObservationPackage::push`](entities/rustsymbol/ob/observationpackage-push.md)
- `enum` [`ObserveError`](entities/rustsymbol/ob/observeerror.md)
- `method` [`ObserveError::connector`](entities/rustsymbol/ob/observeerror-connector.md)
- `trait` [`Observer`](entities/rustsymbol/ob/observer.md)
- `struct` [`PackageMeta`](entities/rustsymbol/pa/packagemeta.md)
- `struct` [`ScanContext`](entities/rustsymbol/sc/scancontext.md)
- `method` [`ScanContext::is_ignored`](entities/rustsymbol/sc/scancontext-is-ignored.md)
- `method` [`ScanContext::new`](entities/rustsymbol/sc/scancontext-new.md)
- `method` [`ScanContext::with_config`](entities/rustsymbol/sc/scancontext-with-config.md)
- `method` [`ScanContext::with_ignore_patterns`](entities/rustsymbol/sc/scancontext-with-ignore-patterns.md)
- `function` [`source_fingerprint`](entities/rustsymbol/so/source-fingerprint.md)

## ekos/crates/recovery/src/anthropic.rs

- `struct` [`AnthropicProvider`](entities/rustsymbol/an/anthropicprovider.md)
- `method` [`AnthropicProvider::complete`](entities/rustsymbol/an/anthropicprovider-complete.md)
- `method` [`AnthropicProvider::from_env`](entities/rustsymbol/an/anthropicprovider-from-env.md)
- `method` [`AnthropicProvider::from_env_var`](entities/rustsymbol/an/anthropicprovider-from-env-var.md)
- `method` [`AnthropicProvider::model_name`](entities/rustsymbol/an/anthropicprovider-model-name.md)
- `method` [`AnthropicProvider::new`](entities/rustsymbol/an/anthropicprovider-new.md)
- `struct` [`ApiContent`](entities/rustsymbol/ap/apicontent.md)
- `struct` [`ApiMessage`](entities/rustsymbol/ap/apimessage.md)
- `struct` [`ApiRequest`](entities/rustsymbol/ap/apirequest-d7b913bf.md)
- `struct` [`ApiResponse`](entities/rustsymbol/ap/apiresponse.md)
- `struct` [`ApiUsage`](entities/rustsymbol/ap/apiusage.md)

## ekos/crates/recovery/src/cache.rs

- `struct` [`CachedLlmProvider`](entities/rustsymbol/ca/cachedllmprovider.md)
- `method` [`CachedLlmProvider::cache_root`](entities/rustsymbol/ca/cachedllmprovider-cache-root.md)
- `method` [`CachedLlmProvider::complete`](entities/rustsymbol/ca/cachedllmprovider-complete.md)
- `method` [`CachedLlmProvider::model_name`](entities/rustsymbol/ca/cachedllmprovider-model-name.md)
- `method` [`CachedLlmProvider::new`](entities/rustsymbol/ca/cachedllmprovider-new.md)
- `function` [`cache_key`](entities/rustsymbol/ca/cache-key.md)
- `function` [`cache_path`](entities/rustsymbol/ca/cache-path.md)

## ekos/crates/recovery/src/cicd_analyzer.rs

- `struct` [`CicdAnalyzerPass`](entities/rustsymbol/ci/cicdanalyzerpass.md)
- `method` [`CicdAnalyzerPass::cache_inputs`](entities/rustsymbol/ci/cicdanalyzerpass-cache-inputs.md)
- `method` [`CicdAnalyzerPass::name`](entities/rustsymbol/ci/cicdanalyzerpass-name.md)
- `method` [`CicdAnalyzerPass::new`](entities/rustsymbol/ci/cicdanalyzerpass-new.md)
- `method` [`CicdAnalyzerPass::run`](entities/rustsymbol/ci/cicdanalyzerpass-run.md)
- `function` [`extract_jobs`](entities/rustsymbol/ex/extract-jobs.md)
- `function` [`extract_triggers`](entities/rustsymbol/ex/extract-triggers.md)
- `function` [`pipeline_kir_id`](entities/rustsymbol/pi/pipeline-kir-id.md)

## ekos/crates/recovery/src/confluence_analyzer.rs

- `struct` [`ConfluenceAnalyzerPass`](entities/rustsymbol/co/confluenceanalyzerpass.md)
- `method` [`ConfluenceAnalyzerPass::cache_inputs`](entities/rustsymbol/co/confluenceanalyzerpass-cache-inputs.md)
- `method` [`ConfluenceAnalyzerPass::name`](entities/rustsymbol/co/confluenceanalyzerpass-name.md)
- `method` [`ConfluenceAnalyzerPass::new`](entities/rustsymbol/co/confluenceanalyzerpass-new.md)
- `method` [`ConfluenceAnalyzerPass::run`](entities/rustsymbol/co/confluenceanalyzerpass-run.md)
- `struct` [`PageData`](entities/rustsymbol/pa/pagedata.md)
- `function` [`body_excerpt`](entities/rustsymbol/bo/body-excerpt.md)
- `function` [`find_linked_titles`](entities/rustsymbol/fi/find-linked-titles.md)
- `function` [`page_kir_id`](entities/rustsymbol/pa/page-kir-id.md)

## ekos/crates/recovery/src/crate_topology_analyzer.rs

- `struct` [`CrateTopologyAnalyzerPass`](entities/rustsymbol/cr/cratetopologyanalyzerpass.md)
- `method` [`CrateTopologyAnalyzerPass::cache_inputs`](entities/rustsymbol/cr/cratetopologyanalyzerpass-cache-inputs.md)
- `method` [`CrateTopologyAnalyzerPass::name`](entities/rustsymbol/cr/cratetopologyanalyzerpass-name.md)
- `method` [`CrateTopologyAnalyzerPass::new`](entities/rustsymbol/cr/cratetopologyanalyzerpass-new.md)
- `method` [`CrateTopologyAnalyzerPass::run`](entities/rustsymbol/cr/cratetopologyanalyzerpass-run.md)
- `enum` [`DepResolution`](entities/rustsymbol/de/depresolution.md)
- `enum` [`WorkspaceDep`](entities/rustsymbol/wo/workspacedep.md)
- `function` [`crate_kir_id`](entities/rustsymbol/cr/crate-kir-id.md)
- `function` [`normalize_rel_path`](entities/rustsymbol/no/normalize-rel-path.md)
- `function` [`resolve_dep_entry`](entities/rustsymbol/re/resolve-dep-entry.md)
- `function` [`technology_kir_id`](entities/rustsymbol/te/technology-kir-id-84387622.md)

## ekos/crates/recovery/src/crypto_analyzer.rs

- `struct` [`BatchData`](entities/rustsymbol/ba/batchdata.md)
- `struct` [`CryptoAnalyzerPass`](entities/rustsymbol/cr/cryptoanalyzerpass.md)
- `method` [`CryptoAnalyzerPass::cache_inputs`](entities/rustsymbol/cr/cryptoanalyzerpass-cache-inputs.md)
- `method` [`CryptoAnalyzerPass::name`](entities/rustsymbol/cr/cryptoanalyzerpass-name.md)
- `method` [`CryptoAnalyzerPass::new`](entities/rustsymbol/cr/cryptoanalyzerpass-new.md)
- `method` [`CryptoAnalyzerPass::run`](entities/rustsymbol/cr/cryptoanalyzerpass-run.md)
- `struct` [`EntityRow`](entities/rustsymbol/en/entityrow.md)
- `struct` [`EvidenceRow`](entities/rustsymbol/ev/evidencerow-21ccac89.md)
- `struct` [`RelationshipRow`](entities/rustsymbol/re/relationshiprow-5e78a376.md)
- `function` [`deterministic_id`](entities/rustsymbol/de/deterministic-id.md)
- `function` [`parse_attrs`](entities/rustsymbol/pa/parse-attrs.md)

## ekos/crates/recovery/src/dependency_analyzer.rs

- `struct` [`DependencyAnalyzerPass`](entities/rustsymbol/de/dependencyanalyzerpass.md)
- `method` [`DependencyAnalyzerPass::cache_inputs`](entities/rustsymbol/de/dependencyanalyzerpass-cache-inputs.md)
- `method` [`DependencyAnalyzerPass::name`](entities/rustsymbol/de/dependencyanalyzerpass-name.md)
- `method` [`DependencyAnalyzerPass::new`](entities/rustsymbol/de/dependencyanalyzerpass-new.md)
- `method` [`DependencyAnalyzerPass::run`](entities/rustsymbol/de/dependencyanalyzerpass-run.md)
- `function` [`file_kir_id`](entities/rustsymbol/fi/file-kir-id.md)
- `function` [`technology_kir_id`](entities/rustsymbol/te/technology-kir-id.md)

## ekos/crates/recovery/src/document_semantics_analyzer.rs

- `struct` [`DocumentSemanticsAnalyzerPass`](entities/rustsymbol/do/documentsemanticsanalyzerpass.md)
- `method` [`DocumentSemanticsAnalyzerPass::collect_sections`](entities/rustsymbol/do/documentsemanticsanalyzerpass-collect-sections.md)
- `method` [`DocumentSemanticsAnalyzerPass::dependencies`](entities/rustsymbol/do/documentsemanticsanalyzerpass-dependencies.md)
- `method` [`DocumentSemanticsAnalyzerPass::name`](entities/rustsymbol/do/documentsemanticsanalyzerpass-name.md)
- `method` [`DocumentSemanticsAnalyzerPass::new`](entities/rustsymbol/do/documentsemanticsanalyzerpass-new.md)
- `method` [`DocumentSemanticsAnalyzerPass::run`](entities/rustsymbol/do/documentsemanticsanalyzerpass-run.md)
- `method` [`DocumentSemanticsAnalyzerPass::stats_handle`](entities/rustsymbol/do/documentsemanticsanalyzerpass-stats-handle.md)
- `method` [`DocumentSemanticsAnalyzerPass::with_max_sections`](entities/rustsymbol/do/documentsemanticsanalyzerpass-with-max-sections.md)
- `struct` [`DocumentSemanticsStats`](entities/rustsymbol/do/documentsemanticsstats.md)
- `struct` [`LlmConcept`](entities/rustsymbol/ll/llmconcept.md)
- `struct` [`LlmOutput`](entities/rustsymbol/ll/llmoutput.md)
- `struct` [`LlmRelationship`](entities/rustsymbol/ll/llmrelationship-a8e764aa.md)
- `struct` [`SectionInput`](entities/rustsymbol/se/sectioninput.md)
- `function` [`concept_kir_id`](entities/rustsymbol/co/concept-kir-id.md)
- `function` [`normalize_concept_name`](entities/rustsymbol/no/normalize-concept-name.md)
- `function` [`sections_from_graph`](entities/rustsymbol/se/sections-from-graph.md)

## ekos/crates/recovery/src/git_analyzer.rs

- `struct` [`GitAnalyzerPass`](entities/rustsymbol/gi/gitanalyzerpass.md)
- `method` [`GitAnalyzerPass::cache_inputs`](entities/rustsymbol/gi/gitanalyzerpass-cache-inputs.md)
- `method` [`GitAnalyzerPass::name`](entities/rustsymbol/gi/gitanalyzerpass-name.md)
- `method` [`GitAnalyzerPass::new`](entities/rustsymbol/gi/gitanalyzerpass-new.md)
- `method` [`GitAnalyzerPass::run`](entities/rustsymbol/gi/gitanalyzerpass-run.md)
- `method` [`GitAnalyzerPass::version`](entities/rustsymbol/gi/gitanalyzerpass-version.md)
- `method` [`GitAnalyzerPass::with_max_coupling_commit_files`](entities/rustsymbol/gi/gitanalyzerpass-with-max-coupling-commit-files.md)
- `method` [`GitAnalyzerPass::with_min_coupling`](entities/rustsymbol/gi/gitanalyzerpass-with-min-coupling.md)
- `function` [`contributor_kir_id`](entities/rustsymbol/co/contributor-kir-id.md)

## ekos/crates/recovery/src/github_analyzer.rs

- `struct` [`GitHubAnalyzerPass`](entities/rustsymbol/gi/githubanalyzerpass.md)
- `method` [`GitHubAnalyzerPass::cache_inputs`](entities/rustsymbol/gi/githubanalyzerpass-cache-inputs.md)
- `method` [`GitHubAnalyzerPass::name`](entities/rustsymbol/gi/githubanalyzerpass-name.md)
- `method` [`GitHubAnalyzerPass::new`](entities/rustsymbol/gi/githubanalyzerpass-new.md)
- `method` [`GitHubAnalyzerPass::run`](entities/rustsymbol/gi/githubanalyzerpass-run.md)
- `struct` [`ItemData`](entities/rustsymbol/it/itemdata.md)
- `function` [`body_excerpt`](entities/rustsymbol/bo/body-excerpt-4f4ffc8a.md)
- `function` [`file_kir_id`](entities/rustsymbol/fi/file-kir-id-d36e01ce.md)
- `function` [`find_closed_issue_numbers`](entities/rustsymbol/fi/find-closed-issue-numbers.md)
- `function` [`item_kir_id`](entities/rustsymbol/it/item-kir-id.md)

## ekos/crates/recovery/src/llm.rs

- `enum` [`LlmError`](entities/rustsymbol/ll/llmerror.md)
- `method` [`LlmError::other`](entities/rustsymbol/ll/llmerror-other.md)
- `trait` [`LlmProvider`](entities/rustsymbol/ll/llmprovider.md)
- `struct` [`LlmRequest`](entities/rustsymbol/ll/llmrequest.md)
- `struct` [`LlmResponse`](entities/rustsymbol/ll/llmresponse.md)
- `struct` [`MockLlmProvider`](entities/rustsymbol/mo/mockllmprovider.md)
- `method` [`MockLlmProvider::complete`](entities/rustsymbol/mo/mockllmprovider-complete.md)
- `method` [`MockLlmProvider::model_name`](entities/rustsymbol/mo/mockllmprovider-model-name.md)
- `method` [`MockLlmProvider::new`](entities/rustsymbol/mo/mockllmprovider-new.md)

## ekos/crates/recovery/src/llm_json.rs

- `function` [`strip_json_fences`](entities/rustsymbol/st/strip-json-fences.md)

## ekos/crates/recovery/src/local_docs_analyzer.rs

- `struct` [`DocumentData`](entities/rustsymbol/do/documentdata.md)
- `struct` [`LocalDocAnalyzerPass`](entities/rustsymbol/lo/localdocanalyzerpass.md)
- `method` [`LocalDocAnalyzerPass::cache_inputs`](entities/rustsymbol/lo/localdocanalyzerpass-cache-inputs.md)
- `method` [`LocalDocAnalyzerPass::name`](entities/rustsymbol/lo/localdocanalyzerpass-name.md)
- `method` [`LocalDocAnalyzerPass::new`](entities/rustsymbol/lo/localdocanalyzerpass-new.md)
- `method` [`LocalDocAnalyzerPass::run`](entities/rustsymbol/lo/localdocanalyzerpass-run.md)
- `struct` [`SectionData`](entities/rustsymbol/se/sectiondata.md)
- `struct` [`TableData`](entities/rustsymbol/ta/tabledata.md)
- `function` [`document_kir_id`](entities/rustsymbol/do/document-kir-id.md)
- `function` [`section_kir_id`](entities/rustsymbol/se/section-kir-id.md)
- `function` [`table_kir_id`](entities/rustsymbol/ta/table-kir-id.md)

## ekos/crates/recovery/src/ollama.rs

- `struct` [`ApiMessage`](entities/rustsymbol/ap/apimessage-e405f66b.md)
- `struct` [`ApiOptions`](entities/rustsymbol/ap/apioptions.md)
- `struct` [`ApiRequest`](entities/rustsymbol/ap/apirequest.md)
- `struct` [`ApiResponse`](entities/rustsymbol/ap/apiresponse-57f09378.md)
- `struct` [`ApiResponseMessage`](entities/rustsymbol/ap/apiresponsemessage.md)
- `struct` [`OllamaProvider`](entities/rustsymbol/ol/ollamaprovider.md)
- `method` [`OllamaProvider::build_request`](entities/rustsymbol/ol/ollamaprovider-build-request.md)
- `method` [`OllamaProvider::complete`](entities/rustsymbol/ol/ollamaprovider-complete.md)
- `method` [`OllamaProvider::from_env`](entities/rustsymbol/ol/ollamaprovider-from-env.md)
- `method` [`OllamaProvider::model_name`](entities/rustsymbol/ol/ollamaprovider-model-name.md)
- `method` [`OllamaProvider::new`](entities/rustsymbol/ol/ollamaprovider-new.md)

## ekos/crates/recovery/src/pentaho_analyzer.rs

- `struct` [`PentahoAnalyzerPass`](entities/rustsymbol/pe/pentahoanalyzerpass.md)
- `method` [`PentahoAnalyzerPass::cache_inputs`](entities/rustsymbol/pe/pentahoanalyzerpass-cache-inputs.md)
- `method` [`PentahoAnalyzerPass::name`](entities/rustsymbol/pe/pentahoanalyzerpass-name.md)
- `method` [`PentahoAnalyzerPass::new`](entities/rustsymbol/pe/pentahoanalyzerpass-new.md)
- `method` [`PentahoAnalyzerPass::run`](entities/rustsymbol/pe/pentahoanalyzerpass-run.md)
- `method` [`PentahoAnalyzerPass::stats_handle`](entities/rustsymbol/pe/pentahoanalyzerpass-stats-handle.md)
- `struct` [`PentahoArtifactData`](entities/rustsymbol/pe/pentahoartifactdata.md)
- `struct` [`PentahoStats`](entities/rustsymbol/pe/pentahostats.md)
- `method` [`PentahoStats::coverage_percent`](entities/rustsymbol/pe/pentahostats-coverage-percent.md)
- `function` [`child_text`](entities/rustsymbol/ch/child-text.md)
- `function` [`extract_calculator`](entities/rustsymbol/ex/extract-calculator.md)
- `function` [`extract_filter_condition`](entities/rustsymbol/ex/extract-filter-condition.md)
- `function` [`extract_group_by`](entities/rustsymbol/ex/extract-group-by.md)
- `function` [`extract_join`](entities/rustsymbol/ex/extract-join.md)
- `function` [`extract_join_keys`](entities/rustsymbol/ex/extract-join-keys.md)
- `function` [`extract_stream_lookup`](entities/rustsymbol/ex/extract-stream-lookup.md)
- `function` [`extract_table_from_sql`](entities/rustsymbol/ex/extract-table-from-sql.md)
- `function` [`map_step`](entities/rustsymbol/ma/map-step.md)
- `function` [`parse_kettle_xml`](entities/rustsymbol/pa/parse-kettle-xml.md)
- `function` [`parse_kjb`](entities/rustsymbol/pa/parse-kjb.md)
- `function` [`parse_ktr`](entities/rustsymbol/pa/parse-ktr.md)
- `function` [`xml_slice`](entities/rustsymbol/xm/xml-slice.md)

## ekos/crates/recovery/src/python_analyzer.rs

- `struct` [`PythonAnalyzerPass`](entities/rustsymbol/py/pythonanalyzerpass.md)
- `method` [`PythonAnalyzerPass::cache_inputs`](entities/rustsymbol/py/pythonanalyzerpass-cache-inputs.md)
- `method` [`PythonAnalyzerPass::name`](entities/rustsymbol/py/pythonanalyzerpass-name.md)
- `method` [`PythonAnalyzerPass::new`](entities/rustsymbol/py/pythonanalyzerpass-new.md)
- `method` [`PythonAnalyzerPass::run`](entities/rustsymbol/py/pythonanalyzerpass-run.md)
- `method` [`PythonAnalyzerPass::stats_handle`](entities/rustsymbol/py/pythonanalyzerpass-stats-handle.md)
- `struct` [`PythonArtifactData`](entities/rustsymbol/py/pythonartifactdata.md)
- `struct` [`PythonFileResult`](entities/rustsymbol/py/pythonfileresult.md)
- `struct` [`PythonStats`](entities/rustsymbol/py/pythonstats.md)
- `method` [`PythonStats::coverage_percent`](entities/rustsymbol/py/pythonstats-coverage-percent.md)
- `struct` [`RawCall`](entities/rustsymbol/ra/rawcall.md)
- `function` [`add_import`](entities/rustsymbol/ad/add-import-89c6ca8d.md)
- `function` [`add_symbol`](entities/rustsymbol/ad/add-symbol-458e9ef2.md)
- `function` [`agg_expr_from_arg`](entities/rustsymbol/ag/agg-expr-from-arg.md)
- `function` [`calls_to_nodes`](entities/rustsymbol/ca/calls-to-nodes.md)
- `function` [`join_keys_from_on`](entities/rustsymbol/jo/join-keys-from-on.md)
- `function` [`join_kind_from_how`](entities/rustsymbol/jo/join-kind-from-how.md)
- `function` [`keyword_arg`](entities/rustsymbol/ke/keyword-arg.md)
- `function` [`linearize_chain`](entities/rustsymbol/li/linearize-chain.md)
- `function` [`parse_python_file`](entities/rustsymbol/pa/parse-python-file.md)
- `function` [`positional_string_arg`](entities/rustsymbol/po/positional-string-arg.md)
- `function` [`python_module_kir_id`](entities/rustsymbol/py/python-module-kir-id.md)
- `function` [`source_slice`](entities/rustsymbol/so/source-slice.md)
- `function` [`string_constant`](entities/rustsymbol/st/string-constant.md)
- `function` [`try_recognize_chain_statement`](entities/rustsymbol/tr/try-recognize-chain-statement.md)
- `function` [`walk_top_level_statement`](entities/rustsymbol/wa/walk-top-level-statement.md)

## ekos/crates/recovery/src/rust_analyzer.rs

- `struct` [`CallVisitor`](entities/rustsymbol/ca/callvisitor.md)
- `method` [`CallVisitor::visit_expr_call`](entities/rustsymbol/ca/callvisitor-visit-expr-call.md)
- `method` [`CallVisitor::visit_expr_method_call`](entities/rustsymbol/ca/callvisitor-visit-expr-method-call.md)
- `struct` [`RustAnalyzerPass`](entities/rustsymbol/ru/rustanalyzerpass.md)
- `method` [`RustAnalyzerPass::cache_inputs`](entities/rustsymbol/ru/rustanalyzerpass-cache-inputs.md)
- `method` [`RustAnalyzerPass::name`](entities/rustsymbol/ru/rustanalyzerpass-name.md)
- `method` [`RustAnalyzerPass::new`](entities/rustsymbol/ru/rustanalyzerpass-new.md)
- `method` [`RustAnalyzerPass::run`](entities/rustsymbol/ru/rustanalyzerpass-run.md)
- `method` [`RustAnalyzerPass::stats_handle`](entities/rustsymbol/ru/rustanalyzerpass-stats-handle.md)
- `struct` [`RustArtifactData`](entities/rustsymbol/ru/rustartifactdata.md)
- `struct` [`RustFileResult`](entities/rustsymbol/ru/rustfileresult.md)
- `struct` [`RustStats`](entities/rustsymbol/ru/ruststats.md)
- `function` [`add_import`](entities/rustsymbol/ad/add-import.md)
- `function` [`add_symbol`](entities/rustsymbol/ad/add-symbol.md)
- `function` [`flatten_use_tree`](entities/rustsymbol/fl/flatten-use-tree.md)
- `function` [`parse_rust_file`](entities/rustsymbol/pa/parse-rust-file.md)
- `function` [`rust_module_kir_id`](entities/rustsymbol/ru/rust-module-kir-id.md)
- `function` [`type_name`](entities/rustsymbol/ty/type-name-b2c88510.md)

## ekos/crates/recovery/src/sql_analyzer.rs

- `struct` [`LlmEntity`](entities/rustsymbol/ll/llmentity.md)
- `struct` [`LlmOutput`](entities/rustsymbol/ll/llmoutput-771440cb.md)
- `struct` [`LlmRelationship`](entities/rustsymbol/ll/llmrelationship.md)
- `struct` [`SqlAnalyzerPass`](entities/rustsymbol/sq/sqlanalyzerpass.md)
- `method` [`SqlAnalyzerPass::cache_inputs`](entities/rustsymbol/sq/sqlanalyzerpass-cache-inputs.md)
- `method` [`SqlAnalyzerPass::name`](entities/rustsymbol/sq/sqlanalyzerpass-name.md)
- `method` [`SqlAnalyzerPass::new`](entities/rustsymbol/sq/sqlanalyzerpass-new.md)
- `method` [`SqlAnalyzerPass::run`](entities/rustsymbol/sq/sqlanalyzerpass-run.md)
- `function` [`add_fk_relationship`](entities/rustsymbol/ad/add-fk-relationship.md)
- `function` [`apply_llm_enrichment`](entities/rustsymbol/ap/apply-llm-enrichment.md)
- `function` [`col_names`](entities/rustsymbol/co/col-names.md)
- `function` [`columns_json`](entities/rustsymbol/co/columns-json.md)
- `function` [`parse_ddl_structural`](entities/rustsymbol/pa/parse-ddl-structural.md)

## ekos/crates/recovery/src/sql_dialect_registry.rs

- `struct` [`DialectRule`](entities/rustsymbol/di/dialectrule.md)
- `struct` [`GenericDialectParser`](entities/rustsymbol/ge/genericdialectparser.md)
- `method` [`GenericDialectParser::name`](entities/rustsymbol/ge/genericdialectparser-name.md)
- `method` [`GenericDialectParser::sqlparser_dialect`](entities/rustsymbol/ge/genericdialectparser-sqlparser-dialect.md)
- `function` [`build_dialect_registry`](entities/rustsymbol/bu/build-dialect-registry.md)
- `function` [`resolve_dialect_name`](entities/rustsymbol/re/resolve-dialect-name.md)

## ekos/crates/recovery/src/sql_transform_analyzer.rs

- `struct` [`SqlTransformAnalyzerPass`](entities/rustsymbol/sq/sqltransformanalyzerpass.md)
- `method` [`SqlTransformAnalyzerPass::cache_inputs`](entities/rustsymbol/sq/sqltransformanalyzerpass-cache-inputs.md)
- `method` [`SqlTransformAnalyzerPass::name`](entities/rustsymbol/sq/sqltransformanalyzerpass-name.md)
- `method` [`SqlTransformAnalyzerPass::new`](entities/rustsymbol/sq/sqltransformanalyzerpass-new.md)
- `method` [`SqlTransformAnalyzerPass::run`](entities/rustsymbol/sq/sqltransformanalyzerpass-run.md)
- `method` [`SqlTransformAnalyzerPass::stats_handle`](entities/rustsymbol/sq/sqltransformanalyzerpass-stats-handle.md)
- `struct` [`SqlTransformStats`](entities/rustsymbol/sq/sqltransformstats.md)
- `method` [`SqlTransformStats::coverage_percent`](entities/rustsymbol/sq/sqltransformstats-coverage-percent.md)
- `function` [`append_fragment`](entities/rustsymbol/ap/append-fragment.md)
- `function` [`as_aggregate_function`](entities/rustsymbol/as/as-aggregate-function.md)
- `function` [`calculated_projection`](entities/rustsymbol/ca/calculated-projection.md)
- `function` [`collect_equi_keys`](entities/rustsymbol/co/collect-equi-keys.md)
- `function` [`dispatch_one_statement`](entities/rustsymbol/di/dispatch-one-statement.md)
- `function` [`extract_aggregates`](entities/rustsymbol/ex/extract-aggregates.md)
- `function` [`extract_equi_keys`](entities/rustsymbol/ex/extract-equi-keys.md)
- `function` [`function_body_text`](entities/rustsymbol/fu/function-body-text.md)
- `function` [`function_to_graph`](entities/rustsymbol/fu/function-to-graph.md)
- `function` [`is_plain_column`](entities/rustsymbol/is/is-plain-column.md)
- `function` [`join_node`](entities/rustsymbol/jo/join-node.md)
- `function` [`parse_sql_statement_by_statement`](entities/rustsymbol/pa/parse-sql-statement-by-statement.md)
- `function` [`parse_sql_to_transform_graphs`](entities/rustsymbol/pa/parse-sql-to-transform-graphs.md)
- `function` [`procedure_body_to_graph`](entities/rustsymbol/pr/procedure-body-to-graph.md)
- `function` [`push`](entities/rustsymbol/pu/push.md)
- `function` [`query_to_graph`](entities/rustsymbol/qu/query-to-graph.md)
- `function` [`select_to_graph`](entities/rustsymbol/se/select-to-graph.md)
- `function` [`source_kind_for`](entities/rustsymbol/so/source-kind-for.md)
- `function` [`table_factor_node`](entities/rustsymbol/ta/table-factor-node.md)

## ekos/crates/recovery/src/statement_repair.rs

- `function` [`ends_with_set_op_keyword`](entities/rustsymbol/en/ends-with-set-op-keyword.md)
- `function` [`ensure_statement_separators`](entities/rustsymbol/en/ensure-statement-separators.md)
- `function` [`starts_with_keyword`](entities/rustsymbol/st/starts-with-keyword.md)

## ekos/crates/runtime/src/ai.rs

- `struct` [`AiAnswer`](entities/rustsymbol/ai/aianswer.md)
- `enum` [`AiError`](entities/rustsymbol/ai/aierror.md)
- `struct` [`AiRuntime`](entities/rustsymbol/ai/airuntime.md)
- `method` [`AiRuntime::ask`](entities/rustsymbol/ai/airuntime-ask.md)
- `method` [`AiRuntime::gather_context`](entities/rustsymbol/ai/airuntime-gather-context.md)
- `method` [`AiRuntime::new`](entities/rustsymbol/ai/airuntime-new.md)
- `struct` [`AiRuntimeConfig`](entities/rustsymbol/ai/airuntimeconfig.md)
- `method` [`AiRuntimeConfig::default`](entities/rustsymbol/ai/airuntimeconfig-default.md)
- `struct` [`CitationBlock`](entities/rustsymbol/ci/citationblock.md)
- `function` [`extract_citations`](entities/rustsymbol/ex/extract-citations.md)

## ekos/crates/runtime/src/lib.rs

- `enum` [`ImpactDirection`](entities/rustsymbol/im/impactdirection.md)
- `struct` [`ImpactHop`](entities/rustsymbol/im/impacthop.md)
- `struct` [`ObjectState`](entities/rustsymbol/ob/objectstate.md)
- `struct` [`Runtime`](entities/rustsymbol/ru/runtime.md)
- `method` [`Runtime::find_objects`](entities/rustsymbol/ru/runtime-find-objects.md)
- `method` [`Runtime::list_objects`](entities/rustsymbol/ru/runtime-list-objects.md)
- `method` [`Runtime::list_relationships`](entities/rustsymbol/ru/runtime-list-relationships.md)
- `method` [`Runtime::load_neighborhood`](entities/rustsymbol/ru/runtime-load-neighborhood.md)
- `method` [`Runtime::load_object`](entities/rustsymbol/ru/runtime-load-object.md)
- `method` [`Runtime::new`](entities/rustsymbol/ru/runtime-new.md)
- `method` [`Runtime::over`](entities/rustsymbol/ru/runtime-over.md)
- `method` [`Runtime::reconstruct_state`](entities/rustsymbol/ru/runtime-reconstruct-state.md)
- `method` [`Runtime::reconstruct_state_at`](entities/rustsymbol/ru/runtime-reconstruct-state-at.md)
- `method` [`Runtime::relationships_for`](entities/rustsymbol/ru/runtime-relationships-for.md)
- `method` [`Runtime::trace_impact`](entities/rustsymbol/ru/runtime-trace-impact.md)
- `enum` [`RuntimeError`](entities/rustsymbol/ru/runtimeerror.md)

## ekos/crates/semantic/src/lib.rs

- `struct` [`CkModel`](entities/rustsymbol/ck/ckmodel.md)
- `method` [`CkModel::validate`](entities/rustsymbol/ck/ckmodel-validate.md)
- `struct` [`CkmObject`](entities/rustsymbol/ck/ckmobject.md)
- `struct` [`CkmRelationship`](entities/rustsymbol/ck/ckmrelationship.md)
- `struct` [`EvidenceRecord`](entities/rustsymbol/ev/evidencerecord-dba444b3.md)
- `struct` [`SemanticCompilerPass`](entities/rustsymbol/se/semanticcompilerpass.md)
- `method` [`SemanticCompilerPass::cache_inputs`](entities/rustsymbol/se/semanticcompilerpass-cache-inputs.md)
- `method` [`SemanticCompilerPass::name`](entities/rustsymbol/se/semanticcompilerpass-name.md)
- `method` [`SemanticCompilerPass::new`](entities/rustsymbol/se/semanticcompilerpass-new.md)
- `method` [`SemanticCompilerPass::run`](entities/rustsymbol/se/semanticcompilerpass-run.md)
- `method` [`SemanticCompilerPass::with_cache_inputs`](entities/rustsymbol/se/semanticcompilerpass-with-cache-inputs.md)
- `function` [`apply_merges`](entities/rustsymbol/ap/apply-merges.md)
- `function` [`build_ckm`](entities/rustsymbol/bu/build-ckm.md)
- `function` [`dedup_relationships`](entities/rustsymbol/de/dedup-relationships.md)
- `function` [`merge_graphs`](entities/rustsymbol/me/merge-graphs.md)

## ekos/crates/semantic/src/transform_ir.rs

- `struct` [`AggExpr`](entities/rustsymbol/ag/aggexpr.md)
- `enum` [`JoinKind`](entities/rustsymbol/jo/joinkind.md)
- `struct` [`NodeId`](entities/rustsymbol/no/nodeid.md)
- `struct` [`TransformGraph`](entities/rustsymbol/tr/transformgraph.md)
- `enum` [`TransformNode`](entities/rustsymbol/tr/transformnode.md)
- `method` [`TransformNode::evidence_fragment`](entities/rustsymbol/tr/transformnode-evidence-fragment.md)
- `method` [`TransformNode::node_type`](entities/rustsymbol/tr/transformnode-node-type.md)
- `method` [`TransformNode::properties`](entities/rustsymbol/tr/transformnode-properties.md)
- `struct` [`TransformOrigin`](entities/rustsymbol/tr/transformorigin.md)
- `function` [`lower_to_kir`](entities/rustsymbol/lo/lower-to-kir.md)
- `function` [`transform_evidence_kir_id`](entities/rustsymbol/tr/transform-evidence-kir-id.md)
- `function` [`transform_node_kir_id`](entities/rustsymbol/tr/transform-node-kir-id.md)

## ekos/crates/sql-dialect-sdk/src/lib.rs

- `trait` [`SqlDialectParser`](entities/rustsymbol/sq/sqldialectparser.md)

## ekos/plugins/confluence/src/lib.rs

- `struct` [`ConfluenceApiClient`](entities/rustsymbol/co/confluenceapiclient.md)
- `method` [`ConfluenceApiClient::list_pages`](entities/rustsymbol/co/confluenceapiclient-list-pages.md)
- `method` [`ConfluenceApiClient::new`](entities/rustsymbol/co/confluenceapiclient-new.md)
- `method` [`ConfluenceApiClient::request`](entities/rustsymbol/co/confluenceapiclient-request.md)
- `trait` [`ConfluenceClient`](entities/rustsymbol/co/confluenceclient.md)
- `enum` [`ConfluenceClientError`](entities/rustsymbol/co/confluenceclienterror.md)
- `struct` [`ConfluenceObserver`](entities/rustsymbol/co/confluenceobserver.md)
- `method` [`ConfluenceObserver::name`](entities/rustsymbol/co/confluenceobserver-name.md)
- `method` [`ConfluenceObserver::new`](entities/rustsymbol/co/confluenceobserver-new.md)
- `method` [`ConfluenceObserver::scan`](entities/rustsymbol/co/confluenceobserver-scan.md)
- `struct` [`ConfluencePage`](entities/rustsymbol/co/confluencepage.md)
- `struct` [`MockConfluenceClient`](entities/rustsymbol/mo/mockconfluenceclient.md)
- `method` [`MockConfluenceClient::list_pages`](entities/rustsymbol/mo/mockconfluenceclient-list-pages.md)
- `method` [`MockConfluenceClient::new`](entities/rustsymbol/mo/mockconfluenceclient-new.md)

## ekos/plugins/crypto/src/lib.rs

- `trait` [`CryptoExportReader`](entities/rustsymbol/cr/cryptoexportreader.md)
- `struct` [`CryptoObserver`](entities/rustsymbol/cr/cryptoobserver.md)
- `method` [`CryptoObserver::name`](entities/rustsymbol/cr/cryptoobserver-name.md)
- `method` [`CryptoObserver::new`](entities/rustsymbol/cr/cryptoobserver-new.md)
- `method` [`CryptoObserver::scan`](entities/rustsymbol/cr/cryptoobserver-scan.md)
- `enum` [`CryptoReaderError`](entities/rustsymbol/cr/cryptoreadererror.md)
- `struct` [`EntityRecord`](entities/rustsymbol/en/entityrecord.md)
- `struct` [`EvidenceRecord`](entities/rustsymbol/ev/evidencerecord.md)
- `struct` [`ExportBatch`](entities/rustsymbol/ex/exportbatch.md)
- `struct` [`MockCryptoExportReader`](entities/rustsymbol/mo/mockcryptoexportreader.md)
- `method` [`MockCryptoExportReader::new`](entities/rustsymbol/mo/mockcryptoexportreader-new.md)
- `method` [`MockCryptoExportReader::read_latest_batch`](entities/rustsymbol/mo/mockcryptoexportreader-read-latest-batch.md)
- `struct` [`ParquetExportReader`](entities/rustsymbol/pa/parquetexportreader.md)
- `method` [`ParquetExportReader::latest_batch_dir`](entities/rustsymbol/pa/parquetexportreader-latest-batch-dir.md)
- `method` [`ParquetExportReader::read_entities`](entities/rustsymbol/pa/parquetexportreader-read-entities.md)
- `method` [`ParquetExportReader::read_evidence`](entities/rustsymbol/pa/parquetexportreader-read-evidence.md)
- `method` [`ParquetExportReader::read_latest_batch`](entities/rustsymbol/pa/parquetexportreader-read-latest-batch.md)
- `method` [`ParquetExportReader::read_relationships`](entities/rustsymbol/pa/parquetexportreader-read-relationships.md)
- `struct` [`RelationshipRecord`](entities/rustsymbol/re/relationshiprecord.md)
- `function` [`get_string`](entities/rustsymbol/ge/get-string.md)
- `function` [`get_string_list`](entities/rustsymbol/ge/get-string-list.md)
- `function` [`read_rows`](entities/rustsymbol/re/read-rows.md)

## ekos/plugins/fabric/src/lib.rs

- `struct` [`FabricApiClient`](entities/rustsymbol/fa/fabricapiclient.md)
- `method` [`FabricApiClient::items_for_workspace`](entities/rustsymbol/fa/fabricapiclient-items-for-workspace.md)
- `method` [`FabricApiClient::list_items`](entities/rustsymbol/fa/fabricapiclient-list-items.md)
- `method` [`FabricApiClient::new`](entities/rustsymbol/fa/fabricapiclient-new.md)
- `trait` [`FabricClient`](entities/rustsymbol/fa/fabricclient.md)
- `enum` [`FabricClientError`](entities/rustsymbol/fa/fabricclienterror.md)
- `struct` [`FabricItem`](entities/rustsymbol/fa/fabricitem.md)
- `struct` [`FabricObserver`](entities/rustsymbol/fa/fabricobserver.md)
- `method` [`FabricObserver::name`](entities/rustsymbol/fa/fabricobserver-name.md)
- `method` [`FabricObserver::new`](entities/rustsymbol/fa/fabricobserver-new.md)
- `method` [`FabricObserver::scan`](entities/rustsymbol/fa/fabricobserver-scan.md)
- `struct` [`MockFabricClient`](entities/rustsymbol/mo/mockfabricclient.md)
- `method` [`MockFabricClient::list_items`](entities/rustsymbol/mo/mockfabricclient-list-items.md)
- `method` [`MockFabricClient::new`](entities/rustsymbol/mo/mockfabricclient-new.md)

## ekos/plugins/file/src/lib.rs

- `struct` [`FileObserver`](entities/rustsymbol/fi/fileobserver.md)
- `method` [`FileObserver::default`](entities/rustsymbol/fi/fileobserver-default.md)
- `method` [`FileObserver::name`](entities/rustsymbol/fi/fileobserver-name.md)
- `method` [`FileObserver::new`](entities/rustsymbol/fi/fileobserver-new.md)
- `method` [`FileObserver::scan`](entities/rustsymbol/fi/fileobserver-scan.md)
- `function` [`harvest_symbols`](entities/rustsymbol/ha/harvest-symbols.md)
- `function` [`text_excerpt`](entities/rustsymbol/te/text-excerpt.md)

## ekos/plugins/git/src/lib.rs

- `struct` [`GitObserver`](entities/rustsymbol/gi/gitobserver.md)
- `method` [`GitObserver::default`](entities/rustsymbol/gi/gitobserver-default.md)
- `method` [`GitObserver::name`](entities/rustsymbol/gi/gitobserver-name.md)
- `method` [`GitObserver::new`](entities/rustsymbol/gi/gitobserver-new.md)
- `method` [`GitObserver::scan`](entities/rustsymbol/gi/gitobserver-scan.md)
- `method` [`GitObserver::with_max_commits`](entities/rustsymbol/gi/gitobserver-with-max-commits.md)
- `function` [`git_output`](entities/rustsymbol/gi/git-output.md)
- `function` [`is_git_repo`](entities/rustsymbol/is/is-git-repo.md)
- `function` [`parse_stat_summary`](entities/rustsymbol/pa/parse-stat-summary.md)

## ekos/plugins/github/src/lib.rs

- `struct` [`GitHubApiClient`](entities/rustsymbol/gi/githubapiclient.md)
- `method` [`GitHubApiClient::list_files`](entities/rustsymbol/gi/githubapiclient-list-files.md)
- `method` [`GitHubApiClient::list_items`](entities/rustsymbol/gi/githubapiclient-list-items.md)
- `method` [`GitHubApiClient::new`](entities/rustsymbol/gi/githubapiclient-new.md)
- `method` [`GitHubApiClient::request`](entities/rustsymbol/gi/githubapiclient-request.md)
- `trait` [`GitHubClient`](entities/rustsymbol/gi/githubclient.md)
- `enum` [`GitHubClientError`](entities/rustsymbol/gi/githubclienterror.md)
- `struct` [`GitHubItem`](entities/rustsymbol/gi/githubitem.md)
- `struct` [`GitHubObserver`](entities/rustsymbol/gi/githubobserver.md)
- `method` [`GitHubObserver::name`](entities/rustsymbol/gi/githubobserver-name.md)
- `method` [`GitHubObserver::new`](entities/rustsymbol/gi/githubobserver-new.md)
- `method` [`GitHubObserver::scan`](entities/rustsymbol/gi/githubobserver-scan.md)
- `struct` [`MockGitHubClient`](entities/rustsymbol/mo/mockgithubclient.md)
- `method` [`MockGitHubClient::list_items`](entities/rustsymbol/mo/mockgithubclient-list-items.md)
- `method` [`MockGitHubClient::new`](entities/rustsymbol/mo/mockgithubclient-new.md)

## ekos/plugins/localdocs/src/docx.rs

- `struct` [`DocxParser`](entities/rustsymbol/do/docxparser.md)
- `method` [`DocxParser::parse`](entities/rustsymbol/do/docxparser-parse.md)
- `method` [`DocxParser::supported_extension`](entities/rustsymbol/do/docxparser-supported-extension.md)
- `function` [`extract_media_images`](entities/rustsymbol/ex/extract-media-images.md)
- `function` [`paragraph_text`](entities/rustsymbol/pa/paragraph-text.md)
- `function` [`table_rows`](entities/rustsymbol/ta/table-rows.md)

## ekos/plugins/localdocs/src/email.rs

- `struct` [`EmailParser`](entities/rustsymbol/em/emailparser.md)
- `method` [`EmailParser::parse`](entities/rustsymbol/em/emailparser-parse.md)
- `method` [`EmailParser::supported_extension`](entities/rustsymbol/em/emailparser-supported-extension.md)
- `function` [`body_text`](entities/rustsymbol/bo/body-text.md)
- `function` [`header_block`](entities/rustsymbol/he/header-block.md)
- `function` [`render_address`](entities/rustsymbol/re/render-address.md)

## ekos/plugins/localdocs/src/html.rs

- `struct` [`HtmlParser`](entities/rustsymbol/ht/htmlparser.md)
- `method` [`HtmlParser::new`](entities/rustsymbol/ht/htmlparser-new.md)
- `method` [`HtmlParser::parse`](entities/rustsymbol/ht/htmlparser-parse.md)
- `method` [`HtmlParser::supported_extension`](entities/rustsymbol/ht/htmlparser-supported-extension.md)
- `function` [`html_to_text`](entities/rustsymbol/ht/html-to-text.md)

## ekos/plugins/localdocs/src/lib.rs

- `trait` [`DocumentParser`](entities/rustsymbol/do/documentparser.md)
- `struct` [`DocumentSection`](entities/rustsymbol/do/documentsection.md)
- `struct` [`EmbeddedImage`](entities/rustsymbol/em/embeddedimage.md)
- `struct` [`ExtractedTable`](entities/rustsymbol/ex/extractedtable.md)
- `enum` [`ImageFormat`](entities/rustsymbol/im/imageformat.md)
- `struct` [`LocalDocsObserver`](entities/rustsymbol/lo/localdocsobserver.md)
- `method` [`LocalDocsObserver::name`](entities/rustsymbol/lo/localdocsobserver-name.md)
- `method` [`LocalDocsObserver::new`](entities/rustsymbol/lo/localdocsobserver-new.md)
- `method` [`LocalDocsObserver::parser_for`](entities/rustsymbol/lo/localdocsobserver-parser-for.md)
- `method` [`LocalDocsObserver::scan`](entities/rustsymbol/lo/localdocsobserver-scan.md)
- `method` [`LocalDocsObserver::with_defaults`](entities/rustsymbol/lo/localdocsobserver-with-defaults.md)
- `trait` [`OcrEngine`](entities/rustsymbol/oc/ocrengine.md)
- `enum` [`OcrError`](entities/rustsymbol/oc/ocrerror.md)
- `enum` [`ParseError`](entities/rustsymbol/pa/parseerror-cfecf937.md)
- `struct` [`ParsedDocument`](entities/rustsymbol/pa/parseddocument.md)

## ekos/plugins/localdocs/src/ocr.rs

- `struct` [`MockOcr`](entities/rustsymbol/mo/mockocr.md)
- `method` [`MockOcr::new`](entities/rustsymbol/mo/mockocr-new.md)
- `method` [`MockOcr::recognize`](entities/rustsymbol/mo/mockocr-recognize.md)
- `struct` [`TesseractOcr`](entities/rustsymbol/te/tesseractocr.md)
- `method` [`TesseractOcr::recognize`](entities/rustsymbol/te/tesseractocr-recognize.md)

## ekos/plugins/localdocs/src/pdf.rs

- `struct` [`PdfParser`](entities/rustsymbol/pd/pdfparser.md)
- `method` [`PdfParser::parse`](entities/rustsymbol/pd/pdfparser-parse.md)
- `method` [`PdfParser::parse_inner`](entities/rustsymbol/pd/pdfparser-parse-inner.md)
- `method` [`PdfParser::supported_extension`](entities/rustsymbol/pd/pdfparser-supported-extension.md)
- `function` [`extract_sections`](entities/rustsymbol/ex/extract-sections.md)
- `function` [`extract_tables`](entities/rustsymbol/ex/extract-tables.md)
- `function` [`has_uniform_column_count`](entities/rustsymbol/ha/has-uniform-column-count.md)
- `function` [`split_table_row`](entities/rustsymbol/sp/split-table-row.md)

## ekos/plugins/localdocs/src/sanitize.rs

- `struct` [`Sanitized`](entities/rustsymbol/sa/sanitized.md)
- `function` [`is_sanitized_char`](entities/rustsymbol/is/is-sanitized-char.md)
- `function` [`sanitize_text`](entities/rustsymbol/sa/sanitize-text.md)

## ekos/plugins/localdocs/src/text.rs

- `struct` [`TextParser`](entities/rustsymbol/te/textparser.md)
- `method` [`TextParser::new`](entities/rustsymbol/te/textparser-new.md)
- `method` [`TextParser::parse`](entities/rustsymbol/te/textparser-parse.md)
- `method` [`TextParser::supported_extension`](entities/rustsymbol/te/textparser-supported-extension.md)
- `function` [`chunk_text`](entities/rustsymbol/ch/chunk-text.md)
- `function` [`split_to_budget`](entities/rustsymbol/sp/split-to-budget.md)

## ekos/plugins/oracle/src/lib.rs

- `struct` [`ColumnMetadata`](entities/rustsymbol/co/columnmetadata.md)
- `struct` [`ConstraintMetadata`](entities/rustsymbol/co/constraintmetadata.md)
- `struct` [`MockOracleClient`](entities/rustsymbol/mo/mockoracleclient.md)
- `method` [`MockOracleClient::list_constraints`](entities/rustsymbol/mo/mockoracleclient-list-constraints.md)
- `method` [`MockOracleClient::list_tables`](entities/rustsymbol/mo/mockoracleclient-list-tables.md)
- `method` [`MockOracleClient::list_views`](entities/rustsymbol/mo/mockoracleclient-list-views.md)
- `method` [`MockOracleClient::new`](entities/rustsymbol/mo/mockoracleclient-new.md)
- `trait` [`OracleClient`](entities/rustsymbol/or/oracleclient.md)
- `enum` [`OracleClientError`](entities/rustsymbol/or/oracleclienterror.md)
- `struct` [`OracleDbClient`](entities/rustsymbol/or/oracledbclient.md)
- `method` [`OracleDbClient::list_constraints`](entities/rustsymbol/or/oracledbclient-list-constraints.md)
- `method` [`OracleDbClient::list_tables`](entities/rustsymbol/or/oracledbclient-list-tables.md)
- `method` [`OracleDbClient::list_views`](entities/rustsymbol/or/oracledbclient-list-views.md)
- `method` [`OracleDbClient::new`](entities/rustsymbol/or/oracledbclient-new.md)
- `struct` [`OracleObserver`](entities/rustsymbol/or/oracleobserver.md)
- `method` [`OracleObserver::name`](entities/rustsymbol/or/oracleobserver-name.md)
- `method` [`OracleObserver::new`](entities/rustsymbol/or/oracleobserver-new.md)
- `method` [`OracleObserver::scan`](entities/rustsymbol/or/oracleobserver-scan.md)
- `struct` [`TableMetadata`](entities/rustsymbol/ta/tablemetadata.md)
- `struct` [`ViewMetadata`](entities/rustsymbol/vi/viewmetadata.md)

## ekos/plugins/pentaho/src/lib.rs

- `struct` [`PentahoObserver`](entities/rustsymbol/pe/pentahoobserver.md)
- `method` [`PentahoObserver::name`](entities/rustsymbol/pe/pentahoobserver-name.md)
- `method` [`PentahoObserver::new`](entities/rustsymbol/pe/pentahoobserver-new.md)
- `method` [`PentahoObserver::scan`](entities/rustsymbol/pe/pentahoobserver-scan.md)
- `function` [`kettle_kind`](entities/rustsymbol/ke/kettle-kind.md)

## ekos/plugins/python/src/lib.rs

- `struct` [`PythonObserver`](entities/rustsymbol/py/pythonobserver.md)
- `method` [`PythonObserver::name`](entities/rustsymbol/py/pythonobserver-name.md)
- `method` [`PythonObserver::new`](entities/rustsymbol/py/pythonobserver-new.md)
- `method` [`PythonObserver::scan`](entities/rustsymbol/py/pythonobserver-scan.md)

## ekos/plugins/rust/src/lib.rs

- `struct` [`RustObserver`](entities/rustsymbol/ru/rustobserver.md)
- `method` [`RustObserver::name`](entities/rustsymbol/ru/rustobserver-name.md)
- `method` [`RustObserver::new`](entities/rustsymbol/ru/rustobserver-new.md)
- `method` [`RustObserver::scan`](entities/rustsymbol/ru/rustobserver-scan.md)

## ekos/plugins/salesforce/src/lib.rs

- `struct` [`MockSalesforceClient`](entities/rustsymbol/mo/mocksalesforceclient.md)
- `method` [`MockSalesforceClient::list_sobjects`](entities/rustsymbol/mo/mocksalesforceclient-list-sobjects.md)
- `method` [`MockSalesforceClient::new`](entities/rustsymbol/mo/mocksalesforceclient-new.md)
- `struct` [`SObjectField`](entities/rustsymbol/so/sobjectfield.md)
- `struct` [`SObjectMetadata`](entities/rustsymbol/so/sobjectmetadata.md)
- `struct` [`SalesforceApiClient`](entities/rustsymbol/sa/salesforceapiclient.md)
- `method` [`SalesforceApiClient::describe`](entities/rustsymbol/sa/salesforceapiclient-describe.md)
- `method` [`SalesforceApiClient::list_sobjects`](entities/rustsymbol/sa/salesforceapiclient-list-sobjects.md)
- `method` [`SalesforceApiClient::new`](entities/rustsymbol/sa/salesforceapiclient-new.md)
- `trait` [`SalesforceClient`](entities/rustsymbol/sa/salesforceclient.md)
- `enum` [`SalesforceClientError`](entities/rustsymbol/sa/salesforceclienterror.md)
- `struct` [`SalesforceObserver`](entities/rustsymbol/sa/salesforceobserver.md)
- `method` [`SalesforceObserver::name`](entities/rustsymbol/sa/salesforceobserver-name.md)
- `method` [`SalesforceObserver::new`](entities/rustsymbol/sa/salesforceobserver-new.md)
- `method` [`SalesforceObserver::scan`](entities/rustsymbol/sa/salesforceobserver-scan.md)

## ekos/plugins/sap/src/lib.rs

- `struct` [`BusinessObject`](entities/rustsymbol/bu/businessobject.md)
- `struct` [`MockSapClient`](entities/rustsymbol/mo/mocksapclient.md)
- `method` [`MockSapClient::list_business_objects`](entities/rustsymbol/mo/mocksapclient-list-business-objects.md)
- `method` [`MockSapClient::list_organizational_units`](entities/rustsymbol/mo/mocksapclient-list-organizational-units.md)
- `method` [`MockSapClient::new`](entities/rustsymbol/mo/mocksapclient-new.md)
- `struct` [`OrganizationalUnit`](entities/rustsymbol/or/organizationalunit.md)
- `trait` [`SapClient`](entities/rustsymbol/sa/sapclient.md)
- `enum` [`SapClientError`](entities/rustsymbol/sa/sapclienterror.md)
- `struct` [`SapODataClient`](entities/rustsymbol/sa/sapodataclient.md)
- `method` [`SapODataClient::get_json`](entities/rustsymbol/sa/sapodataclient-get-json.md)
- `method` [`SapODataClient::list_business_objects`](entities/rustsymbol/sa/sapodataclient-list-business-objects.md)
- `method` [`SapODataClient::list_organizational_units`](entities/rustsymbol/sa/sapodataclient-list-organizational-units.md)
- `method` [`SapODataClient::new`](entities/rustsymbol/sa/sapodataclient-new.md)
- `struct` [`SapObserver`](entities/rustsymbol/sa/sapobserver.md)
- `method` [`SapObserver::name`](entities/rustsymbol/sa/sapobserver-name.md)
- `method` [`SapObserver::new`](entities/rustsymbol/sa/sapobserver-new.md)
- `method` [`SapObserver::scan`](entities/rustsymbol/sa/sapobserver-scan.md)

## ekos/plugins/snowflake/src/lib.rs

- `struct` [`MockSnowflakeClient`](entities/rustsymbol/mo/mocksnowflakeclient.md)
- `method` [`MockSnowflakeClient::list_schema_objects`](entities/rustsymbol/mo/mocksnowflakeclient-list-schema-objects.md)
- `method` [`MockSnowflakeClient::new`](entities/rustsymbol/mo/mocksnowflakeclient-new.md)
- `struct` [`SchemaObject`](entities/rustsymbol/sc/schemaobject.md)
- `struct` [`SnowflakeApiClient`](entities/rustsymbol/sn/snowflakeapiclient.md)
- `method` [`SnowflakeApiClient::list_schema_objects`](entities/rustsymbol/sn/snowflakeapiclient-list-schema-objects.md)
- `method` [`SnowflakeApiClient::new`](entities/rustsymbol/sn/snowflakeapiclient-new.md)
- `method` [`SnowflakeApiClient::run_statement`](entities/rustsymbol/sn/snowflakeapiclient-run-statement.md)
- `trait` [`SnowflakeClient`](entities/rustsymbol/sn/snowflakeclient.md)
- `enum` [`SnowflakeClientError`](entities/rustsymbol/sn/snowflakeclienterror.md)
- `struct` [`SnowflakeObserver`](entities/rustsymbol/sn/snowflakeobserver.md)
- `method` [`SnowflakeObserver::name`](entities/rustsymbol/sn/snowflakeobserver-name.md)
- `method` [`SnowflakeObserver::new`](entities/rustsymbol/sn/snowflakeobserver-new.md)
- `method` [`SnowflakeObserver::scan`](entities/rustsymbol/sn/snowflakeobserver-scan.md)

## ekos/plugins/sql-dialect-databricks/src/lib.rs

- `struct` [`DatabricksDialectParser`](entities/rustsymbol/da/databricksdialectparser.md)
- `method` [`DatabricksDialectParser::name`](entities/rustsymbol/da/databricksdialectparser-name.md)
- `method` [`DatabricksDialectParser::sqlparser_dialect`](entities/rustsymbol/da/databricksdialectparser-sqlparser-dialect.md)

## ekos/plugins/sql-dialect-mssql/src/lib.rs

- `struct` [`MsSqlDialectParser`](entities/rustsymbol/ms/mssqldialectparser.md)
- `method` [`MsSqlDialectParser::name`](entities/rustsymbol/ms/mssqldialectparser-name.md)
- `method` [`MsSqlDialectParser::new`](entities/rustsymbol/ms/mssqldialectparser-new.md)
- `method` [`MsSqlDialectParser::sqlparser_dialect`](entities/rustsymbol/ms/mssqldialectparser-sqlparser-dialect.md)

## ekos/plugins/sql-dialect-mysql/src/lib.rs

- `struct` [`MySqlDialectParser`](entities/rustsymbol/my/mysqldialectparser.md)
- `method` [`MySqlDialectParser::name`](entities/rustsymbol/my/mysqldialectparser-name.md)
- `method` [`MySqlDialectParser::preprocess`](entities/rustsymbol/my/mysqldialectparser-preprocess.md)
- `method` [`MySqlDialectParser::sqlparser_dialect`](entities/rustsymbol/my/mysqldialectparser-sqlparser-dialect.md)
- `function` [`strip_delimiter_directives`](entities/rustsymbol/st/strip-delimiter-directives.md)

## ekos/plugins/sql-dialect-postgres/src/lib.rs

- `struct` [`PostgresDialectParser`](entities/rustsymbol/po/postgresdialectparser.md)
- `method` [`PostgresDialectParser::name`](entities/rustsymbol/po/postgresdialectparser-name.md)
- `method` [`PostgresDialectParser::sqlparser_dialect`](entities/rustsymbol/po/postgresdialectparser-sqlparser-dialect.md)

## ekos/plugins/sql-dialect-snowflake/src/lib.rs

- `struct` [`SnowflakeDialectParser`](entities/rustsymbol/sn/snowflakedialectparser.md)
- `method` [`SnowflakeDialectParser::name`](entities/rustsymbol/sn/snowflakedialectparser-name.md)
- `method` [`SnowflakeDialectParser::sqlparser_dialect`](entities/rustsymbol/sn/snowflakedialectparser-sqlparser-dialect.md)

## tests/fixtures/sample_project/src/lib.rs

- `function` [`add`](entities/rustsymbol/ad/add.md)

## tests/fixtures/sample_project/src/main.rs

- `function` [`main`](entities/rustsymbol/ma/main.md)

## tests/integration/tests/integration.rs

- `function` [`copy_dir`](entities/rustsymbol/co/copy-dir-7496161f.md)
- `function` [`ecommerce_pipeline_end_to_end`](entities/rustsymbol/ec/ecommerce-pipeline-end-to-end.md)
- `function` [`fixtures_dir`](entities/rustsymbol/fi/fixtures-dir.md)
- `function` [`northwind_pipeline_end_to_end`](entities/rustsymbol/no/northwind-pipeline-end-to-end.md)
- `function` [`odoo_git_fixture_pipeline_end_to_end`](entities/rustsymbol/od/odoo-git-fixture-pipeline-end-to-end.md)
- `function` [`run_pipeline`](entities/rustsymbol/ru/run-pipeline.md)
- `function` [`table_count`](entities/rustsymbol/ta/table-count.md)

