# Devlog 261 — Tenants, users, groups and roles in the console (RFC 0180)

**Date:** 2026-10-13
**PRs:** none (local main, `[skip ci]`)
**Branch:** main

---

## Summary

Phase C of the production plan. The console had two roles that applied to everything it served.
It now has tenants, user records, groups, four roles granted per tenant or per project, API
tokens for agents, a per-tenant audit log and per-tenant source roots. One function decides
every permission, and a test fails on a route that does not go through it. A workspace or
tenant the caller has no role in answers 404. No Rust code changed.

---

## RFC 0180

### Problem / motivation

Anyone who could sign in saw every workspace, and anyone with `write` could run, edit and review
everywhere. RFC 0178 made the tenant a directory tree; nothing said who belonged to one.

### What was built

| Part | Change | Where |
|---|---|---|
| Model | `Tenant`, `User`, `Group`, `GroupMember`, `RoleBinding`, `ApiToken`, `AuditLog`; `Workspace.tenant_id` | `web/api/app/models.py` |
| Migrations | `schemaversion` row and an ordered `MIGRATIONS` list; a pre-existing database is version 0 | `models.py` |
| Who is asking | Session (OIDC user), `Bearer ekos_…` (API token), static tokens (development) → `Principal` | `auth.py` |
| What they may do | `authorize`, `roles_in`, `require`, and the dependencies `authenticated`, `allow`, `allow_tenant`, `allow_platform` | `authz.py` (new) |
| Source roots | Per tenant, inside the console-wide root; a non-default tenant without roots can add nothing | `roots.py` (new) |
| Admin API | Tenants, bindings, members, groups, tokens, audit | `routes/tenants.py` (new) |
| Sign-in | A `User` row per subject; a record made by e-mail is claimed at first sign-in | `routes/auth.py` |
| Every other route | `require_role("read"/"write")` replaced by an action | `routes/*.py` |
| Production mode | `EKOS_CONSOLE_PRODUCTION`, `EKOS_CONSOLE_OWNERS` | `settings.py` |
| UI | Admin page; tenant and roles in the workspace list; per-workspace hiding of controls | `pages/Admin.tsx`, `Workspaces.tsx`, `WorkspaceShell.tsx` |

Roles: `viewer` reads; `reviewer` also reviews; `operator` also runs and edits config; `admin`
does all of it and manages the tenant. `reviewer` and `operator` are independent.

### Implementation details worth remembering

- **Roles are read from the database on every request.** The session cookie names the user and
  carries the group-claim values from sign-in; it carries no role. Removing a binding takes
  effect on the next request (a test checks it).
- **A token is denied `review`, `admin` and `platform` inside `authorize`**, not only at
  creation. A test writes an `admin` binding for a token straight into the database and the
  token still cannot review or administer.
- **401, then 404, then 403.** `require_workspace` depends on `authenticated`, so an
  unauthenticated caller gets 401 before any lookup. A workspace the caller has no role on is
  404. 403 is for a visible workspace and a missing role.
- **Routes that look a thing up by its own id** (a run, a schedule) check "may this caller do
  the action anywhere" first. A viewer then gets 403 for every id and cannot probe which exist.
- **Workspace ids.** `<project>` in `default`, `<tenant>.<project>` elsewhere. A name cannot
  contain a dot.
- **Audit at the authorization step.** Changes and refusals are recorded; an allowed `GET` of an
  admin page is not. The first version recorded those too and the log filled with page views.
- **`last_used_at` on a token is refreshed at most once a minute**, so reading with a token is
  not a database write per request.
- **A token is 256 random bits, stored as a plain SHA-256.** A slow hash protects guessable
  secrets; this is not one.

### Decisions

- **Owners are named in configuration, not in the database.** The last one cannot be removed by
  a mistake in the UI.
- **Bindings stop at the project.** Source, path and object scopes and `deny` are RFC 0181.
  Accepting them now would store rules nothing enforces.
- **One kind of token, with its own role.** A personal token carrying its owner's roles would
  let an agent review with a person's `reviewer` role.
- **Outside production mode the `default` tenant keeps RFC 0131's behaviour.** Existing
  installs keep working after an upgrade. Production mode removes it.
