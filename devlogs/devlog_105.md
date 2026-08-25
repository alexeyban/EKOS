# Devlog 105 — Python `from package import submodule` now resolves to the submodule

**Date:** 2026-08-25
**PRs:** (uncommitted at time of writing)
**Branch:** main (direct)

---

## Summary

Second item on the user's "implement every remaining gap" list. `python_analyzer.rs`'s
`ast::Stmt::ImportFrom` handling only ever compiled a `DependsOn` edge to the bare base module
(`imp.module`), ignoring the actual names imported — `from app.services import ai_service` and
`from app.services import db_service` both collapsed onto one `app.services` `PythonModule`
object, losing exactly the distinction the real source draws between two different real files.
Fixed and live-verified against the exact real line in `pdf-reader` that motivated the gap.

## The fix

`walk_top_level_statement`'s `ImportFrom` arm now emits one `DependsOn` edge per imported name,
qualified as `<module>.<name>` instead of the bare `<module>`:

```rust
for alias in &imp.names {
    let name = alias.name.as_str();
    if name == "*" {
        add_import(module.as_str(), file_id, result);
    } else {
        add_import(&format!("{module}.{name}"), file_id, result);
    }
}
```

`<module>.<name>` is a real dotted reference the source itself makes — whether `name` turns out to
be a submodule (the case that motivated this fix) or a symbol re-exported from `module`'s
`__init__`, both are real, non-fabricated facts about what the file references, and strictly finer
than what was compiled before. A star import (`from pkg import *`) has no specific name to qualify
with, so it keeps falling back to the bare module — the only real fact available in that case.

3 tests: updated `recognizes_imports_as_depends_on` (now asserts the qualified name, not the bare
package), a new `from_import_with_multiple_names_resolves_each_to_its_own_qualified_module` (two
distinct real submodules from the same package no longer collapse to one object), and a new
`star_import_falls_back_to_the_bare_module` (confirms the one case that should stay coarse).

## Live verification

Rebuilt `pdf-reader`'s `.ekos/` ledger fresh against the real, whole-project scope
(`backend`+`frontend`+`README.md`) — the exact real line that motivated this fix,
`backend/app/api/ai.py:7: from app.services import ai_service`:

- Before: `DependsOn` from `app/api/ai.py`'s `File` object → a single coarse `app.services`
  `PythonModule` object.
- After: `ekos query find "ai_service"` returns a real `app.services.ai_service` `PythonModule`
  object; `ekos query neighbourhood <ai.py's real File id> --depth 1` shows the real `DependsOn`
  edge landing directly on it. No bare `app.services` import object exists anymore — `ekos query
  find "app.services"` still surfaces the real *directory* (`Rollup`) and the real
  `app/services/__init__.py` `File` object (unrelated, structural, unaffected by this fix), just no
  import-derived package-level node.

`resolve` (whole-project scope) still surfaces the 5 pre-existing cross-kind identity conflicts
`devlog_102` already documented and left as an open, separate identity-design question
(`react`/`vite`/`pdfjs-dist`/`@vitejs/plugin-react`/`react-router-dom` as both `Technology` and
`JsModule`) — required `--force` to proceed, same as every prior round this session; unrelated to
this fix.

Full workspace gate clean: `cargo fmt`, `cargo build --workspace`, `cargo clippy --workspace -- -D
warnings`, `cargo test --workspace` (101/101 test groups). `tests/integration` 3/3.

## Knowledge Captured

- **A bare-package import edge can be strictly less useful than no edge plus a qualified one** —
  `from pkg import a` and `from pkg import b` are two different real dependencies in the source;
  compiling both down to one shared `pkg` node erases that distinction permanently (the coarser
  edge, once compiled, gives no way to later recover which specific name a given file actually
  used). Worth checking for the same "collapsed the finest real signal available" shape in other
  analyzers' import/reference handling before assuming an existing edge is already as precise as
  the source allows.

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/recovery/src/python_analyzer.rs` | `ImportFrom` now emits one `DependsOn` edge per imported name, qualified `<module>.<name>`; star imports still fall back to the bare module; 1 test updated, 2 new |
| `pdf-reader/.ekos/` (external project) | Rebuilt fresh against the fix |
