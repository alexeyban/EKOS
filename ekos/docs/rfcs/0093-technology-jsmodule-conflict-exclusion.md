# RFC 0093 — `Technology`/`JsModule` cross-kind conflict false positive

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-08-25
**Implemented:** 2026-08-25

---

## Motivation

Confirmed real gap, named explicitly in this session's gap-closure list and found live against a
real project (`pdf-reader`, `devlog_102`): `react`/`vite`/`react-router-dom`/`pdfjs-dist`/
`@vitejs/plugin-react` each real-compile as **both** a `Custom("Technology")` object
(`package_json_analyzer.rs`, one per declared `package.json` dependency) **and** a
`Custom("JsModule")` object (`javascript_analyzer.rs`, one per real `import` statement) — and
`DefaultResolver`'s cross-kind conflict detector (`SameNameDifferentKind`, exact normalized-name
match across any two different kinds) flags every one of these as a `[CONFLICT]`. `ekos resolve`
(no `--force`) refuses to proceed at all when any conflict exists — meaning this fires, and blocks
the bare `resolve` command, on **every** real JS/TS project with both a `package.json` and real
source imports, which is the overwhelmingly common shape, not an edge case.

This is a real design gap, not a display bug: a `Technology` (declared dependency) and a `JsModule`
(actually imported specifier) sharing an exact name is an **expected, legitimate co-existence** —
two different, both-real facts about the same external package (declared vs. actually used) — not
a genuine ambiguity a human needs to adjudicate the way, say, two unrelated real-world entities
coincidentally sharing a name would be. Flagging it as a conflict every time trains users to reach
for `--force` reflexively, which defeats the actual purpose of the conflict check for the rare case
it's genuinely needed.

## Design

### Not a merge — a narrower, precise conflict exclusion

Considered and rejected: merging `Technology` and `JsModule` objects that share a name (treating
them as literally the same real-world entity, RFC 0026 `Concept`-style). Rejected because a
`JsModule` isn't exclusively "an external npm package" — `javascript_analyzer.rs`'s `handle_import`
creates one for **every** `import` specifier equally, including real relative/local imports
(`./api/client`, `../components/Foo`) that have no `Technology` counterpart at all and are a
structurally different real thing (a project's own source file, not an external dependency).
Merging by name alone would risk conflating a local file with an unrelated npm package that happens
to share its bare name — a real, if rare, collision this exclusion must not create.

The fix instead narrows exactly what stops being flagged: a `(name)` group whose kinds are **exactly**
`{Technology, JsModule}` (no third kind mixed in — that would still be a genuine, worth-flagging
surprise) **and** every `JsModule` object in the group looks like a real bare package specifier —
not starting with `.`, `..`, or `/` — the same syntactic distinction Node's own module resolution
already uses to tell "resolve from `node_modules`" apart from "resolve relative to this file."
`react`/`@vitejs/plugin-react` pass (bare specifiers, the latter's `@scope/name` form still doesn't
start with `.`/`/`); `./api/client` does not (correctly still eligible to conflict-flag against an
unrelated same-named `Technology`, however unlikely). Both objects remain real, distinct,
unmerged — this only stops them from being *reported* as an ambiguity; `ekos resolve` (no
`--force`) proceeds normally through this case instead of refusing outright.

### Why exclusion, not `cross_system.rs`-style reviewable candidates

RFC 0029/0063's pattern (unconfirmed `SameAs` candidates via `ekos_identity_review`) fits genuine
uncertainty worth a human/agent's judgment call. This isn't that: a `Technology`/bare-specifier-
`JsModule` name match is not uncertain — it is definitionally the expected shape for any real
external dependency that's both declared and imported, which will be *every* dependency of a
project structured this way. Routing all of them through a review queue would just move the noise
rather than remove it, for a case where the review answer is essentially always "yes, expected,
not a merge, not a real ambiguity" — a static syntactic rule is the more honest, appropriately-
scoped fix for a case this mechanically predictable.

## Verification

- `crates/identity/src/lib.rs`: new tests — the real `react`/`Technology`+`JsModule` shape no
  longer conflicts; a third mixed-in kind (e.g. also a `PythonModule`) still conflicts; a
  relative-specifier `JsModule` (`./api/client`) sharing a name with an unrelated `Technology`
  still conflicts (the collision this exclusion must not silently hide).
- Full workspace gate (`fmt`/`build`/`clippy -D warnings`/`test --workspace`) clean, `tests/integration`
  3/3.
- Live-verified against `pdf-reader`'s real whole-project ledger: `ekos resolve` (no `--force`)
  conflict count dropped from 5 to 0 for the `Technology`/`JsModule` pairs this RFC targets, and
  now succeeds without requiring `--force` at all for this project.
