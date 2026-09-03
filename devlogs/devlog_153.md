# Devlog 153 — RFC 0130: Web Console Phase 2 (`ekos.toml` config UX)

**Date:** 2026-09-03
**Branch:** `feat/0130-web-console-phase-2` → `main` (local merge, `[skip ci]`, local gates only)
**RFC:** `ekos/docs/rfcs/0130-web-console-phase-2.md`

---

## Summary

A browser path to view and edit `ekos.toml` — raw editor, validate, preview-scan, and the
append-only warning flow that devlog 43 established the need for. Two Rust additions:
`ekos config validate` and `ekos config preview-scan`. Auth stays the single static
`CONSOLE_TOKEN` (RFC 0129 §10 Q3 resolved — a config edit is a reversible file write; the
destructive wipe-and-rebuild is Phase 3's job, and that's where the role split lands).

**A real bug surfaced immediately:** this repo's own `ekos.toml` has `*.lock` in
`ignore-patterns`. `ignore-patterns` match a directory *name* exactly (not a glob), so `*.lock`
matches nothing — `ekos config preview-scan` reports `dirs_skipped: 0` for it and `validate`
warns `ignore-pattern-looks-like-a-path`. Left as-is (it's the maintainer's config and changing
scan scope has ledger implications), but the feature earned its keep on its first run.

---

## PR — R7 / R8 (Rust)

New `config` subcommand group; both `--json` (logs to stderr via `emits_machine_output`).

### `ekos config validate [--json] [--file <path>]`

```
{schema_version, ok, errors:[{code, detail}], warnings:[{code, detail}]}
```

- **`errors`** = `EkosConfig::from_file` failures (TOML syntax + `deny_unknown_fields` — a typo'd
  top-level section is already a hard error). Non-empty → `ok: false`.
