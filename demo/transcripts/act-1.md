# Act 1 transcript

**Prompt:** What do you actually know about my projects right now?

```
Here's what I currently have on your projects, from memory plus what's visible in this working directory:

**Active/tracked initiatives:**
- **DeFi Sentinel Platform** — Phases 1–2 done as of 2026-07-18. Local-first, GeckoTerminal discovery, exports to EKOS via a Parquet contract. Not yet its own git repo.
- **azure-databricks-project** — has a hard rule: every change goes through a feature branch + Azure DevOps PR (remote is `azure`, not `origin`), and I always run `ruff check src/ notebooks/` + `pytest tests/dp/` before opening a PR.
- **Azure DP (dev)** — RBAC gap: the pipeline SP (`21cbb557`) needs `User Access Administrator` on `rg-dp-dev-n001` to create role assignments during `terraform apply`.
- **Infra cleanup + security** — pending Terraform PR to delete orphaned auth/search/db-subnet resources, fix ACR/KV/Storage security, add NSGs + service endpoints, parameterize OpenAI models.
- **Telegram daily trainer (Phase 19e)** — push scheduler + task FSM, on branch `feature/phase-19e-telegram-daily-trainer`, plan saved at `.claude/plans/dreamy-prancing-boole.md`.
- **lingualoop** — PR merges always need `gh pr merge --admin` (branch ruleset blocks self-approval).

**Standing behavioral notes (not project-specific):**
- Auto-proceed on similar follow-up commands (linter/format/push/merge) without re-confirming.
- Alembic startup hang bug pattern: `asyncio.to_thread` + nested `asyncio.run` causes asyncpg lock hangs — fix via subprocess with 60s timeout.
- EKOS MCP connection needs reconnecting after an EKOS rebuild before querying.

**What I don't actually know:** your current directory listing shows a *lot* of untracked stuff I have no memory of — `EKOS/`, `DeFiSentinelPlatform/`, `VisualDataVault/`, `ai-factory/`, `ai-data-platform-engineer/`, `cdc-lab/`, `game_agent/`, `gitlab/`, multiple `ekos-article-*.md` files, `azure-data-project/`, `DataEngineeringWithDBT/`, etc. Some of these overlap with what I know by name (EKOS, DeFi Sentinel) but I have no detail on the rest. There's also a big pending Terraform diff staged (AzureResources, github.tf, labs/terraform) that I have no context on — want me to look at `git status`/`git diff` to figure out what that's about?
```
