"""Console configuration (RFC 0128 §3.3).

Everything is environment-driven for the Phase 0 skeleton; a real workspace registry backed by
SQLite arrives with Phase 1.
"""

from __future__ import annotations

import json

from pydantic import BaseModel, Field
from pydantic_settings import BaseSettings, SettingsConfigDict


class WorkspaceConfig(BaseModel):
    """One registered workspace and the MCP server that serves it."""

    id: str
    name: str
    path: str
    mcp_host: str = "127.0.0.1"
    mcp_port: int = 7331


class Settings(BaseSettings):
    model_config = SettingsConfigDict(env_prefix="EKOS_CONSOLE_", env_file=".env", extra="ignore")

    # Bearer token the browser must present to every /api route except /api/health.
    # Unrelated to the MCP token below.
    console_token: str = "dev-console-token"

    # Token forwarded to `ekos mcp serve --tcp` in each connection's initialize handshake
    # (RFC 0128 R4). Empty => the MCP servers are unauthenticated.
    mcp_token: str = Field(default="", validation_alias="EKOS_MCP_TOKEN")

    # JSON array of WorkspaceConfig, e.g.
    #   EKOS_CONSOLE_WORKSPACES='[{"id":"self","name":"EKOS","path":"/repo","mcp_port":7331}]'
    workspaces_json: str = "[]"

    # Origin the Vite dev server runs on, allowed through CORS.
    dev_origin: str = "http://localhost:5173"

    def workspaces(self) -> list[WorkspaceConfig]:
        raw = json.loads(self.workspaces_json or "[]")
        return [WorkspaceConfig(**w) for w in raw]


_settings: Settings | None = None


def get_settings() -> Settings:
    global _settings
    if _settings is None:
        _settings = Settings()
    return _settings