- **`warnings`** = `[observe]` lint, all pure functions unit-tested directly:
  - `ignore-pattern-looks-like-a-path` — the pattern contains `/`, `\`, `*`, `?`, `[`, or has a
    `stem.ext` shape with a ≤4-char extension. `filter_entry` matches dir *names*, so these match
    nothing.
  - `observe-path-missing` — an `[observe] paths` entry that doesn't resolve under the root.
  - `observe-empty` — `paths = []` (informational).
- **`--file <path>`** validates an arbitrary file instead of the workspace `ekos.toml`, with
  `[observe]` paths still resolved against the workspace root — this is how the console validates
  *unsaved* editor text (writes it to a temp file, points `--file` at it).

### `ekos config preview-scan [--json] [--max-files N]`

```
{roots, total_files, total_bytes, truncated, by_extension:[{ext, files, bytes}],
 ignored_dir_hits:[{pattern, dirs_skipped}], elapsed_ms}
```

Walks exactly what `ekos build` would observe and **counts** — no reading, no observing.
`dirs_skipped: 0` for a pattern is the concrete signal it matched nothing. `--max-files` (default
200 000) stops a pathological walk and sets `truncated: true`.

### The shared walk

`ekos_observation_sdk::walk_observed(ctx, on_file, on_pruned_dir)` was factored out of
`source_fingerprint` — `walkdir` + `filter_entry` on directory-name equality + the per-component
`is_ignored` file check, with two `FnMut` callbacks. `source_fingerprint` and `preview-scan` now
go through it, so "what would `build` observe" has one definition.

---

## PR — Phase 2 console (`web/api`)

| Module | Change |
|---|---|
| `config_io.py` | Stub → real. `tomlkit` (never `tomli-w` — it flattens comments). `read_config` returns raw text + a read-only `[observe]` projection. `write_config` parses, copies the current file to `ekos.toml.bak`, writes. `diff_observe(before, after)` → added/removed paths + patterns; `.narrows` is true iff anything was removed. `append_only_warning(delta)` returns the devlog-43 wording or `None`. |
| `readproc.py` | Allowlist reworked from a special-cased `if` chain to a `{prefix: frozenset(allowed_flags)}` table. Adds `config validate --json [--file]` and `config preview-scan --json [--max-files]`. |
| `routes/config.py` | `GET /config`, `PUT /config` (parse → `ekos config validate` via a temp file → `.bak` → write; `422` on any error, file untouched), `POST /config/validate` (dry-run of unsaved text), `POST /config/preview-scan`. |

Widening (adding paths/patterns) writes with no warning — always safe. The file write in `PUT` is
done by the console directly (`config_io.write_config`), not through a subprocess — it's a plain
local write with a `.bak`, not a pipeline op.

---

## PR — Phase 2 UI (`web/ui`)

`/w/:id/config` (linked from the dashboard header): a monospace `<textarea>` loaded with the
current file, **Validate** / **Preview scan** / **Save** buttons. Save is disabled until a
Validate has passed *for the current text* (editing after validating re-disables it). On a
narrowing save, the append-only warning renders in an amber banner with the exact §2 wording.
Preview-scan renders `total_files` + top extensions and, prominently, any `dirs_skipped: 0`
pattern. A read-only `[observe]` chip summary sits below. No new UI deps.

---

## Verification

Local gates only (`[skip ci]`):

- Rust: `fmt`, `clippy --workspace -D warnings` (exit 0), `test --workspace` (0 failures),
  `tests/integration` 4/4, `ekos-observation-sdk` 5/5. 11 new tests (`commands::config` + the
  refactored `source_fingerprint` still green).
- `web/api`: `ruff` clean, `pytest` **36/36** with `EKOS_BIN` (12 new: `config_io` unit + config
  endpoints live).
- `web/ui`: `tsc -b --noEmit` clean, `vite build` succeeds.
- **End to end** against this repo: `GET /config` returns the real file + observe; `validate` of
  `["target", "src/fixtures"]` flags `src/fixtures`; `validate` of `[bogus]` → `ok: false` with
  the real serde error; `preview-scan` → 10 067 files, top ext `py`, and `*.lock` + `doc-objects`
  flagged as matching no directories.

---

## Knowledge Captured

- **`ignore-patterns` is directory-NAME equality, not a glob or path.** `*.lock`, `src/fixtures`,
  `build/`, `*.tmp` all match nothing. This repo's `ekos.toml` has `*.lock` — a no-op that's been
  there unnoticed. `ekos config validate` / `preview-scan` now catch this class of mistake.
- **`deny_unknown_fields` is only on the top-level `EkosConfig`**, not the nested section structs.
  A typo'd key inside `[observe]` is silently ignored; a typo'd *section* name is a hard error.
- **`EkosConfig::from_file` errors; `from_file_or_default` swallows and defaults.** For validation
  you want `from_file`.
- **`filter_entry`'s closure is `FnMut`** — you can accumulate side effects (the pruned-dir
  callback) inside it during iteration.
- **Validating unsaved editor text needs `--file`**, not stdin — the CLI resolves `[observe]`
  paths against `cwd`, so the console writes a temp file and runs with `cwd = <workspace>`.

---

## Files Changed

| File | Change |
|---|---|
| `ekos/crates/cli/src/commands/config.rs` | New — `validate` + `preview_scan` + 6 tests |
| `ekos/crates/cli/src/commands/mod.rs` | `pub mod config` |
| `ekos/crates/cli/src/bin/ekos.rs` | `Config` subcommand group, `--file` on `validate`, both in `emits_machine_output` |
| `ekos/crates/observation-sdk/src/lib.rs` | `walk_observed` factored out of `source_fingerprint` |
| `ekos/docs/rfcs/0130-web-console-phase-2.md` | The RFC |
| `web/api/app/config_io.py` | Stub → real |
| `web/api/app/readproc.py` | Allowlist table + two `config` shapes |
| `web/api/app/routes/config.py` | New — 4 endpoints |
| `web/api/app/{main,routes/__init__}.py` | Register the config router |
| `web/api/pyproject.toml` | `tomlkit` dep |
| `web/api/tests/{test_config_io,test_config_live}.py` | New; `test_readproc.py` extended |
| `web/ui/src/pages/Config.tsx` | New |
| `web/ui/src/{main.tsx,pages/Dashboard.tsx,api/client.ts,api/types.ts,index.css}` | Route, link, `apiPut`, types, styles |
