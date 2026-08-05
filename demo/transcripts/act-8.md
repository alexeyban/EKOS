# Act 8 transcript

**Prompt:** Where is authentication implemented in my estate? Then, if I replace PostgreSQL with Cosmos DB, what breaks?

```
## Authentication — as compiled from the ledger (last build 2026-07-17)

Two genuinely different implementations exist across the estate, plus a cluster of auth flows that aren't user-authentication at all:

**`serbian-legal-adviser`** — self-rolled, Postgres-backed
- `src/api/auth/dependencies.py` — FastAPI bearer-token dependency, validates via `validate_api_key()`, pulled through a SQLAlchemy `AsyncSession` (`src/db/connection.py`)
- `src/db/connection.py` — `create_async_engine(settings.database_url)`, i.e. a plain Postgres SQLAlchemy engine
- `src/admin/auth.py` — a *separate*, DB-free path: static `ADMIN_TOKEN` env var checked via `HTTPBearer`, used only for admin endpoints (`src/admin/routes/{users,stats,crawl}.py`)
- Consumers: `src/api/routes/v1/{keys,subscriptions,profile}.py`

**`lingualoop`** — delegates to Supabase Auth
- `backend/routers/auth.py` explicitly documents: *"Supabase Auth handles registration, login, magic-link delivery, and email verification."* The app only upserts a local user row from Supabase JWT claims (`/api/auth/sync`), serves `/api/auth/me`, and does GDPR erasure across **app DB + Supabase + Blob Storage**.
- The app's own Postgres (Azure Flexible Server, provisioned in `infra/terraform/modules/database`) stores app data, not credentials — Supabase owns the actual identity store.

**`openclaw`** — not app auth, provider OAuth
- `github-copilot-auth.ts`, `gemini-auth.ts`, `qwen-portal-auth/`, `mobile-reauth.sh`, etc. — these authenticate the *tool* against AI provider APIs, unrelated to end-user login.

Caveat: the ledger's relationships here are git co-change evidence (`CoupledWith`), not a live import graph — good for "what tends to change together," not a substitute for reading the code directly for exact call chains.

## If PostgreSQL is replaced with Cosmos DB

- **`serbian-legal-adviser` breaks hardest.** Every layer assumes relational Postgres: `db/connection.py`'s SQLAlchemy async engine, `db/models.py` ORM models, Alembic migrations (`alembic/env.py`), and — critically — auth itself, since `dependencies.py` validates API keys through that same session. Auth, billing (`subscriptions.py`, `keys.py`), and profile routes all go dark unless rewritten against a document/NoSQL driver. Azure's "Cosmos DB for PostgreSQL" (Citus) flavor would be far less disruptive than the NoSQL/Mongo API flavor, since it's wire-compatible with Postgres.
- **`lingualoop`'s login itself is safe** — Supabase manages its own backing Postgres independently of your infra. But the app-side Postgres (`infra/terraform/modules/database`) is coupled (co-change evidence, 12 dependents) to `modules/network`, `modules/storage`, `modules/containers`, and `modules/ai` — replacing it means touching that whole Terraform surface, plus rewriting the GDPR-erasure path that currently deletes rows from "app DB."
- **A wider blast radius exists beyond these two projects.** The estate also has a `components/{api,admin,bot,scraper,scheduler,reasoning-engine,knowledge-graph,vector-db}` cluster, each with its own `db/connection.py` + `db/models.py` + Alembic env + Postgres `docker-compose.yml`. Notably `tests/test_tkg/test_age_schema.py` implies use of **Apache AGE**, a Postgres graph extension — that has no Cosmos DB equivalent and would be a hard blocker, not just a migration effort, for that component.

I didn't reconstruct exact schema-level FK dependents (the ledger's `Table` objects returned were mostly sample-db schemas like Northwind, not these apps' live models) — if you want a precise column/FK blast radius before actually migrating, worth running `ekos build && ekos recover && ekos compile && ekos commit` on `serbian-legal-adviser` and the components cluster specifically, then re-querying `ekos_dependents` on the resulting `Table` objects.
```