- **Creating or removing a workspace is `admin`,** not `operator`. The static write token is an
  owner, so development setups are unaffected.
- **No tenant deletion.** It needs a decision about the tenant's ledgers; not made here.

---

## Verification

| Check | Result |
|---|---|
| `authorize` table (`test_authz.py`) | 19 pass: every role × action, tenant and project bindings, groups by record and by claim, tokens, owners, the compatibility mapping |
| Route walk (`test_authz_routes.py`) | 6 pass; 64 routes, each declares an action or is on the public or dynamic list |
| Two tenants over HTTP (`test_tenants.py`) | 31 pass: visibility, 404 for another tenant, each role's limits, runs and schedules, groups, source roots, tokens (revoked, expired, forced binding, hash only), audit, migration, production mode |
| Whole console API suite | 234 pass (178 before) |
| Console UI (`tsc`, `vitest`) | clean, 90 pass (81 before) |
| Live API process with the real binary | tenant, workspace, token created; token read 200, run 403, admin 403; audit shows each |

Two existing tests were changed. `test_graph_unit.py` named a workspace that was never
registered and overrode only the MCP dependency; it now also supplies the workspace.
`test_semantics.py` built a `Principal` with the old fields.

Not verified: a real OIDC provider (tests sign the session cookie with the test secret, the way
the callback sets it), and the container image, which was not rebuilt for this phase.

---

## Knowledge Captured

- **A guard test that walks an empty list passes.** The first route walk iterated `app.routes`
  looking for `APIRoute`. This FastAPI (0.141) stores an included router as one
  `_IncludedRouter` entry, so the walk saw nothing and "every route declares an action" passed.
  The walk now goes over each router module's own routes, and a separate test asserts the set
  equals the paths in the app's OpenAPI document.
- **Router-level dependencies are on each route's `dependant`** when read from the router
  itself, so the walk sees them without reaching into FastAPI internals.
- **An unauthenticated 404 is an enumeration leak.** If the workspace lookup runs before the
  credential check, an anonymous caller learns which ids exist. Make the lookup depend on the
  principal.
- **`create_all` never alters a table.** Adding a column to an existing SQLite database needs
  an `ALTER TABLE`, and the first run must tell a new database (start at the latest version)
  from an old one (run every migration). The test builds the old schema by hand.
- **SQLite returns naive datetimes.** Comparing a stored expiry with `datetime.now(UTC)` raises
  until the stored value is given its zone back.
- **Starlette's session cookie is `TimestampSigner(secret).sign(base64(json))`.** Tests can
  sign in as any user without an identity provider.
- **Anything under `ekos/` in the Docker build context invalidates the compile layer.** An
  edited RFC in `ekos/docs/` would rebuild the binary; `ekos/docs/` is now in `.dockerignore`.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0180-tenants-users-groups-roles.md` | New |
| `web/api/app/models.py` | Seven tables, `tenant_id`, migrations, accessors |
| `web/api/app/auth.py` | `Principal`, API tokens, session and static resolution |
| `web/api/app/authz.py`, `roots.py` | New |
| `web/api/app/routes/tenants.py` | New |
| `web/api/app/routes/{auth,workspaces,runs,schedules,commands,config,stats,graph,evals,semantics,conflicts}.py` | Actions through `authz` |
| `web/api/app/{deps,main,settings,schemas,config_io}.py` | `ROUTERS`, production mode, owners, tenant on create, several roots |
| `web/api/tests/test_authz.py`, `test_authz_routes.py`, `test_tenants.py` | New |
| `web/api/tests/test_graph_unit.py`, `test_semantics.py` | Adjusted as described |
| `web/ui/src/pages/Admin.tsx`, `Admin.test.tsx` | New |
| `web/ui/src/{Layout,WorkspaceShell,main}.tsx`, `pages/{Workspaces,Run,Semantics,SemanticsItemPanel,Conflicts}.tsx`, `api/{client,types}.ts`, `index.css` | Roles, tenant, Admin link |
| `deploy/.env.example`, `.dockerignore` | Owners and production mode; `ekos/docs/` excluded |
| `README.md`, `CHANGELOG.md`, `CLAUDE.md`, `TODO.md`, `web/README.md`, `docs/generated/ekos-self-documentation.html` | Documented |
