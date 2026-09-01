# Devlog 144 — Distributed storage/query engine: fixing what a real end-to-end test found

**Date:** 2026-09-01
**PRs:** (branch `fix/distributed-storage-issues`, not yet merged)
**Branch:** `fix/distributed-storage-issues` → `main`

---

## Summary

Two autonomous end-to-end test runs of RFC 0111/0113 (partitioned + distributed storage) against
real workloads turned up **eight** defects that unit tests had never exercised, because no test
combined `#[tokio::main]` + an object-store backend + a multi-minute pipeline + a short lease TTL
+ concurrent RPC on one connection. All eight are fixed here with regression tests. The headline:
the object-storage read path (RFC 0113 B4) now actually works — a query worker can serve a
partition from an `s3://` URL alone, including its committed-but-unsealed rows — and a dead query
worker no longer takes the whole gateway down with it.

Test artifacts: `test-runs/run-20260831T212115Z/` (run 1, stock) and
`test-runs/run-20260831T222159Z/` (run 2, post-fix, MinIO + the 95-partition Plausible/Elixir
workspace + OpenAI). Each has a `REPORT.md`.

---

## PR — distributed storage/query fixes (8 defects)

### Problem / motivation

Run 1 (stock `e8e1ca3`, a small Pentaho workspace, `file://` object store) found:

1. `[storage.partition] segment-backend-url` → `ekos build`/`commit` **panic**:
   `Cannot drop a runtime in a context where blocking is not allowed`. The `file://` object-store
   path was completely unusable through the CLI.
2. One `kill -9`'d query worker → **every** `DistributedLedger` query failed
   `io: Connection refused`, no failover, even with a second live worker holding the same
   partitions.
3. `ekos coordinator status` always printed `watermark 0`.
4. `ekos diff` on a partitioned ledger printed opaque `entry #N` placeholders.

Run 2 (fixes 1–4 applied, real MinIO, 95 partitions, OpenAI) found four more, each only reachable
once the full object-store + cluster path ran end to end:

5. `[llm-description]` ignored `[llm] provider = "openai"` and sent the OpenAI key to Anthropic →
   1112 × HTTP 401.
6. An object-storage partition below the 8 MiB seal threshold published only an empty
   `manifest.json`; a remote-only query worker saw it as **empty**. With fine-grained
   (entity-kind) partitioning *almost no* partition ever seals, so this was most of the data.
7. `CompileWorker`'s heartbeat interval was hard-coded to 10 s regardless of TTL. Any coordinator
   run with `--ttl-seconds < ~15` silently expired every worker's lease mid-pipeline.
7b. `CoordinatorClient::call` / `QueryWorkerClient::call` guarded write and read with **separate**
    mutexes — two concurrent callers on the one shared connection (a worker's heartbeat racing
    its guard's commit; the gateway's concurrent fan-out) could read each other's response frame.

### What was built

