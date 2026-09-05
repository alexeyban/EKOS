"""Console configuration.

RFC 0128 (Phase 0) drove the whole console from environment variables. RFC 0129 (Phase 1) moves
the workspace list into a SQLite registry (:mod:`app.models`); `EKOS_CONSOLE_WORKSPACES_JSON`
stays supported as a one-time **seed** for an empty registry so existing Compose setups keep
working.
"""

from __future__ import annotations

import json

from pydantic import BaseModel, Field
from pydantic_settings import BaseSettings, SettingsConfigDict


class WorkspaceSeed(BaseModel):
    """A workspace entry from `EKOS_CONSOLE_WORKSPACES_JSON` — used only to seed an empty
    registry on first start. `mcp_host` / `mcp_port` are ignored in Phase 1: the console's
    `McpSupervisor` spawns each server itself and picks the port."""

    id: str
    name: str
    path: str
    mcp_host: str = "127.0.0.1"
    mcp_port: int = 7331


class Settings(BaseSettings):
    model_config = SettingsConfigDict(env_prefix="EKOS_CONSOLE_", env_file=".env", extra="ignore")

    # ── Auth (RFC 0131) ─────────────────────────────────────────────────────
    # OIDC is the real path; the two static tokens below are the fallback used when OIDC_ISSUER
    # is unset (CI, docker compose, local dev).
    oidc_issuer: str = Field(default="", validation_alias="EKOS_CONSOLE_OIDC_ISSUER")
    oidc_client_id: str = ""
    oidc_client_secret: str = ""
    oidc_redirect_uri: str = "http://localhost:8000/api/auth/callback"
    # ID-token claim inspected for the write role, and the values that grant it.
    oidc_role_claim: str = "groups"
    oidc_write_values: str = ""  # comma-separated; empty => every authenticated user is read-only
    post_login_redirect: str = "/"
    session_secret: str = "dev-session-secret-change-me"

    # Token-mode credentials. `console_token` → read; `console_write_token` → read + write.
    console_token: str = "dev-console-token"
    console_write_token: str = ""  # unset => write is never granted in token mode

    @property
    def oidc_enabled(self) -> bool:
        return bool(self.oidc_issuer)

    def oidc_write_value_set(self) -> set[str]:
        return {v.strip() for v in self.oidc_write_values.split(",") if v.strip()}

    # Token base forwarded to `ekos mcp serve --tcp` (RFC 0128 R4). In Phase 1 the supervisor
    # generates a fresh random token per spawned server; this is only the fallback / seed default
    # and what a hand-started server (Phase 0 style) would use.
    mcp_token: str = Field(default="", validation_alias="EKOS_MCP_TOKEN")

    # Path to a built `ekos` binary. `EKOS_BIN` (no prefix) matches the CI / test convention.
    ekos_bin: str = Field(default="ekos", validation_alias="EKOS_BIN")

    # SQLite file for the console's own state (workspace registry today; runs / schedules later).
    console_db: str = ".ekos-web/console.db"

    # Loopback port range the supervisor allocates per-workspace MCP servers from.
    mcp_port_base: int = 7400

    # Job runner (RFC 0131): per-workspace queue depth, and where run logs land.
    run_queue_depth: int = 16
    runs_dir: str = ".ekos-web/runs"

    # JSON array of WorkspaceSeed, e.g.
    #   EKOS_CONSOLE_WORKSPACES_JSON='[{"id":"self","name":"EKOS","path":"/repo"}]'
    workspaces_json: str = "[]"

    # Optional containment for `POST /workspaces` (SonarCloud pythonsecurity:S2083 hardening):
    # when set, a registered workspace's resolved `path` must be this directory or a descendant
    # of it — closing off a `write`-role caller pointing the registry at an arbitrary host path
    # that happens to contain `ekos.toml` + `.ekos/`. Unset (default) preserves the original
    # unrestricted behavior existing Compose setups rely on (e.g. registering "/repo" directly).
    workspaces_root: str = Field(default="", validation_alias="EKOS_CONSOLE_WORKSPACES_ROOT")

    # Origin the Vite dev server runs on, allowed through CORS.
    dev_origin: str = "http://localhost:5173"

    def workspace_seeds(self) -> list[WorkspaceSeed]:
        raw = json.loads(self.workspaces_json or "[]")
        return [WorkspaceSeed(**w) for w in raw]


_settings: Settings | None = None


def get_settings() -> Settings:
    global _settings
    if _settings is None:
        _settings = Settings()
    return _settings
