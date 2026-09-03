# RFC 0130 — Web Console Phase 2: `ekos.toml` configuration UX

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-09-03
**Phase 2 of:** RFC 0127 (§8.6, §10) · **builds on:** RFC 0129 (Phase 1 console, `devlog_152`),
RFC 0043 (redaction baseline is non-editable), devlog 43 (append-only ledger, config-change vs
rebuild)
**Defers:** RFC 0127 Phases 3–7 (job runner, scheduler, graph) → RFC 0131+

---

## Motivation

`ekos.toml`'s `[observe] paths` and `ignore-patterns` are hand-edited today, and getting them
wrong is expensive in a way that is not obvious. **Devlog 43** established the hard fact: the
ledger is append-only, so *narrowing* `ignore-patterns` or removing an `[observe] path` never
retroactively removes already-compiled data — the only remedy is a full `.ekos/` wipe and rebuild.
A UI that shows what a path change *will* and *will not* do is worth more than a config form.

A second, subtler trap (RFC 0127 §8.6): `WalkDir::filter_entry` in this codebase matches on
directory-**name** equality, not a path prefix or a glob (`observation-sdk/src/lib.rs`,
`plugins/file/src/lib.rs`). Adding `fixtures` to `ignore-patterns` excludes **every** directory
named `fixtures` anywhere in the tree; adding `src/fixtures` or `*.tmp` matches **nothing**. The
preview-scan makes that concrete instead of a footnote.

Phase 2 delivers: a raw `ekos.toml` editor (comments preserved), validate, preview-scan, and the
append-only warning flow.

**Decisions locked before this RFC** (from the maintainer):

- **Auth stays the single static `CONSOLE_TOKEN`.** A config edit is a reversible file write (text
  file, git, a `.bak` the console writes before every save); the *destructive* step — wipe and
  rebuild — is Phase 3's job runner, which is where real users and the read/write role split land.
  RFC 0129 §10 Q3 is hereby resolved: the split does **not** move up to Phase 2.
- **Raw editor + validate; the structured view is read-only.** `PUT` takes the full TOML text
  (round-tripped through `tomlkit`, comments and formatting intact). The UI renders a read-only
  parsed summary of `[observe] paths` / `ignore-patterns` next to the editor — no field-level form
  widgets. Smallest correct surface; a full structured form is a later refinement if it's wanted.

**Not in this RFC:** running the rebuild (Phase 3), any non-`observe` config section getting
special UI, schema-driven form generation.

---

## 1. Rust-side additions — a `config` subcommand group