| Component | Change |
|---|---|
| `ekos-segment-backend` `object_store_backend.rs` | `ObjectStoreBackend` now owns a **`DedicatedRt`** — a `tokio` current-thread runtime pinned to one private OS thread. Every `object_store` call `spawn`s onto it and blocks the caller on a `std::sync::mpsc` reply. The `Runtime` is only ever *dropped* on its own thread, so the backend is safe to build/call/drop from a plain sync test, a `spawn_blocking` thread, a current-thread runtime (`ekos compile-worker`), **and** the multi-threaded `#[tokio::main]` CLI. Fixes #1. |
| same, `parse_url` | Now forwards `AWS_*/AZURE_*/GOOGLE_*/OBJECT_STORE_*` env vars to `object_store::parse_url_opts` (plain `parse_url` reads **nothing** — an `s3://` URL against MinIO never authenticated). `store.rs` validates the configured URL with this parse-only path, never by building+dropping a backend. |
| `ekos-segment-backend` `Cargo.toml` | `object-store` feature now implies `object_store/aws` + `object_store/azure` (was `fs` only) — "object storage support" now means real object stores. |
| `ekos-ledger` `segment/mod.rs` | `SegmentStore::publish_active` — publish the active (unsealed) segment + `HEAD` to the backend. `open_with_backend` pulls them when the local active segment is absent/empty. Fixes #6. |
| `ekos-ledger` `fact_ledger.rs` / `partitioned/mod.rs` | `FactLedger::publish_active_to_backend` + `PartitionedLedger::publish_active_segments`. |
| `ekos-distributed` `gateway.rs` | `DistributedLedger::call_worker_failover` — on a *connection* error, rotate to the next worker (every worker can materialise any partition). `fan_out` / `first_present` use it. Non-connection errors still return as-is. Fixes #2. |
| `ekos-cluster` `worker.rs` | Heartbeat interval derived from `lease.expires_at - Utc::now()` (~TTL/3, floored 500 ms, never slower than the 10 s default). Fixes #7. |
| `ekos-cluster` `client.rs` / `ekos-distributed` `worker_client.rs` | `call` holds the write lock across the whole write-then-read round-trip. Fixes #7b. |
| `ekos-cluster` `protocol.rs` / `coordinator.rs` / `client.rs` | New `Request::Watermarks` → `Response::WatermarkMap`; `CoordinatorClient::watermarks()`. |
| `cli` `commands/cluster.rs` | `ekos coordinator status` prints a real "shard / generation" section (fixes #3); `finalize_partitions` also calls `publish_active_segments`; `ekos compile-worker run` gained `--force` (Service A equivalent of `ekos resolve --force` — without it any identity conflict aborted every compile-worker run; the Plausible workspace has 19). |
| `cli` `commands/commit.rs` | `select_llm_provider_for_description` gained an `openai` branch (fixes #5). |
| `cli` `commands/diff.rs` | Resolves `LedgerDiff::touched` ids to `name (kind)` / relationship labels, capped at 50 (fixes #4). |

Regression tests: `object_store_backend::usable_from_within_an_async_runtime`,
`fact_ledger::active_segment_travels_through_the_backend`,
`gateway::gateway_fails_over_when_a_query_worker_is_down`,
`gateway::gateway_errors_cleanly_when_all_workers_are_down`,
`harness::heartbeat_adapts_to_a_short_ttl_so_long_work_keeps_its_lease`.

### Implementation details worth remembering

- **`block_in_place` is a trap for a helper that must run "from any context".** It panics on a
  current-thread runtime. The gateway's `block_on_sync` uses it (guarded by `Handle::try_current`)
  and that's fine there because the gateway only runs under the multi-threaded CLI / MCP server —
  but `ObjectStoreBackend` is also called by `ekos compile-worker`, whose pipeline runs on a
  `new_current_thread` runtime (`cluster.rs`, deliberately, so the executor is free to heartbeat).
  The dedicated-thread design sidesteps the question entirely: the future runs on *its own*
  runtime, so a plain `mpsc::recv()` on the caller thread can never deadlock and never needs
  `block_in_place`.
- **`object_store::parse_url` reads zero configuration.** The doc-comment "credentials come from
  the standard provider env vars" describes `parse_url_opts` with `std::env::vars()` passed in —
  `parse_url` on its own gives you an unconfigured `AmazonS3Builder::new()`. `builder_opts!`
  silently drops keys a scheme doesn't recognise, so forwarding *all* `AWS_*` (etc.) vars is safe.
  For MinIO: `AWS_ENDPOINT`, `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, `AWS_REGION`,
  `AWS_ALLOW_HTTP=true`.
- **The active segment is where the data is** under fine-grained partitioning. `SEGMENT_SEAL_BYTES`
  is 8 MiB and a hard `const`; a per-entity-kind partition of a normal repo holds tens of KB.
  RFC 0113 B4's "a partition is self-describing in object storage" was true only for *sealed*
  history. `publish_active` closes that — a `PartitionCache::materialize` now pulls
  `segments/seg-000000.facts` + `HEAD` and `FactLedger::open_read_only` on the cache dir reads it
  like any local partition (the query worker never uses `open_read_only_with_backend`).
- **`CompileWorker`'s heartbeat must track the coordinator's real TTL.** `CompileWorker::new`
  hard-codes `Duration::from_secs(10)`; the coordinator default TTL is 30 s (`coordinator.rs:20`)
  so it works by luck. `--ttl-seconds 8` → heartbeat 10 s > TTL 8 s → the lease expires between
  every beat, `guard.commit(watermark)` fails `LostLease`, and the generation watermark is never
  recorded even though the whole pipeline ran and there was no competing writer. The lease
  response already carries `expires_at`; derive `~TTL/3` from it.
- **Two mutexes ≠ one round-trip.** A newline-delimited-JSON client with `Mutex<WriteHalf>` +
  `Mutex<Lines<ReadHalf>>` does *not* serialise concurrent `call`s: caller B can take the read
  lock between caller A's write and read and consume A's response line. Symptom:
  `Coordinator("unexpected Ok")` / `Worker("unexpected …")` under load. Hold the write lock across
  the read.
- **`ekos compile-worker` has no retry.** It calls `lease_acquire` once; `already leased` → exit 1.
  Fault-tolerant takeover needs an external supervisor loop (or k8s restart). The test driver
  retries the process until it acquires; measured recovery = ~TTL minus elapsed lease time
  (6.56 s at an 8 s TTL).
- **Cost/latency seen in practice:** committing 95 partitions to MinIO over HTTP ≈ 235 s (the
  per-partition manifest/segment PUTs dominate; the compile itself is ~3 s). A query worker
  materialising all 95 partitions from MinIO on first use pulls ~12 MB / ~840 files. gpt-4o-mini
  document-semantics on a README + CHANGELOG ≈ 2 min for 250 concepts; `[llm-description]` at
  `scope = "modules"` on 1112 Elixir modules is ~30 min sequential — treat as opt-in and expect
  to background it.

### Decisions (alternatives considered)

- **Dedicated OS thread for `ObjectStoreBackend`** vs. (a) making `ekos` main not `#[tokio::main]`
  (huge blast radius), (b) `Handle::block_on` (panics — can't block_on from within a runtime),
  (c) forcing every caller onto `spawn_blocking` (the CLI's `open_store` → `build_partitioned`
  chain is deep sync code). The dedicated thread is fully contained to one file and makes the
  sync `SegmentBackend` contract honest from literally anywhere.
- **`publish_active` on the compile-worker path only**, not on every `FactLedger` commit — a
  co-located `ekos commit` doesn't need it and the extra PUTs aren't free. `ekos compile-worker`
  is the thing that writes for a remote reader.
- **Pragmatic fixes + this devlog**, no per-fix RFC — these are bug fixes to already-accepted
  RFCs (0111/0113), and the RFC process is for new design surface. `--force` on `compile-worker`
  and `Request::Watermarks` are small additive CLI/protocol changes in the same spirit.
- **`[llm-description]` left off for the storage acts.** It's a post-commit enrichment, not part
  of the storage/query path under test, and 30 min of sequential OpenAI calls per compile-worker
  re-run would have dominated the run. ISSUE-5 is proven by the calls that did succeed.

---

## Knowledge Captured

- **RFC 0113 B4 object-storage read path is now real** for the common case (unsealed partitions).
  Before this, `[storage.partition] segment-backend-url` was effectively write-only —
  `ekos build` panicked, and even past the panic a query worker saw empty partitions.
- **The object-store backend is safe from any async/sync context.** Callers no longer need to
  wrap it in `spawn_blocking`. The one cost: constructing an `ObjectStoreBackend` spawns an OS
  thread (named `ekos-objstore-rt`); dropping it joins that thread.
- **`--features distributed` now compiles the AWS + Azure `object_store` backends**, not just
  `fs`. Stock `cargo build` is unchanged (feature off → no `object_store` at all).
- **Coordinator lease TTL and compile-worker heartbeat are now coupled.** Safe to run
  `ekos coordinator serve --ttl-seconds 5` for fast failover; the worker adapts. There is still
  no interrupt-of-in-flight-work on lease loss (RFC 0113 follow-on) — a fenced worker runs to the
  end of its pipeline, then its `guard.commit` is rejected.
- **`ekos coordinator status` finally shows generation numbers** — they're keyed by shard
  (`main`), not partition id, which is why the old per-partition column was always 0.
- **The document-semantics analyzer emits free-form relationship kinds** (bare prepositions,
  natural-language phrases with spaces). Under entity-kind partitioning that's one partition per
  kind — the Plausible workspace produced 95. The object-store key path handles spaces fine;
  the analyzer's vocabulary is the thing to tighten (left for later — DOC-SEM-1 in the report).

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/segment-backend/src/object_store_backend.rs` | `DedicatedRt` (owned runtime on a private OS thread); `parse_url` forwards provider env vars via `parse_url_opts`; +2 tests |
| `ekos/crates/segment-backend/Cargo.toml` | `object-store` feature now bundles `object_store/aws` + `azure` |
| `ekos/crates/ledger/src/segment/mod.rs` | `SegmentStore::publish_active`; `open_with_backend` pulls the active segment when local is absent |
| `ekos/crates/ledger/src/fact_ledger.rs` | `FactLedger::publish_active_to_backend`; `active_segment_travels_through_the_backend` test |
| `ekos/crates/ledger/src/partitioned/mod.rs` | `PartitionedLedger::publish_active_segments` |
| `ekos/crates/distributed/src/gateway.rs` | `call_worker_failover`; `fan_out`/`first_present` use it |
| `ekos/crates/distributed/src/worker_client.rs` | `call` holds the write lock across the round-trip |
| `ekos/crates/distributed/tests/gateway.rs` | failover + all-workers-down tests |
| `ekos/crates/cluster/src/worker.rs` | heartbeat interval derived from the lease's real TTL |
| `ekos/crates/cluster/src/client.rs` | `call` holds the write lock across the round-trip; `watermarks()` |
| `ekos/crates/cluster/src/protocol.rs` | `Request::Watermarks`, `Response::WatermarkMap` |
| `ekos/crates/cluster/src/coordinator.rs` | handle `Request::Watermarks` |
| `ekos/crates/cluster/tests/harness.rs` | `heartbeat_adapts_to_a_short_ttl_…` test |
| `ekos/crates/cli/src/commands/cluster.rs` | `status` shard/generation section; `publish_active_segments` in `finalize_partitions`; `compile-worker --force` |
| `ekos/crates/cli/src/commands/commit.rs` | `select_llm_provider_for_description` `openai` branch |
| `ekos/crates/cli/src/commands/diff.rs` | resolve touched ids to names/kinds, cap 50 |
| `ekos/crates/cli/src/commands/store.rs` | validate `segment-backend-url` via parse-only |
| `ekos/crates/cli/src/bin/ekos.rs` | `compile-worker run --force` flag |
| `test-runs/run-20260831T21…` + `…T22…` | full logs, metrics, REPORT.md for both runs |
