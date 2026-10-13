# Devlog 262 — Read scope: access control inside a project (RFC 0181)

**Date:** 2026-10-13
**PRs:** none (local main, `[skip ci]`)
**Branch:** main

---

## Summary

Phase D of the production plan. A project is one ledger over several sources, so a limit inside
a project has to be applied where the ledger is read. A read scope is a list of allow and deny
rules by source, path pattern or object. It is enforced in a wrapper around the store, so every
reader of knowledge is covered, including ones not yet written. The MCP server takes a scope
per call and refuses every tool that reads more than the store. The console turns role bindings
into a scope, sends it with every MCP call, and closes the pages a scope cannot cover. A leak
suite runs the real pipeline and every scoped tool, and was itself checked by disabling filters.

---

## RFC 0181

### Problem / motivation

RFC 0180 grants roles down to a project. Two needs were unmet: seeing one repository of a
project and not another, and seeing a project except one directory. Session memory showed what
happens when such a limit is taught to read paths one at a time: six tools returned what two
had been taught to hide.

### What was built

| Part | Change | Where |
|---|---|---|
| Scope and decision | `ReadScope`, `Rule`, `Compiled::visible` | `ekos/crates/ledger/src/scope.rs` (new) |
| Enforcement | `ScopedStore`: a `KnowledgeStore` that filters every read and refuses writes when restricted | same |
| CLI | `--read-scope <file>` / `EKOS_READ_SCOPE`, for `mcp serve`, `ekl`, `query`; every store open is wrapped | `cli/src/app.rs`, `commands/store.rs`, `compiler-core/src/config.rs` |
| MCP | Scope per call in `_meta["ekos/readScope"]`; `SCOPED_TOOLS` / `SCOPE_REFUSED_TOOLS`; result cache keyed by scope; no freshness note for a restricted caller | `cli/src/commands/mcp.rs` |
| Leak suite | Two-source project through the real pipeline, every scoped tool, four scopes | `cli/tests/read_scope.rs` (new) |
| Console: bindings | A binding may carry `effect`, `source`, `path`, `object` (migration 2) | `web/api/app/models.py`, `routes/tenants.py` |
| Console: scope | `authz.scope_for`, `is_restricted`, `sees_all_of`, `allow(..., scoped=True)` | `web/api/app/authz.py` |
| Console: MCP | `ScopedMcp` attaches the scope to every call; it is the only client a route can get | `mcp_client.py`, `deps.py` |
| UI | "only… / all except…" on a viewer grant; a restricted workspace shows the graph only | `pages/Admin.tsx`, `WorkspaceShell.tsx` |

### The decision

An object's origins are its sources crossed with its evidence paths. It is visible when it is
allowed and not denied. Deny always wins. Every origin must be allowed. An unknown source or
path never matches an allow rule and always matches a deny rule.

### Implementation details worth remembering

- **The wrapper owns the inner store** (`Box<dyn KnowledgeStore>`). `KnowledgeStore: Send`, and
  a wrapper holding `&dyn KnowledgeStore` is not `Send`, because the stores are not `Sync`. The
  MCP server therefore keeps one wrapper per cached store and swaps its scopes per call behind
  a `Mutex`, instead of wrapping a borrowed store for each call.
- **Evidence has a path and no source**, so it cannot be judged alone. It is returned only for
  an id listed by an object or relationship already found visible. A cold `get_evidence` under
  a scope returns nothing.
- **Evidence paths are loaded only when a scope has a path rule.** A source-only scope decides
  from the object's own properties.
- **History is checked twice**: the version asked for, and today's. An object moved into a
  hidden directory is hidden in its history too.
- **Search over-fetches four times the limit** and filters, so a page is not short.
- **The scope is part of the MCP result-cache key.** Without it one reader's cached graph
  export would be served to another. The leak suite runs every call unscoped first, on the
  same server, so the cache is populated when the scoped calls arrive.
- **A scope that fails to parse fails the call.** `default` has no default value: a scope that
  lost a field must not become "allow everything".
- **`operator` and `admin` are never restricted.** The pipeline's output shows every source.
  The API returns a warning when a rule is given to one.

### Decisions

- **Enforce at the store, not in `Runtime`.** The plan said `Runtime::with_read_scope`. Graph
  export, EKL and several commands read the store directly; a `Runtime`-level filter would
  have needed the same audit that failed for session memory.
- **Tools and routes are refused by default.** A tool or route is open to a restricted reader
  only when listed. The alternative, listing what to block, leaks whenever someone adds a tool.
- **Deny matches an unknown source.** The first version let a deny rule on source `api` skip
  an object with no source. A commit object lists the files it changed and has no source, so a
  reader denied `secrets/**` would have read those file names.
- **A restricted reader gets the graph and search, and nothing else.** Status, run logs, the
  config, semantics and conflicts read files or run commands that no store wrapper filters.
- **Rules go with `viewer` only.** A restricted reviewer has nothing to review: the items
  carry no source and are hidden.

---

## Verification

