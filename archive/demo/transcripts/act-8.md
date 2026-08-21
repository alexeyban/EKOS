# Act 8 transcript

**Prompt:** Where is authentication implemented in my estate? Then, if I replace PostgreSQL with Cosmos DB, what breaks?

```
## Where authentication lives

Per the EKOS ledger (project: `serbian-legal-adviser`), there are two independent auth mechanisms, each duplicated across a legacy top-level layout and a newer `components/*` monorepo layout (same code, two paths — worth flagging as drift risk):

1. **API auth** — `src/api/auth/dependencies.py` (+ `components/api/src/api/auth/dependencies.py`)
   FastAPI dependency-injected Bearer auth. `get_auth_context()` pulls a `AsyncSession` via `Depends(get_session)` and validates the token against the DB through `src.api.auth.api_key.validate_api_key`.

2. **Admin auth** — `src/admin/auth.py` (+ `components/admin/src/admin/auth.py`)
   Simpler static-token check: `require_admin()` reads `ADMIN_TOKEN` from env and compares against the bearer token. No DB lookup itself, but it gates routes (`admin/routes/users.py`, `admin/routes/stats.py`, `admin/routes/crawl.py`) that do hit the DB.

Both routes back onto `src/db/connection.py`, which builds an **async SQLAlchemy engine directly from `settings.database_url`** (`src/config.py`, built from `postgres_user/password/db/host/port`, default port 5432). The `users`/`api_keys`/`subscriptions` tables that back auth are defined in `alembic/versions/007_users_and_subscriptions.py`.

## What breaks if PostgreSQL → Cosmos DB

The migration `007_users_and_subscriptions.py` is not portable SQL — it's Postgres-native:

- **`sa.UUID()` with `server_default=sa.text("gen_random_uuid()")`** — Postgres server-side UUID generation function; Cosmos has no equivalent server-side default.
- **`sqlalchemy.dialects.postgresql.JSONB`** — dialect-specific type; doesn't exist outside Postgres.
- The whole stack is **SQLAlchemy + Alembic**, which assume a relational/SQL engine. Cosmos DB (document/NoSQL, even via its SQL API) is not a SQLAlchemy dialect target without a from-scratch data-access rewrite — `get_session()`, every `AsyncSession`-typed route, and Alembic's entire migration chain (`alembic/versions/*`) would need to be replaced, not adapted.
- `src/config.py`'s `postgres_*` settings and `database_url` construction are Postgres-shaped connection params — Cosmos uses account URI + key/AAD, so config and `src/db/connection.py`'s engine construction both break.
- Anything coupled to `src/config.py` by co-change history in the ledger (18 dependency edges, mostly `infra/terraform/modules/database/*`, `infra/terraform/main.tf`, `modules/containers`, `modules/ai`) — i.e., the Terraform database module provisioning Postgres — would need a parallel Cosmos module; nothing in Terraform currently provisions Cosmos.
- Both API-key auth (`validate_api_key`, queries `api_keys`/`users` tables) and admin routes (`users.py`, `stats.py`, `crawl.py`) go through this same session/engine, so **all authentication and authorization is blocked** until the data layer is ported — this isn't an isolated auth-only change.

Bottom line: it's not a config swap. Auth (API-key and admin) both depend transitively on SQLAlchemy/Alembic/Postgres-dialect code, and that dependency is duplicated in two source trees (`src/` and `components/*`), doubling the surface to migrate.
```
