# RFC 0180 — Tenants, users, groups and roles in the console

**Status:** Accepted — implemented 2026-10-13 (devlog_261)
**Author:** EKOS team
**Created:** 2026-10-13
**Builds on:** RFC 0131 (OIDC / token sign-in, read and write roles), RFC 0178 (tenant as a directory tree), RFC 0179 (console runs home projects)
**Part of:** production deployment (0178, 0179, this RFC, then 0181 read scope, 0182 MCP gateway, 0183 operations)

## Motivation

The console has two roles, `read` and `write`, and they apply to everything it serves. Anyone
who can sign in sees every workspace; anyone with `write` can run, edit and review everywhere.
RFC 0178 made the tenant a directory tree but nothing says who belongs to one.

This RFC adds, in the console only: tenants, user records, groups, four roles bound per tenant
or per project, API tokens for agents, and an audit log. The Rust CLI is unchanged: a person
with a shell on the machine already has the files.

## Model

| Table | Purpose |
|---|---|
| `Tenant` | id (RFC 0178 name rule), display name, allowed source roots |
| `User` | OIDC subject, email; created at first sign-in, or ahead of it by email |
| `Group`, `GroupMember` | tenant-scoped; a group may name an OIDC claim value, and then everyone whose token carries that value is a member |
| `RoleBinding` | (user, group or token) × (tenant, or one project of it) × role |
| `ApiToken` | SHA-256 of the token, a display prefix, name, tenant, creator, expiry, last use, revocation |
| `AuditLog` | when, who, action, tenant, workspace, outcome, detail |
| `Workspace` | gains `tenant_id`; existing rows belong to `default` |

The tenant `default` always exists.

The schema is versioned: a `schema_version` row and an ordered list of migration functions. An
existing database (three tables, no version) is version 0 and is upgraded at start.

## Roles and actions

| Role | Actions |
|---|---|
| `viewer` | `read`: everything in a project, including runs and their logs |
| `reviewer` | `read`, `review`: semantics confirm / reject / edit, LinkML import, conflict resolve |
| `operator` | `read`, `operate`: run commands, schedules, edit the project config |
| `admin` | all of the above, and `admin`: create and remove projects, members, groups, bindings, tokens, the tenant's audit log |
| platform owner | everything in every tenant, and `platform`: create tenants, set their source roots |

`reviewer` and `operator` are independent; a person may hold both.

A binding on a tenant covers every project of it. A binding on a project covers that project.
`admin` binds only on a tenant.

Bindings narrower than a project (a source, a path pattern, an object) and `deny` rules are
RFC 0181. They are not accepted here, because nothing would enforce them yet.

Platform owners are named in configuration (`EKOS_CONSOLE_OWNERS`, e-mail addresses or OIDC
subjects), not in the database, so the last one cannot be removed by a mistake in the UI.

## Who is asking

| Credential | Principal |
|---|---|
| Session cookie after OIDC sign-in | the `User` with that subject. Roles are read from the database on every request, so a change takes effect at once |
| `Authorization: Bearer ekos_…` | the `ApiToken` with that hash, unless expired or revoked |
| Static console tokens (RFC 0131) | development only: the read token is `viewer` everywhere, the write token is a platform owner |

### Rules

- **Review stays human-only.** A token is bound to `viewer` or `operator`, never `reviewer` or
  `admin`: creation refuses them, and `authorize` denies `review`, `admin` and `platform` to
  any token principal whatever its bindings say. The CLI-level invariant (the MCP server never
  names a review command) now also holds at this layer.
- **One function.** `authz.authorize(principal, action, tenant, workspace)` is the only place a
  role is compared. Every route declares its action through `authz.allow(...)`; a test walks
  the registered routes and fails on one that declares none.
- **No enumeration.** A workspace or tenant the caller has no role in answers 404, the same as
  one that does not exist. 403 is for a caller who can see it and lacks the role.
- **A tenant cannot reach arbitrary host paths.** A project's sources must be inside its
  tenant's source roots, which only a platform owner sets, and each root must itself be inside
  `EKOS_CONSOLE_WORKSPACES_ROOT` when that is set. A tenant other than `default` with no roots
  can create nothing.
