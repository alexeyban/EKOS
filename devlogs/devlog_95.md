# Devlog 95 — Two more real bugs found running RFC 0088 at real scale: bare-file observe paths, README lookalike matching

**Date:** 2026-08-24
**PRs:** (uncommitted at time of writing)
**Branch:** main (direct)

---

## Summary

Enabled `[llm-description]` (`scope = "modules"`) against the real analytics backend for real —
not a small test scope this time, the full ~1,066 real modules/subsystems — after the user pointed
out a specific real page (`PlausibleWeb.RequireAccountPlug`) had no AI-Assisted Overview at all.
The run succeeded (1,062/1,066 described, 4 errors) and the exact flagged page now has a real,
accurate, grounded overview. But `Architecture.md`'s new `Purpose` field read "A parser database
for UAInspector, using data from the Matomo device-detector project" — visibly wrong for describing
the whole backend. Chased to two real, previously-undiscovered bugs: a `File` observer bug (any
single bare *file* used as its own `[observe] paths` entry — this project's own config lists four:
`mix.exs`, `mix.lock`, `README.md`, `CHANGELOG.md` — got a silently empty name/path), and a too-loose
README-matching heuristic in RFC 0088's own new code that picked a real vendored lookalike file
instead. Both fixed, tested — but fixing Bug 1 only in `plugins/file` still left the real `Document`
object for `README.md` (produced by `plugins/localdocs`, not `plugins/file`) with an empty name,
so `describe_project` still couldn't find it. Grepping for the same `WalkDir::new(root)` /
`abs_path.strip_prefix(root)` pattern turned up the identical bug independently duplicated in six
more Observer plugins. All fixed identically, with a matching regression test in each.

## Bug 1b — the same bare-file bug was independently duplicated in six more Observer plugins

Bug 1's fix in `plugins/file/src/lib.rs` only fixes the `File`-kind objects that plugin produces —
it does nothing for `describe_project`'s own README lookup, which reads `Document`-kind objects,
produced by `plugins/localdocs`, a completely separate Observer with its own independent copy of
the same `WalkDir::new(root)` / `abs_path.strip_prefix(root)` logic. Re-ran `ekl "FIND Object WHERE
kind = 'Document'"` after the Bug 1 + Bug 2 fixes and confirmed the real `README.md` `Document`
object *still* had `name == ""` — the bug was never actually fixed end-to-end, only for one of the
two object kinds that mattered.

Searched the whole codebase for the same pattern:

```
grep -rln "WalkDir::new(root)\|abs_path.strip_prefix(root)" ekos/plugins/ ekos/crates/
```

Found it duplicated in six more places: `plugins/localdocs`, `plugins/pentaho`, `plugins/javascript`,
`plugins/python`, `plugins/elixir`, `plugins/rust` — every Observer plugin that walks a workspace
tree copied the same directory-only assumption independently, none of them exercised against a
bare-file `[observe] paths` entry before now. A seventh reference turned up in
`crates/simulation/src/ingest.rs`, but only as a comment: that code had already independently
discovered and worked around this exact bug in the past (by always scanning the whole directory
rather than pointing the scanner at a single file) without ever root-causing or fixing the
underlying plugins — real prior evidence this bug class was known, just never actually closed.

Fixed all six plugins with the identical pattern from Bug 1's fix (fall back to the file's own
basename when the stripped relative path is empty), plus one new regression test per plugin
(`a_single_bare_file_observe_path_gets_its_own_real_name_not_an_empty_one`), each pointing
`ScanContext::new(&file_path)` at a single real file and asserting the artifact's path equals the
real filename, not empty. Full workspace gate re-run clean after this round: `cargo fmt`,
`cargo build --workspace`, `cargo clippy --workspace -- -D warnings`, `cargo test --workspace`
(all green, including the 6 new tests), and `tests/integration` (3/3 passed).

## Bug 1 — a bare-file `[observe] paths` entry produced a nameless, pathless `File` object

