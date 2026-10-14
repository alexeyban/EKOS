"""Console configuration.

RFC 0128 (Phase 0) drove the whole console from environment variables. RFC 0129 (Phase 1) moves
the workspace list into a SQLite registry (:mod:`app.models`); `EKOS_CONSOLE_WORKSPACES_JSON`
stays supported as a one-time **seed** for an empty registry so existing Compose setups keep
working.
"""

from __future__ import annotations

import json
import logging
import secrets
from pathlib import Path
from typing import Literal

from pydantic import BaseModel, Field, model_validator
from pydantic_settings import BaseSettings, PydanticBaseSettingsSource, SettingsConfigDict

from . import console_file

log = logging.getLogger("ekos.console")

# Placeholders that ship in this public repository. Anything signed with one of these is forgeable
# by anyone who has read the source.
_PLACEHOLDER_SESSION_SECRETS = frozenset({"", "dev-session-secret-change-me"})
DEFAULT_CONSOLE_TOKEN = "dev-console-token"


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

    @classmethod
    def settings_customise_sources(
        cls,
        settings_cls: type[BaseSettings],
        init_settings: PydanticBaseSettingsSource,
        env_settings: PydanticBaseSettingsSource,
        dotenv_settings: PydanticBaseSettingsSource,
        file_secret_settings: PydanticBaseSettingsSource,
    ) -> tuple[PydanticBaseSettingsSource, ...]:
        """RFC 0184: environment, then console secrets in the keychain, then `console.toml`."""
        return (
            init_settings,
            env_settings,
            dotenv_settings,
            console_file.KeychainSource(settings_cls),
            console_file.FileSource(settings_cls),
            file_secret_settings,
        )

    # ── Server (RFC 0184) ───────────────────────────────────────────────────
    # `local`: one person on their own machine, signed in by a one-time code written into the
    # home, loopback only. `team`: OIDC, tenants and roles (RFC 0180).
    mode: Literal["team", "local"] = "team"
    # What `ekos console` (RFC 0185) binds. The console itself does not read them.
    host: str = "127.0.0.1"
    port: int = 8000
    # The built UI. Empty: `web/ui/dist` beside this package, as in a checkout.
    ui_dir: str = ""
    # Set by a launcher that starts the console again when it exits (`ekos console`, a container
    # with a restart policy): `POST /api/platform/restart` then works.
    supervised: bool = False

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
    # What sign-in asks the provider for, space-separated. Dex, Keycloak and Okta put group
    # membership in the token only when `groups` is asked for; Entra ID and Google need nothing
    # more than the default.
    oidc_scopes: str = "openid email profile"
    post_login_redirect: str = "/"
    # Signs the session cookie, which is trusted in *every* auth mode. Left at the placeholder, a
    # random per-process secret replaces it (see `_no_published_session_secret`): sessions then do
    # not survive a restart, and a cookie signed with the published string never validates.
    session_secret: str = "dev-session-secret-change-me"

    # Token-mode credentials. `console_token` → read; `console_write_token` → read + write.
    console_token: str = DEFAULT_CONSOLE_TOKEN
    console_write_token: str = ""  # unset => write is never granted in token mode

    # ── RFC 0180 ────────────────────────────────────────────────────────────
    # Platform owners: e-mail addresses or OIDC subjects, comma-separated. Named here and not in
    # the database so the last owner cannot be removed through the console.
    owners: str = ""
    # Production mode: the console refuses to start on development credentials (see
    # `_production_needs_real_sign_in`), static tokens are not accepted, and RFC 0131's
    # everyone-can-read mapping is off, so access comes from role bindings only.
    production: bool = False

    def owner_set(self) -> set[str]:
        return {v.strip().lower() for v in self.owners.split(",") if v.strip()}

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

    # The EKOS home (RFC 0178) this console serves, from `EKOS_HOME`. Unset, the console has no
    # home: every workspace is in place and nothing below moves (RFC 0179).
    ekos_home: str = Field(default="", validation_alias="EKOS_HOME")

    # SQLite file for the console's own state (workspace registry, runs, schedules). Empty means
    # the default: `<home>/control/console.db` with a home, `.ekos-web/console.db` without.
    console_db: str = ""

    # Loopback port range the supervisor allocates per-workspace MCP servers from.
    mcp_port_base: int = 7400
    # RFC 0182: a project's MCP server starts on first use. One unused for this long is stopped
    # (0 keeps them), and at most `mcp_max_servers` run at once; starting another stops the one
    # used longest ago. Each server holds its project's index in memory.
    mcp_idle_seconds: int = 900
    mcp_max_servers: int = 8

    # ── Chat (RFC 0187) ─────────────────────────────────────────────────────
    # `anthropic`, `openai` (any OpenAI-compatible host), `ollama`, or empty for no chat.
    chat_provider: str = ""
    chat_model: str = ""
    chat_base_url: str = ""
    # The NAME of the variable holding the key; the value comes from the environment or the
    # keychain entry of that name (RFC 0184), never from a file.
    chat_api_key_env: str = ""
    chat_max_tool_calls: int = 12
    chat_max_tokens: int = 2048

    # Job runner (RFC 0131): per-workspace queue depth, and where run logs land.
    run_queue_depth: int = 16
    # Each stage of a chained compile run (pipeline, Init & compile). A single command keeps its
    # own limit (30 min); a large project's commit takes longer than that (RFC 0189 amendment).
    run_stage_timeout_seconds: int = Field(default=6 * 3600, ge=60)
    # Empty means the default: `<home>/control/runs` with a home, `.ekos-web/runs` without.
    runs_dir: str = ""

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

    @model_validator(mode="after")
    def _state_under_the_home(self) -> Settings:
        base = Path(self.ekos_home) / "control" if self.ekos_home else Path(".ekos-web")
        self.console_db = self.console_db or str(base / "console.db")
        self.runs_dir = self.runs_dir or str(base / "runs")
        return self

    @model_validator(mode="after")
    def _local_mode_is_alone(self) -> Settings:
        if self.mode != "local":
            return self
        if self.oidc_enabled:
            raise ValueError(
                "local mode signs in without a provider; unset EKOS_CONSOLE_OIDC_ISSUER or use "
                "mode = team"
            )
        if self.production:
            raise ValueError("local mode is for one person on one machine; production is team mode")
        return self

    @property
    def local(self) -> bool:
        return self.mode == "local"

    @property
    def control_dir(self) -> Path:
        """Where the console keeps its state: beside its database."""
        return Path(self.console_db).parent

    @model_validator(mode="after")
    def _scopes_include_openid(self) -> Settings:
        scopes = self.oidc_scopes.split()
        if "openid" not in scopes:
            raise ValueError("EKOS_CONSOLE_OIDC_SCOPES must include `openid`")
        self.oidc_scopes = " ".join(scopes)
        return self

    @model_validator(mode="after")
    def _production_needs_real_sign_in(self) -> Settings:
        if not self.production:
            return self
        missing = []
        if not self.oidc_enabled:
            missing.append("EKOS_CONSOLE_OIDC_ISSUER (static tokens are for development)")
        if not self.owner_set():
            missing.append("EKOS_CONSOLE_OWNERS (nobody could administer the console)")
        # With a home, a generated key is kept in it (`_home_session_key`), as good as a set one.
        if self.session_secret in _PLACEHOLDER_SESSION_SECRETS and not self.ekos_home:
            missing.append("EKOS_CONSOLE_SESSION_SECRET")
        if missing:
            raise ValueError("EKOS_CONSOLE_PRODUCTION is set; also set " + "; ".join(missing))
        return self

    @model_validator(mode="after")
    def _no_published_session_secret(self) -> Settings:
        """Found 2026-09-28: with no `EKOS_CONSOLE_SESSION_SECRET` set, anyone could sign
        `{"user": {"role": "write"}}` with the repo's placeholder and hold write access — including
        in token mode with no write token configured, because the session is checked first."""
        if self.session_secret in _PLACEHOLDER_SESSION_SECRETS:
            kept = _home_session_key(self.ekos_home) if self.ekos_home else None
            if kept is not None:
                self.session_secret = kept
            else:
                log.warning(
                    "EKOS_CONSOLE_SESSION_SECRET is unset or the published placeholder; using a "
                    "random per-process secret. Sessions will not survive a restart — set it to "
                    "keep them."
                )
                self.session_secret = secrets.token_urlsafe(32)
        if not self.oidc_enabled and not self.local and self.console_token == DEFAULT_CONSOLE_TOKEN:
            log.warning(
                "EKOS_CONSOLE_CONSOLE_TOKEN is the published default %r: anyone who can reach this "
                "console can read every workspace. Set it in api/.env for anything but local use.",
                DEFAULT_CONSOLE_TOKEN,
            )
        return self

    def workspace_seeds(self) -> list[WorkspaceSeed]:
        raw = json.loads(self.workspaces_json or "[]")
        return [WorkspaceSeed(**w) for w in raw]


def _home_session_key(ekos_home: str) -> str | None:
    """RFC 0185: with no session secret set, a console with a home keeps a generated one in
    `control/session.key`, readable by its owner only. Without it every restart (the Settings
    page's own button included) signed everyone out. It sits beside the console's database,
    which an attacker with that directory has anyway."""
    path = Path(ekos_home) / "control" / "session.key"
    try:
        if path.is_file():
            kept = path.read_text(encoding="utf-8").strip()
            if len(kept) >= 32:
                return kept
        path.parent.mkdir(parents=True, exist_ok=True)
        key = secrets.token_urlsafe(32)
        tmp = path.with_name("session.key.tmp")
        tmp.write_text(key, encoding="utf-8")
        tmp.chmod(0o600)
        tmp.replace(path)
        return key
    except OSError as exc:
        log.warning("cannot keep a session key in %s: %s", path, exc)
        return None


_settings: Settings | None = None


def get_settings() -> Settings:
    global _settings
    if _settings is None:
        _settings = Settings()
    return _settings