- **Production refuses development credentials.** With `EKOS_CONSOLE_PRODUCTION=true` the
  console does not start unless OIDC is configured, at least one owner is named, and the session
  secret is set. Static tokens and the compatibility mapping below are off.

### Compatibility (not in production mode)

- Static tokens work as described above.
- Under OIDC, RFC 0131's mapping still applies to the tenant `default`: every signed-in user is
  `viewer` there, and a user whose role claim matches `EKOS_CONSOLE_OIDC_WRITE_VALUES` is
  `operator` and `reviewer` there. Other tenants are reached through bindings only.

## Projects and tenants

- A project of tenant `default` has workspace id `<project>`; of any other tenant,
  `<tenant>.<project>`. Names cannot contain a dot, so the two never collide.
- `POST /api/workspaces` takes `tenant` (default `default`) and needs `admin` there.
- At start the console registers the home's projects for every tenant it has a row for.

## API

```
GET    /api/auth/me                         mode, email, owner, tenants with the caller's roles
GET    /api/tenants                         tenants the caller has a role in
POST   /api/tenants                         platform
PATCH  /api/tenants/{t}                     platform: name, source roots
GET    /api/tenants/{t}/members             admin: users, their bindings
GET    /api/tenants/{t}/bindings            admin
POST   /api/tenants/{t}/bindings            admin: {email | group_id | token_id, role, workspace_id?}
DELETE /api/tenants/{t}/bindings/{id}       admin
GET    /api/tenants/{t}/groups              admin
POST   /api/tenants/{t}/groups              admin: {name, oidc_value?}
DELETE /api/tenants/{t}/groups/{id}         admin
POST   /api/tenants/{t}/groups/{id}/members admin: {email}
DELETE /api/tenants/{t}/groups/{id}/members/{user_id}
GET    /api/tenants/{t}/tokens              admin
POST   /api/tenants/{t}/tokens              admin: {name, role, workspace_id?, expires_days?}; the token is returned once
DELETE /api/tenants/{t}/tokens/{id}         admin: revoke
GET    /api/tenants/{t}/audit               admin
```

A binding by e-mail for someone who has not signed in creates the `User` with no subject; the
first sign-in with that e-mail claims it. The provider's `email_verified: false` is refused.

## Audit log

Recorded at the authorization step, with the outcome `allowed` or `denied`:

- every request that changes something (any method but `GET`) and needs more than `read`;
- every refused request for more than `read`, `GET` included.

An allowed `GET` of an admin page is not recorded. Sign-ins are recorded. A record says the
request was permitted, not that the command it started succeeded; that is in the run history.
Records of platform actions (creating a tenant) carry no tenant and have no endpoint yet.

## UI

- The header shows the signed-in identity and, for an admin or owner, an Admin link.
- Admin page per tenant: members and bindings, groups, API tokens (the value shown once), audit
  log; for an owner, creating tenants and editing source roots.
- The workspace list shows each workspace's tenant and the caller's roles on it. Controls a role
  cannot use are hidden or disabled; the API enforces regardless.

## Not built

- Bindings below a project, and `deny` (RFC 0181).
- The MCP gateway. An API token authenticates to the console's HTTP API; an MCP client cannot
  use one yet (RFC 0182).
- Deleting a tenant or a user. Renaming a tenant id.
- Custom roles. Local passwords.
- An endpoint for audit records that belong to no tenant (platform actions, sign-ins).
- The image has not been rebuilt and run with this RFC; it was verified against the API
  process and the built UI.
- Token use is not rate-limited.

## Verification

- `authorize` as a table: every role × action, tenant binding and project binding, group
  membership by record and by claim, token principals, owners.
- The route walk: every `/api` route outside the public list declares an action.
- End to end through the API with two tenants: a user in each and one in both; each sees only
  their workspaces (404 otherwise); a viewer cannot run or review; an operator cannot review; a
  token cannot review even when a binding is forced into the database; a revoked or expired
  token is refused; a source outside the tenant's roots is refused.
- Migration: a database created by the previous version opens, keeps its rows, and its
  workspaces belong to `default`.
- Production mode refuses to start with static tokens.
- Every existing console test passes unchanged.