Both emit one JSON object on `--json` (logs to stderr, per RFC 0129's `emits_machine_output`), a
short human summary otherwise.

### 1.1 R7 — `ekos config validate [--json]`

```json
{
  "schema_version": 1,
  "ok": true,
  "errors": [],
  "warnings": [
    {"code": "ignore-pattern-looks-like-a-path",
     "detail": "'src/fixtures' contains '/'; ignore-patterns match a directory NAME exactly, so this matches nothing. Use 'fixtures'."},
    {"code": "observe-path-missing",
     "detail": "'crates/legacy' does not exist under the workspace root"}
  ]
}
```

- **`errors`** come from `EkosConfig::from_file` — TOML syntax and `deny_unknown_fields` (a typo'd
  key is already a hard error there). A non-empty `errors` array means `ok: false` and the config
  would not load.
- **`warnings`** are the Phase 2 value-add, all `observe`-focused:
  - `ignore-pattern-looks-like-a-path` — the pattern contains `/`, `\`, `*`, `?`, or a leading
    `.` + extension shape (`*.tmp`, `.log`) — anything that reads like a glob or path but is
    matched as a bare directory name.
  - `observe-path-missing` — an `[observe] paths` entry that doesn't resolve under the root.
  - `observe-empty` — `paths` is `[]` (informational: the whole workspace root is scanned).
- The built-in redaction baseline (RFC 0043) is **not** configurable and not reported here — it
  cannot be weakened, so there is nothing to warn about.

Lives in `crates/cli/src/commands/config.rs`; the warning checks are pure functions over
`&ObserveConfig` + the resolved root, unit-tested directly.

### 1.2 R8 — `ekos config preview-scan [--json]`

Walks exactly what `ekos build` would observe — the same `walkdir` + `filter_entry` on
directory-name equality, reusing `ekos_observation_sdk::ScanContext` — and **counts, without
reading or observing anything**.

```json
{
  "schema_version": 1,
  "roots": ["/abs/workspace"],
  "total_files": 691,
  "total_bytes": 5_100_000,
  "by_extension": [{"ext": "rs", "files": 420, "bytes": 3_900_000}, {"ext": "md", "files": 190}],
  "ignored_dir_hits": [{"pattern": "target", "dirs_skipped": 1}, {"pattern": "fixtures", "dirs_skipped": 3}],
  "elapsed_ms": 40
}
```

- `ignored_dir_hits` reports how many directories each `ignore-patterns` entry actually pruned —
  `dirs_skipped: 0` for a pattern is the concrete signal that it matched nothing (a glob typo).
- Deterministic modulo `elapsed_ms`. Bounded by a `--max-files` (default 200 000) that stops the
  walk and sets `truncated: true` rather than hanging on a pathological tree.

Reuses `source_fingerprint`'s walk shape (`observation-sdk/src/lib.rs`) — factored into a shared
`walk_observed(ctx, on_file, on_pruned_dir)` so the two can't drift.

---

## 2. Console — `web/api/app/config_io.py`

Replaces the Phase 1 stub. `tomlkit` only (never `tomli-w` — it discards comments and layout).

```python
def read_config(path: Path) -> ConfigOut          # {raw: str, observe: {paths, ignore_patterns}}
def write_config(path: Path, raw: str) -> WriteOut # validates, .bak, writes, returns the diff + warnings
def diff_observe(before: str, after: str) -> ObserveDelta   # added / removed paths + patterns
```

- **`read_config`** parses with `tomlkit`, returns the raw text plus a **read-only** projection of
  `[observe]` (`paths`, `ignore-patterns`) for the summary panel.
- **`write_config`**:
  1. Parse the incoming `raw` with `tomlkit` — a syntax error is a `422` with the message, nothing
     is written.
  2. Shell out to `ekos config validate --json` (through the RFC 0129 `readproc` allowlist, now
     four shapes) — `errors` → `422`, `warnings` pass through to the response.
  3. Compute `diff_observe(current_file, raw)`. If any path or pattern was **removed** (narrowing),
     attach an `append_only_warning`:
     > "N path(s) / M ignore-pattern(s) were removed. This affects **future** builds only — the
     > append-only ledger keeps everything already compiled for the removed scope. To actually
     > drop it you must wipe `.ekos/` and rebuild (a Phase 3 job)."
  4. Copy the current file to `ekos.toml.bak` (overwriting the previous `.bak`), then write `raw`.
  5. Return `{written: true, observe_delta, warnings, append_only_warning}`.

Widening (adding paths/patterns) writes with no warning — it is always safe, and the next build
picks it up.

---

## 3. HTTP surface (RFC 0127 §8.3 subset)

```
GET    /api/workspaces/{id}/config                  # {raw, observe}
PUT    /api/workspaces/{id}/config     {raw}        # validate → .bak → write; 422 on error
POST   /api/workspaces/{id}/config/validate {raw}   # dry run of the above's validate step
POST   /api/workspaces/{id}/config/preview-scan     # runs `ekos config preview-scan --json`
```

All four sit behind the same single `require_console_token` dependency. `PUT` and
`preview-scan` are the only new subprocess callers — both read-only-ish (`preview-scan` reads the
tree; `validate` reads the file) and added to the `readproc` allowlist. The **file write** in
`PUT` is done by the console directly (`config_io.write_config`), not through a subprocess — it is
a plain local write with a `.bak`, not a pipeline operation.

---

## 4. Frontend

A **Config** route (`/w/:id/config`, linked from the dashboard):

- A `<textarea>` raw editor (no CodeMirror/Monaco in Phase 2 — a plain monospace textarea with the
  current file loaded; syntax highlighting is a Phase 7 polish item).
- **Validate** button → `POST /config/validate`, renders `errors` (red) and `warnings` (amber)
  inline.
- **Preview scan** button → `POST /config/preview-scan`, renders `total_files`, the top
  `by_extension` rows, and — prominently — any `ignored_dir_hits` entry with `dirs_skipped: 0`
  ("this pattern matched nothing").
- **Save** → `PUT /config`. On success shows the `observe_delta` and, if present, the
  `append_only_warning` in a dismissible banner with the exact wording from §2. Save is disabled
  until a Validate has passed for the current text.
- The read-only structured summary panel (`observe.paths`, `observe.ignore_patterns` as chips)
  from `GET /config`.

`recharts` is not needed here; no new UI deps.

---

## 5. Testing

**Rust**
- R7: `deny_unknown_fields` typo → `errors`, `ok:false`; `src/fixtures` and `*.tmp` patterns →
  `ignore-pattern-looks-like-a-path`; a missing `[observe] path` → `observe-path-missing`; a clean
  config → `ok:true, warnings:[]`.
- R8: file count + `by_extension` against a fixture tree; a pattern matching a real dir →
  `dirs_skipped > 0`; a glob-shaped pattern → `dirs_skipped: 0`; `--max-files` truncation.

**`web/api`** (pytest)
- `config_io`: `tomlkit` round-trip preserves a comment; a syntax error raises before any write;
  `diff_observe` detects a removed path and a removed pattern.
- Endpoints (live, `EKOS_BIN`-gated): `GET` returns the repo's real `[observe]`; `PUT` that
  narrows `paths` returns `append_only_warning` and leaves an `ekos.toml.bak`; `PUT` with broken
  TOML is `422` and does not touch the file.

**`web/ui`**: `tsc` + `vite build`; a render test of the Config page against a mocked API.

---

## 6. Verification

- Rust workspace gate clean; R7/R8 tests included.
- `web/api`: `ruff` + `pytest` green (unit + `EKOS_BIN`-gated).
- `web/ui`: `tsc` clean, `vite build` succeeds.
- End to end (recorded in the phase devlog): edit this repo's `ekos.toml` through the console —
  add a bogus `fixtures` pattern, run preview-scan, see `dirs_skipped` for it; narrow `paths`,
  see the append-only banner; a `.bak` is left; `git checkout ekos.toml` restores.

---

## 7. Files changed (projected)

| File / area | Change |
|---|---|
| `crates/cli/src/commands/config.rs` | New — `validate` + `preview-scan` |
| `crates/cli/src/bin/ekos.rs` | `Config` subcommand group; both added to `emits_machine_output` |
| `crates/observation-sdk/src/lib.rs` | Factor the walk into `walk_observed(...)` shared by `source_fingerprint` + preview-scan |
| `web/api/app/config_io.py` | Stub → real (tomlkit read / validate / `.bak` write / observe diff) |
| `web/api/app/readproc.py` | Allowlist grows to `config validate --json` + `config preview-scan --json` |
| `web/api/app/routes/config.py` | New — the four endpoints |
| `web/ui/src/pages/Config.tsx` | New — editor + validate + preview-scan + save + summary |
| `web/ui/src/main.tsx`, `pages/Dashboard.tsx` | Route + link |
| `README.md`, `TODO.md`, `docs/generated/ekos-self-documentation.html` | Phase 2 note |

---

## 8. Open questions

1. **A real editor component.** Phase 2 ships a plain `<textarea>` (as built). CodeMirror 6 with a
   TOML mode is ~120 KB; worth it in Phase 7's polish pass, not now.
2. **`config/history`.** Every `PUT` leaves one `.bak` (overwriting the previous). A rolling
   history (last N, or git-backed) would let the console offer "revert to previous" — deferred.
3. **Non-`observe` sections.** `[llm]`, `[embeddings]`, `[storage]` are editable as raw text but
   get no validation beyond parse + `deny_unknown_fields`. Section-specific checks land if and
   when a phase needs them.

## 9. Implementation notes (`devlog_153`)

- Validating **unsaved** editor text needed a CLI change: `ekos config validate --file <path>`.
  The console writes the text to a temp file, runs `validate --file <tmp>` with `cwd = workspace`
  so `[observe]` paths still resolve correctly, and deletes the temp file.
- `deny_unknown_fields` is only on the top-level `EkosConfig` — a typo inside `[observe]` is
  silently dropped; a typo'd *section* is a hard error. `observe_warnings` covers the `[observe]`
  value mistakes that serde can't see.
- `walk_observed` was factored out of `source_fingerprint` (observation-sdk) so preview-scan and
  the fingerprint walk share one definition of "what `build` observes".
- On first run against this repo, `validate` + `preview-scan` both flagged `*.lock` in the repo's
  own `ekos.toml` `ignore-patterns` as a no-op (glob shape, matched as a dir name → matches
  nothing). Left for the maintainer to decide — changing scan scope has ledger implications.