| Check | Result |
|---|---|
| Decision and store (`ledger` unit tests) | 31 pass |
| Leak suite (`cli/tests/read_scope.rs`) | 6 pass: no marker and no hidden id in any response under four scopes; markers present without a scope; counts; refused tools; malformed scopes; process and call scopes combined; the CLI flag |
| Tool classification | every MCP tool is scoped or refused with a reason |
| Console (`uv run pytest`) | 257 pass (234 before): scope from bindings, a restricted viewer over HTTP, the scoped-route list, migration 2, one test through the real binary |
| Console UI | 92 pass (90 before) |
| `cargo test --workspace --no-fail-fast` | 2,835 pass |
| `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --check` | clean |
| `tests/integration`, `scripts/audit.sh` | 5 pass; exit 0 after refreshing two lockfiles |

Mutation checks on the leak suite, each a filter in `ScopedStore` turned off:

| Filter disabled | Caught |
|---|---|
| `retrieve` | yes |
| `all_relationships` | yes |
| `get_object` | yes |
| `all_objects` | yes |
| `relationships_for` | yes, after the fixes below |
| `get_evidence` | no: no tool takes an evidence id. The store unit test covers it |
| `find_objects` | no: the tools and `query find` use `retrieve`. The store unit test covers it |

The suite missed `relationships_for` twice. First it asked about three named objects by id, and
none was the visible end of an edge to a hidden object. Then the only "crossing" edge turned
out to be a dangling one whose source id is not an object. The pipeline links objects within
one file, so no path or source rule separates two linked objects in this fixture. A fourth
scope that denies one object by id gives a real crossing edge (`orders` references
`customers`), and every object is now asked about by id.

Not verified: an edge between two sources (the fixture has none), a real OIDC sign-in, the
console pages in a browser, the rebuilt container image.

---

## Knowledge Captured

- **A leak suite needs its own tests.** Three of the first seven mutations passed. Disable one
  filter at a time and require the suite to fail; where it cannot, say which test does.
- **A control run is part of a leak test.** Each marker must come back from a call that did not
  name it, without a scope. Otherwise a fixture that never produced the marker passes.
- **Tools echo their input.** `ekos_retrieve` returns the question's keywords and plan. A
  marker the request names proves nothing; check the markers and hidden ids the request did
  not name.
- **"Crossing edge" must mean two real objects.** `OwnedBy` from a commit id that is not an
  object satisfied "one end hidden, one end not" and tested nothing.
- **A visible document keeps its own text.** A link to a hidden page, with the page's file
  name, is in the visible document. Rules hide objects, not sentences.
- **`&dyn Trait` is `Send` only when the trait object is `Sync`.** A borrowing wrapper cannot
  implement a `Send` trait over stores that are not `Sync`.
- **FastAPI caches a dependency per request**, so `require_workspace` and `authenticated` run
  once however many dependencies ask for them.
- **A new dependency in a crate under `ekos/` makes two other lockfiles stale** (`benchmark/`,
  `tests/integration/`). `scripts/audit.sh` fails on it; `cargo metadata --offline` in each
  refreshes them. `ledger` gained `glob` here.
- **Two cache tests poisoned the MCP result cache by its raw key.** Putting the scope into the
  key at the call site broke them; the key is now built in one helper that both the server and
  the tests go through.
- **Objects without a source** in the two-source fixture: `Person` and `Crate`. Rollups,
  semantics items and conflicts are unattributed by construction (RFC 0178).

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0181-read-scope.md` | New |
| `ekos/crates/ledger/src/scope.rs` | New: scope, decision, `ScopedStore`, tests |
| `ekos/crates/ledger/src/lib.rs`, `Cargo.toml` | `pub mod scope`, `LedgerError::ReadScope`, `glob` |
| `ekos/Cargo.lock`, `benchmark/Cargo.lock`, `tests/integration/Cargo.lock` | `glob` in `ekos-ledger` |
| `ekos/crates/compiler-core/src/config.rs` | Runtime-only `read_scope` |
| `ekos/crates/cli/src/app.rs` | `--read-scope`, allowed on three commands |
| `ekos/crates/cli/src/commands/store.rs` | Every store open wrapped when a scope is set |
| `ekos/crates/cli/src/commands/mcp.rs` | Per-call scope, tool lists, cache key, guard test |
| `ekos/crates/cli/tests/read_scope.rs` | New: leak suite |
| `web/api/app/{models,authz,deps,mcp_client,schemas}.py` | Rule bindings, scope, scoped client |
| `web/api/app/routes/{tenants,graph,stats,commands,runs,schedules,workspaces}.py` | Rules on bindings; scoped and closed routes |
| `web/api/tests/test_authz.py`, `test_authz_routes.py`, `test_tenants.py`, `test_projects_live.py` | Scope table, route list, restricted viewer, live test |
| `web/ui/src/pages/Admin.tsx`, `Admin.test.tsx`, `Workspaces.tsx`, `WorkspaceShell.tsx`, `api/types.ts`, `index.css` | Rule on a grant; restricted workspace |
| `README.md`, `CHANGELOG.md`, `CLAUDE.md`, `TODO.md`, `docs/generated/ekos-self-documentation.html` | Documented |
