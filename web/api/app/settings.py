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

    # Bearer token the browser must present to every /api route except /api/health.
    # Unrelated to the MCP token below. RFC 0129 keeps this a single static token — real users
    # and the read/write role split land with the first browser write path (Phase 3).
    console_token: str = "dev-console-token"

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

    # JSON array of WorkspaceSeed, e.g.
    #   EKOS_CONSOLE_WORKSPACES_JSON='[{"id":"self","name":"EKOS","path":"/repo"}]'
    workspaces_json: str = "[]"

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
