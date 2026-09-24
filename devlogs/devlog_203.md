# Devlog 203 — Web console: three security fixes and four reliability fixes

**Date:** 2026-09-24
**PRs:** none (local `main`)
**Branch:** main

---

## Summary
A security and reliability pass over the web console's config I/O, subprocess layer and job
runner, plus the CI workflow's token scope. The highest-impact finding is not a security one: a
single transient database error could kill a workspace's job-runner task permanently, after which
every run on that workspace queued forever and eventually 429'd, with nothing in the logs
pointing at the cause. Two of the fixes changed shape because a test disproved the reasoning
behind them.

---

## Security

### 1. `ci.yml` had no `permissions:` block at all

Every job inherited the repository's *default* `GITHUB_TOKEN` scopes. That default is a
repository **setting** — invisible from the workflow file, and one that silently widens every
workflow the day someone changes it. The workflow also runs on `pull_request`, so the token is
exposed to code from an unreviewed branch.

Now `permissions: contents: read` at the top. None of the four jobs writes to the repository;
`upload-artifact` uses the Actions runtime token, not this one.

**Honest severity:** the repository's current default is already `read` (checked:
`actions/permissions/workflow` → `default_workflow_permissions: read`), so nothing was actually
over-permitted today. This is defence in depth and making the guarantee local to the file, not a
live hole being closed.

### 2. `ConfigError` messages leaked absolute server paths into HTTP responses

`routes/config.py` surfaces them verbatim: `raise HTTPException(status_code=502, detail=str(exc))`.
The messages interpolated full paths — `/home/legion/.../ekos.toml does not exist` — which tells a
caller the deployment layout, the OS user and the directory structure (CWE-209). Messages now
name `ekos.toml` and nothing else; the real paths go to the log, where the operator can see them.

### 3. The config write followed symlinks, truncated in place, and used the process umask

`path.write_text(raw)` replaced with a private temp file plus `os.replace`, which fixes three
things at once: the write is atomic (no half-written `ekos.toml` after a crash or a concurrent
writer), the file is created 0600 rather than 0644, and a symlink planted at that name is
*replaced* rather than followed.

Also added a 1 MiB cap on both the file read and the parse. `raw` arrives straight from an HTTP
body and `tomlkit` builds a full document tree in memory before anything validates it.

---

## Reliability

### 4. A dead worker task silently stopped a workspace forever — the worst bug here

`JobRunner._worker` is one task per workspace. Nothing supervises it, nothing restarts it, and
`submit` only creates one when the queue does not already exist:

```python
while True:
    run_id, params = await queue.get()
    try:
        ...
        await self._execute(run_id, ws_path, params)
    finally:
        queue.task_done()
```

`_execute` has its own `except Exception`, but not everything runs inside it — the cancelled
branch's `update_run`, `_notify_done`, and `_execute`'s own *final* status write all sit outside
it. One transient database error in any of those ends the task. After that the queue is never
drained again: runs sit at `queued`, the queue fills, and every subsequent request gets a 429
that says the queue is full — with nothing anywhere saying why.

Reproduced in a test, and confirmed against the unfixed code, where it shows up exactly as it
would in production:

```
Task exception was never retrieved
future: <Task finished name='Task-2' coro=<JobRunner._worker() done, ...
         exception=RuntimeError('database briefly unavailable')>
```

The test then hangs on `queue.join()` — which is the bug, precisely.

### 5. One long output line failed an entire run

`asyncio`'s `StreamReader` defaults to a **64 KiB** limit, and `readline()` raises `ValueError` —
not EOF, not a truncated line — the moment a single line exceeds it. `ekos` emits long lines
routinely: a `--json` payload, a diagnostics path list, a wrapped evidence excerpt. The exception
propagated out of the pump, through `gather`, into `_execute`'s handler, and marked a
multi-minute run failed.

Limit raised to 4 MiB, and past even that the line is dropped with a marker in the log instead of
raising. Losing one pathological line beats losing the run.

### 6. A failing pump orphaned the subprocess

If the pump raised for any reason, `run_streaming` propagated with the child **still running**.
The runner's `finally` pops the process from `self._running`, so after that nothing tracked it
and nothing could cancel it — it ran to completion unsupervised, or until the API process died.
Now any non-timeout exit path reaps the child before the exception leaves.

### 7. Smaller ones

- `terminate()`: `proc.kill()` raises `ProcessLookupError` if the process exits during the grace
  period — a cancel that raced a natural exit propagated that into the request handler.
- `_run_chain`: `models.get_run(run_id).stages` dereferenced a `Run | None` that every other call
  site checks.
- `_execute`: the failure log write could itself raise (full disk), replacing the original error
  with an `OSError` and escaping into the worker loop — i.e. straight into bug 4.
- `aclose()`: fire-and-forget `on_done` webhook tasks were never awaited, so they were destroyed
  pending at loop close.
- `_validate_text`: `tmp = tf.name` was assigned *after* `tf.write(raw)`, so a failing write left
  a temp file on disk with nothing holding a reference to delete it — one leaked file per such
  request.

---

## Knowledge Captured

- **A test can disprove the reasoning behind its own fix, and that is the point.** The claim
  "`os.replace` means a symlink at `ekos.toml` is replaced, not followed" is only half true.
  `config_path` calls `.resolve()`, so a symlink that already exists is followed and its *target*
  is what gets range-checked — deliberate, and safe, because the target must still be inside the
  workspace. What `os.replace` actually protects is the other ordering: the check passes on an
  ordinary path and a link appears before the write lands. The test failed, the comment was
  wrong, and both were corrected rather than the test being bent to fit.
- **64 KiB is the asyncio line limit, and it raises rather than truncating.** Any code doing
  `async for line in proc.stdout` over a subprocess that might emit a long line has this bug.
  `create_subprocess_exec(..., limit=N)` raises it; catching `ValueError` handles the rest,
  because `readline()` resets its buffer when it raises, so the stream stays usable.
- **A `while True` worker with no supervisor must never let an exception escape its loop body.**
  The damage is not the failed run, it is that the *next* thousand runs never start. `_execute`
  having a handler was not enough, because the bookkeeping around the call was outside it.
- **Check the repository setting before rating a workflow-permissions finding.** The default here
  was already `read`, so the fix is defence in depth rather than a live hole. Saying so is more
  useful than claiming a severity the evidence does not support.
- **A test that passes before the fix is not a regression test.** Each fix here was confirmed by
  reverting it and watching the test fail — which is how the worker-death test earned its keep.

---

## Files Changed

| File | Change summary |
|---|---|
| `.github/workflows/ci.yml` | explicit `permissions: contents: read` |
| `web/api/app/config_io.py` | path-free error messages, atomic 0600 write via `os.replace`, 1 MiB size cap, explicit UTF-8 |
| `web/api/app/_proc.py` | 4 MiB stream limit + graceful long-line drop, reap the child on any failure, `ProcessLookupError` race in `terminate` |
| `web/api/app/runner.py` | guarded worker loop, `None`-safe `_run_chain`, log-write failure contained, `aclose` drains callback tasks |
| `web/api/app/routes/config.py` | temp file created before it is written, so a failed write cannot leak it |
| `web/api/tests/test_config_io.py` | 5 new tests + one existing assertion updated to the path-free message |
| `web/api/tests/test_proc_reliability.py` | new — 5 tests |
| `web/api/tests/test_runner.py` | new worker-survival test |
