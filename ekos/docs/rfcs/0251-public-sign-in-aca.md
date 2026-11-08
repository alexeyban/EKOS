# RFC 0251 — Public sign-in: Azure Container Apps authentication for the console and Superset

**Status:** Accepted (2026-11-08, on the maintainer's choice of GitHub/Google sign-in for the public demo)
**Date:** 2026-11-08
**Depends on:** RFC 0180 (console access control), RFC 0182 (MCP gateway), RFC 0249 (public-demo safety),
RFC 0250 (`ekos_sql_generate`)

---

## 1. Motivation

The Azure demo must give the public read-only access to the EKOS console and a user role in Superset, behind
GitHub or Google sign-in. The console signs people in through one OIDC issuer; GitHub is not an OIDC provider,
and the demo plan chose Azure Container Apps' built-in authentication ("Easy Auth"), which offers both and
keeps sign-in code out of the apps. Today, a console binding names a user, a group or a token: there is no way
to say "everyone who signed in may read this project".

## 2. Design

### 2.1 Console: `platform_auth = "aca"`

- With the setting on, a request's person comes from the headers Container Apps sets after sign-in:
  `X-MS-CLIENT-PRINCIPAL-IDP` (`github`, `google`) and `X-MS-CLIENT-PRINCIPAL-NAME` (the GitHub login or the
  Google address), plus `X-MS-CLIENT-PRINCIPAL-ID`. Container Apps strips these headers from outside requests,
  so only it can set them. Another app **inside** the same environment could; the setting is for a console
  whose environment holds only the deployment's own apps (documented).
- The person is a `User` with subject `<idp>:<name>` (lower case), created on first sight; a Google address
  is also their e-mail. `EKOS_CONSOLE_OWNERS` may name `github:<login>` or an address.
- An API token (`Authorization: Bearer ekos_…`) still wins over the headers, so Superset's calls to
  `/mcp/<tenant>/<project>` work unchanged; Container Apps lets them through because the console is set to
  *allow unauthenticated requests* and decides itself (a browser without a person gets the sign-in page).
- It counts as real sign-in everywhere OIDC does: static tokens are refused, production mode accepts it,
  review decisions are recorded under the person's name. It cannot be combined with OIDC or local mode.
- `GET /api/auth/login` answers a page with one link per provider in `platform_auth_providers`
  (`/.auth/login/<provider>?post_login_redirect_uri=/`); sign-out is `/.auth/logout`.

### 2.2 Console: `signed_in_group`

The id of a console group every signed-in person (a `user` principal, never a token) counts as a member of.
A `viewer` binding of that group on the demo project gives the public read access; nothing else changes —
the binding is an ordinary row an admin can remove. Off (empty) by default.

### 2.3 Superset

Container Apps' authentication is set to *require* sign-in for Superset. A small WSGI middleware turns
`X-MS-CLIENT-PRINCIPAL-IDP`/`-NAME` into `REMOTE_USER` (`<idp>:<name>`), `AUTH_TYPE = AUTH_REMOTE_USER`,
self-registration on, into the role `EKOS_PUBLIC_ROLE` (default `EkosPublic`): the `Gamma` permissions plus
`EkosAI can read/write`, without SQL Lab. The admin keeps a local password login only on a separate path
(`/login/` stays off in the public image; the owner is named in `EKOS_ADMINS` and gets `Admin`).

### 2.4 Terraform

`azapi_resource` `Microsoft.App/containerApps/authConfigs` (azurerm has no resource for it) for the console
(allow anonymous, GitHub + Google) and Superset (require sign-in, redirect to GitHub, Google offered on the
sign-in page Superset shows). Client ids are variables; secrets come from Key Vault (the person creates the
GitHub OAuth app and the Google OAuth client, callback `https://<app>/.auth/login/<provider>/callback`).
iDempiere's web UI stays IP-restricted (its seed passwords are public).

## 3. Tests

Console: headers make a user principal only with the setting on; an API token beats the headers; the headers
are ignored without the setting; the signed-in group grants the bound role to a new person and never to a
token; static tokens are refused in `aca` mode; the settings validator refuses `aca` with OIDC or local mode;
the login page lists the providers. Superset: the middleware sets `REMOTE_USER` only from both headers and
strips a client-sent `REMOTE_USER`.

## 4. Open questions

1. A per-person daily allowance already exists (RFC 0249/0250); should a person need to accept the published
   rules before their first question (RFC 0249 §2.4 consent)? Likely yes before launch.
