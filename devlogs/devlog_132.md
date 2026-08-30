# Devlog 132 — RFC 0113: Service A binds the lease to the real compile pipeline

**Date:** 2026-08-30
**PRs:** the commit landed with this devlog
**Branch:** main (direct)

---

## Summary

Since B3, `ekos compile-worker run` was a smoke path — acquire a lease, sleep, commit a made-up
watermark. This wires it to the **real** compiler pipeline: under a heartbeated, fencing-tokened
coordinator lease it runs `build → recover → resolve → compile → commit` against a local
partitioned workspace, then registers every partition it produced with the coordinator and
commits the store's monotonic entry count as the manifest generation. With this, RFC 0113 Phase B
is feature-complete at v1 scope.

---

## What was built

`ekos compile-worker run --coordinator <addr> --shard <name> [--workspace <dir>] [--parallel]`:

1. Load `<workspace>/ekos.toml`. Reject `[storage.distributed]` (that's the read gateway) and
   anything that isn't a Local `[storage.partition]` workspace.
2. `CoordinatorClient::connect`, `CompileWorker::run_shard(shard, …)` — acquires the lease and
   spawns the ~TTL/3 heartbeat.
3. Inside the lease, on a **`spawn_blocking` thread with its own current-thread runtime**, run
   `run_pipeline` = the five existing pipeline stage `run()` functions in order (`commit` with
   `yes = true`). Running it off the worker's executor is the point: a real compile takes
   minutes, and the worker's async executor has to stay free to send `lease_renew` every ~10 s or
   the coordinator would expire the lease out from under a perfectly healthy worker.
4. After the pipeline: `collect_partitions` opens the just-written partitioned store read-only,
   enumerates `(partition-id, root)` for every catalog entry, and takes `entry_count()` as the
   generation watermark.
5. `CatalogRegister` each partition + `RecordEntityPartitions(shard, [ids])`, then
   `guard.commit(watermark)` — **fenced**: if the lease was lost mid-run the commit returns
   `LostLease` and the run fails loudly.
6. Release.

Supporting change: `store::build_partitioned` went `pub(crate)` so the compile worker can build a
read-only `PartitionedLedger` to enumerate partitions after the run.

### Shards, for now

With `entity-kind` partitioning a single compile produces objects across many partitions, so
there is effectively **one shard per workspace** (`--shard main`). The lease is "the right to
write this workspace"; the coordinator provides the mutual exclusion a shared filesystem doesn't.
Genuine multi-worker parallelism needs `source-scope` routing (a `KirObject.source` field,
still pending) — the lease/fence machinery already supports it.

### What's still deferred

- **Object-storage partition *writes*.** `PartitionedLedger` still opens each partition via
  `FactLedger::open(local_path)` — it has no `SegmentBackend` wiring. So a distributed cluster's
  partition roots must sit on a filesystem shared by the compile workers and the query workers
  (NFS etc.) until that lands. Reads from object storage already work (B4a's `PartitionCache`);
  writes are the gap.
- **Interrupting an in-flight compile on lease loss.** v1 lets the pipeline finish and then the
  fenced `manifest_commit` fails. The per-`FactLedger` `write.lock` (RFC 0104) is the real guard
  against a concurrent second writer — a stolen lease can't actually corrupt anything, it just
  wastes the loser's compute.

---

## Knowledge Captured

- **A long sync job under an async lease must run on its own thread, not the worker's executor.**
  `CompileWorker::run_shard` runs the work future inline (not spawned) and heartbeats on the same
  runtime. If the work future blocks that runtime for longer than the lease TTL, the heartbeat
  starves and the lease expires even though nothing is wrong. Fix: `spawn_blocking` a closure
  that builds its own current-thread runtime and `block_on`s the (async) pipeline. The executor
  is then free to renew the lease the whole time.
- **The pipeline stage functions are `async fn` but mostly do sync work** — they're written to be
  called from `#[tokio::main]`. Calling them from a fresh current-thread runtime inside
  `spawn_blocking` is fine and keeps them exactly as-is (no "unmodified passes" violation).
- **`compile-worker` can't use `open_store` in a distributed workspace** — that returns the
  read-only `DistributedLedger` gateway, which rejects `append_*`. Service A writes to a *Local*
  partitioned store directly; the `[storage.distributed]` config is for readers only. The worker
  guards against being pointed at a gateway config.
- **`build_partitioned` is the only way to enumerate a partitioned store's `(key, root)` pairs**
  from `cli` — `open_store` erases the concrete type behind `Box<dyn KnowledgeStore>`. Made it
  `pub(crate)`.

---

## Files Changed

| File | Change summary |
|---|---|
| `crates/cli/src/commands/cluster.rs` | `worker_run` (sleep + fake watermark) → `compile_worker_run` (real `build→…→commit` under a lease, on a blocking-thread runtime; registers partitions; fenced generation commit); `run_pipeline`, `collect_partitions` helpers |
| `crates/cli/src/commands/store.rs` | `build_partitioned` → `pub(crate)` |
| `crates/cli/src/bin/ekos.rs` | `compile-worker run` args: `--shard` / `--workspace` / `--parallel` (was `--partition`/`--root`/`--hold-seconds`/`--watermark`) |
| `tests/integration/tests/integration.rs` | `compile_worker_runs_the_real_pipeline_under_a_lease` — coordinator + real pipeline over the ecommerce fixture, partitions registered, watermark advanced, 6 tables present |
| `tests/integration/Cargo.toml` | `ekos-cluster` dev-dep |
| `ekos/docs/rfcs/0113-…md`, `0111-…md` | Service A real-pipeline acceptance; Phase B feature-complete at v1 |
| `TODO.md`, `README.md` | Service A pipeline binding |
