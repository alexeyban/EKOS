# Devlog 279 — Pentaho packages to lakehouse dbt: core seams, a redaction fix, console pages v3

**Date:** 2026-10-18
**PRs:** none (local `main`, not pushed)
**Branch:** main — cee0370, 5948a8b, 0b45885, 0f1cdf8, cf2571e, 9c67629, e6f94b1, c62d197 (+ this docs commit)
**Companion:** ekos-lakehouse devlog_11 (the translator itself)

---

## Summary

EKOS Lakehouse can now translate Pentaho (Kettle) packages into silver and gold dbt-trino models,
with every step labelled exact / rewritten / llm-drafted / gap (RFC 0200–0205, built in the
lakehouse repo). Public EKOS gained only additive seams for it — Pentaho custom kinds, an LLM task,
a `JavaDecompiler` hook the private ekos-binary build can fill — plus a console page seam v3
(RFC 0207: selections, code/graph/markdown views, bound actions) that the new Pentaho tab uses.
Building it found a real RFC 0043 leak: Kettle connection passwords reached core artifacts.

---

## cee0370 — RFC 0200 seams

### Problem / motivation
The translator lives in ekos-lakehouse (a public extension), compiled Java decompilation in the
private ekos-binary; neither may depend on the other, so the core needs the meeting points.

### What was built
| Part | What |
|---|---|
| `custom_kinds::REGISTRY` | `PentahoPackage`, `PentahoJob`, `PentahoTransformation`, `PentahoStep`, `PentahoJobEntry`, `PentahoConnection`, `PentahoTranslation`, all structurally keyed |
| `ekos_recovery::Task::Translate` | `[llm.tasks.translate]` route (RFC 0197) |
| `extension.rs` | `JavaDecompiler` (+ `DecompiledClass/Method/Fidelity`), `EkosExtension::java_decompiler()`, `Extensions::java_decompiler()`; `extensions` on `CommandContext` and `RecoverContext` |

### Decisions
Decompilers return Java-like **source text**, so a plugin's code goes through the same
statement-subset parser as a User Defined Java Class — one lowering path, capped at `rewritten`.

## 5948a8b — redaction: Kettle passwords

### Problem / motivation
`<password>Encrypted 2be98…</password>` is a fixed-key obfuscation anyone can reverse. The
generic secret pattern needs `:` or `=`, which XML never has, so `.ktr`/`.kjb` artifacts from the
core Pentaho observer carried connection passwords into the artifact store.

### What was built
Two built-in patterns: `kettle-encrypted-password` (ordered **before** the generic one, which
otherwise takes the word `Encrypted` as the value and leaves the hex) and `xml-password-element`
(value-only replacement, so the XML still parses). `redact_with_pattern` now replaces only the
`value_*` group whenever a pattern has one. `PIPELINE_LOGIC_VERSION` 3 → 4 so workspaces re-observe.

## 9c67629, e6f94b1 — RFC 0207 console pages v3

### What was built
| Part | What |
|---|---|
| Rust types | `ConsolePage.params` (`PageParam`), `Panel.params`, `Action.binds/flags/confirm`, `View::Code/Graph/Markdown`; types derive `Default` |
| `extension_pages.rs` | selection sources exist and are tables, no cycles, a selection is a **value** option of the panel's read command, bound args exist, required args bound |
| console API | `panel_argv(cat, panel, selections)` appends `--name=value` (422 when missing, NUL, >512 chars); `POST /cli` refuses `--allow-metered` without the tenant's `allow_metered` |
| UI | selectors + row-click selection, dependents cleared, Code / Graph (layered SVG, lanes per group) / Markdown (own renderer, no HTML strings) views, actions with consent |

### Decisions
A selection reaches only a read command's declared value option — the same freedom the Commands
tab already gives a reader — so no "value must be in the source panel's rows" re-check (it would
have run the source command again on every panel load for no new guarantee).

---

## Knowledge Captured

- **Pattern order in `BUILTIN_SECRET_PATTERNS` matters**: an earlier, broader pattern can consume
  part of a secret and leave the rest in clear text.
- **Adding a field to a public struct breaks literal construction downstream** — the console page
  types now derive `Default` and every constructor uses `..Default::default()`.
- **SVG fills with `color-mix(… currentColor …)` render black in Chromium**; use rgba.
- **`pytest` here needs the console venv and `--no-cov`** (`web/api/.venv/bin/python -m pytest
  --no-cov`); `vitest` needs `--no-cache` in a sandbox that cannot write `node_modules/.vite`.

---

## Files Changed

| File | Change |
|---|---|
| `ekos/crates/kir/src/custom_kinds.rs` | `Pentaho*` rows |
| `ekos/crates/recovery/src/llm.rs` | `Task::Translate` |
| `ekos/crates/cli/src/extension.rs` | `JavaDecompiler`, contexts carry `extensions`, page v3 types |
| `ekos/crates/cli/src/{app.rs,commands/recover.rs}` | pass `extensions` |
| `ekos/crates/cli/src/commands/extension_pages.rs` | v3 checks + tests |
| `ekos/crates/cli/tests/{extension_seam,extension_commands}.rs` | decompiler test, `Default` literals |
| `ekos/crates/common/src/{redaction,lib}.rs` | Kettle password patterns, logic version 4 |
| `web/api/app/{extensions,routes/extensions,routes/cli}.py`, `tests/test_extensions.py` | selections, metered rule, tests |
| `web/ui/src/pages/{ExtensionPage.tsx,extension-view.ts,markdown.tsx,ExtensionPage.v3.test.tsx}`, `index.css` | v3 UI |
| `ekos/docs/rfcs/0200…0205, 0207` | the design |
| `CLAUDE.md`, `TODO.md` | pointers |