`plugins/file/src/lib.rs`'s `FileObserver::scan` calls `WalkDir::new(root)` and computes each
file's relative path as `abs_path.strip_prefix(root)`. This is correct when `root` is a directory —
but a real, valid `[observe] paths` entry can also be a single bare file (`paths = ["README.md"]`,
or this project's own `paths = [..., "mix.exs", "mix.lock", "README.md", "CHANGELOG.md"]`). When
`root` *is* a file, `WalkDir` yields exactly one entry equal to `root` itself, and stripping a path
from itself leaves an empty relative path — `Ok("")`, not an `Err` the existing `continue` branch
would have caught. Confirmed at scale: exactly 4 real `File` objects in the real committed ledger
had `name == ""`, matching the real analytics backend-only config's own 4 bare-file entries exactly.

Fixed: when the stripped result is empty, fall back to the file's own basename
(`abs_path.file_name()`) instead of the empty string. One new regression test reproducing the exact
real shape (`ScanContext::new` given a file path directly, not a directory).

## Bug 2 — RFC 0088's own new README-detection was a loose substring match

`describe_project`'s `readme_excerpt` lookup used `o.name.to_lowercase().contains("readme")` — real,
but too loose. The real analytics project vendors two real upstream data-parser license files as
`Document` objects: `ua_inspector/ua_inspector.readme.md` and `ref_inspector/ref_inspector.readme.md`
(both real, both legitimately containing "readme" in their name). Combined with Bug 1 (the real
top-level `README.md` had an *empty* name at the time this ran), the loose substring match picked
`ua_inspector/ua_inspector.readme.md` instead — real content, wrong document, and the LLM correctly
but unhelpfully summarized the real thing it was shown.

Fixed with `is_real_readme_name`: matches only a basename whose own stem (before the first `.`)
equals `readme` case-insensitively — `README.md`/`README`/`README.rst` all match,
`ua_inspector.readme.md`'s real stem (`ua_inspector`) does not. Two new tests: the matcher itself,
and a full `describe_project` run with both a real vendored lookalike and the real README present,
asserting the real one wins regardless of insertion order.

## Live verification

Real full run against the real analytics backend-only config (908 Elixir modules), `scope =
"modules"`, real local `llama3:latest`, zero API cost: **1,062 of 1,066 real modules/subsystems
described, 0 cached (fresh ledger), 4 errors**, real elapsed time ≈2h40m. The exact page the user
flagged, `PlausibleWeb.RequireAccountPlug`, now reads: *"This module... is a plug that handles
account-related functionality... It contains various functions to check if 2-factor authentication
(2FA) is enabled or required, and redirects users accordingly."* — genuinely grounded in its real
`must_enable_2fa?`/`maybe_force_2fa`/`redirect_to` functions, not guessed.

Both new fixes confirmed at the data level on a fresh clean rebuild: 0 real `File` objects with an
empty name (was 4), `README.md` resolves by that exact real name. Bug 1b (found after this section
was first written) means that rebuild still predates the full fix — the `Document`-kind `README.md`
object was still empty-named at that point. A fresh full re-run against the real analytics config,
now with all seven plugins fixed, is the next step to actually confirm the corrected end-to-end
`Purpose`/`Architecture style` output (same real ~2-3hr cost — a fresh ledger means every module
needs re-describing, no cache hits survive an id change).

## Knowledge Captured

- **`[observe] paths` supporting a single bare file, not just directories, is a real, already-used
  shape** (this project's own backend-only config uses it for `mix.exs`/`mix.lock`/`README.md`/
  `CHANGELOG.md`) — any future code that assumes `WalkDir::new(root)` always walks a directory with
  at least one real subdirectory level needs to handle `root` itself being the one and only yielded
  entry.
- **A "does this name contain X" heuristic is exactly the kind of shortcut that looks fine against
  a small test fixture and breaks against a real, messy real-world repo** — the vendored
  `*.readme.md` files that broke this were never hypothetical; they're real files this real,
  popular open-source project bundles for its own dependency licensing.
- **Fixing a bug pattern in the one plugin that exposed the symptom is not the same as fixing the
  bug** — `plugins/file` was fixed first because that's the object kind the investigation started
  from, but the same `WalkDir::new(root)`/`strip_prefix(root)` logic had been independently
  copy-pasted into `localdocs`/`pentaho`/`javascript`/`python`/`elixir`/`rust` as each plugin was
  written, with no shared helper. **Whenever a bug is found in one plugin's `scan()`, grep every
  other plugin's `scan()` for the same pattern before declaring it fixed** — a single shared
  `Observer` trait doesn't imply shared implementation of the parts that aren't in the trait.
- Confirms an existing lesson rather than a new one, a fifth time this session: running the real
  feature at real scale, on a real page a human actually looked at, found bugs no unit test (17
  passing tests for RFC 0088 alone, by this point) had reproduced — and even after finding one,
  the fix's *scope* (one plugin vs. all seven) still needed a deliberate codebase-wide check rather
  than being assumed complete.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/plugins/file/src/lib.rs` | Bare-file `[observe] paths` entry now gets its own real basename instead of an empty relative path; 1 new test |
| `ekos/plugins/localdocs/src/lib.rs` | Same fix — this is the plugin that actually produces the `Document` objects `describe_project` reads; 1 new test |
| `ekos/plugins/pentaho/src/lib.rs` | Same fix; 1 new test |
| `ekos/plugins/javascript/src/lib.rs` | Same fix; 1 new test |
| `ekos/plugins/python/src/lib.rs` | Same fix; 1 new test |
| `ekos/plugins/elixir/src/lib.rs` | Same fix; 1 new test |
| `ekos/plugins/rust/src/lib.rs` | Same fix; 1 new test |
| `ekos/crates/recovery/src/llm_description.rs` | `is_real_readme_name` replaces a loose substring match; 2 new tests |
| `devlogs/devlog_95.md` | This file |
