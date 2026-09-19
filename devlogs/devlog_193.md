# Devlog 193 — characterization tests: run the original, check the rewrite against what it did

**Date:** 2026-09-19
**PRs:** (working tree; implementation is in the private `alexeyban/ekos-binary` repo, this repo gets the devlog)
**Branch:** `main` (local)

---

## Summary

RFC 0150's static parity check compares what a rewrite *touches* with what the original touches. It cannot
say whether they compute the same thing. `ekos-characterize` runs the original .NET assembly — under wine-mono,
inside a bubblewrap sandbox — on recorded inputs, stores what it did, and checks the Python rewrite against that
record. First run on a real TSD type (`TSDServer.Compressor`, 6 cases) found a behavioural difference the static
check had reported as a clean match: given empty input, the original's `Compress(byte[])` returns **zero bytes**
(no gzip header), where the rewrite returned a valid 20-byte empty gzip stream.

This is the first code in the project that **executes** a binary. Everything else reads bytes only. It is
opt-in, kept out of the ledger and the MCP server, and sandboxed; the sandbox is verified, not assumed.

---

## What was built (private repo, `crates/characterize`)

| Piece | What it does |
|---|---|
| `runner/Runner.cs` | Compiled with wine-mono's own `mcs`. Loads the assembly by reflection (non-public members, overload selection by `params`, `out` params, instance construction, per-case working directory), calls the method on a worker thread with a timeout, records return / exception / named fields / stdout / files (size + sha256) |
| `runner/runner.py` | Same for the rewrite, same JSON encoding (`$bytes`, `$datetime`, `$enum`, `$float`, `$decimal`) |
| `src/compare.rs` | Structural comparison: an integer is not a string, `null` is not `""`; forgiving only where the ecosystems differ without meaning (floats 1e-9, enum ↔ name, .NET object ↔ dict, CRLF, `DateTime` fraction digits, `out` params as a Python tuple, `gunzip` mode for compressors) |
| `src/exec.rs` | Toolchain discovery, disposable wine prefix, runner compilation cache, bwrap sandbox for both runtimes, `doctor` probe |
| `src/lib.rs` | `record` → golden file (with assembly SHA-256 and a hash of each case's inputs); `check` → verdicts |
| CLI | `doctor` / `record` / `check` / `run` |

`check` needs only Python and a golden file. A committed golden lets CI verify a rewrite with no wine and no
untrusted code.

## The sandbox, and how it was verified

bubblewrap with `--unshare-all` (network, PID, IPC, UTS, user), a read-only `/usr`, only the assembly's directory
(read-only), a per-run scratch directory and the wine prefix visible. No `$HOME`. `doctor` runs a probe that
tries five escapes: connect to 1.1.1.1:53, resolve a hostname, read a host file outside the binds, list `/home`,
write into a user-writable host directory. **All five blocked.** The probe was then validated with a control:
run unsandboxed, **all five succeed**. (The first version probed writes to `/etc` and `/usr`; the control showed
those are blocked by ordinary file permissions anyway, so they proved nothing, and were replaced.) The Python side
is sandboxed the same way; a test confirms a rewrite that opens a socket or lists `/home` is stopped.

## Verification

- 10 unit tests on the comparator, 3 end-to-end tests that compile a real C# library and run it under the
  sandbox: a correct Python rewrite passes every comparable case (17 cases: instance state, files, stdout,
  enums, `out` params, dates, overloads, exceptions, `NaN`/`Infinity`, an infinite loop that times out on both
  sides), and a rewrite with **4 planted differences** is caught on exactly the cases they belong to while six
  untouched cases still pass. An edited case is an error, not a comparison against stale output.
- Real TSD: 6/6 pass after fixing the rewrite.

## What the first real run found

| Step | Result |
|---|---|
| Static `ekos_binary_migration_check` on the Compressor rewrite | 4 of 4 match |
| `ekos-characterize run`, first attempt | 5 pass, **1 fail**: `compress-empty` |
| Cause | Original returns `byte[0]` for empty input; rewrite returned a 20-byte gzip stream |
| Fix | Rewrite returns `b""` for empty input; comment records the observation |
| Second failure, same case | A bug in the *comparator*: two identical non-gzip byte arrays were treated as unequal because of a bookkeeping marker. Fixed with a test |

---

## Knowledge Captured

- **wine-mono is a working .NET runtime here with no SDK**: `wine app.exe` runs a managed exe, and wine-mono
  ships `mcs.exe` (C# 6), so a C# harness compiles and runs on a machine with neither `dotnet` nor `mono`.
  A dedicated prefix needs wine-mono copied in (`drive_c/windows/mono/mono-2.0`); the system `~/.wine` was
  9.2 GB of unrelated data, the minimal prefix is 1.6 GB and built once.
- **bwrap 0.6.1 cannot bind a file that does not exist yet** (no overlay support either): bind an output
  *directory*. Wine under `--unshare-all` needs `/opt/wine-stable` bound when `/usr/bin/wine` is a symlink into
  it — resolve the real install root at run time, do not hard-code it.
- **A sandbox probe must be validated by its control**, or it can report "blocked" for the wrong reason.
- **Characterization is bounded by the runtime it runs on.** The result for `GZipStream` on empty input is a
  fact about wine-mono's BCL; it is not confirmed on the .NET Compact Framework the app shipped on. Recorded in
  the rewrite's own comment.
- **The original runs with Windows semantics**: `Console.WriteLine` ends in CRLF, paths use backslashes. A file
  the original writes with `Environment.NewLine` will differ from Python's LF file; that is a true cross-platform
  difference, and `files` comparison (sha256) reports it.
- **`.NET DateTime` round-trip format writes 7 fraction digits; Python's `isoformat` writes none when the
  microseconds are zero.** Compare instants after canonicalizing, not as strings.
- Two overloads with the same name cannot be mapped separately by name; a case picks one with `params`.

## Not done / open

- Cases are hand-written. The spec (`spec.pseudocode`, constants, conditions) says which inputs matter, and the
  planner agent now proposes them, but nothing generates a case file.
- No UI driving; the TSD *client* (Compact Framework, native UI, OpenNETCF) cannot run on wine-mono at all.
- The TSDServer forms need a display, so only its non-UI types (`Compressor`, `DataTable`, …) are reachable.
- Sequences of calls on one instance are not expressible (each case constructs a fresh instance).

## Files Changed
| File | Change summary |
|---|---|
| `devlogs/devlog_193.md` | This file |
| `ekos/docs/rfcs/0150-*.md` | Phase 5 describes the implemented harness |
| `TODO.md`, `docs/generated/ekos-self-documentation.html` | Characterization tests ticked / described |
| (private) `crates/characterize/**` | New crate: runners, comparator, executor, CLI, fixture, end-to-end tests |
| (private) `bench/migration/compressor.cases.json`, `.golden.json`, `compressor.py` | Real-type cases, golden and the corrected rewrite |
| (private) `README.md`, `agents/binary-migration-planner.md` | Documentation and the agent's new step |
