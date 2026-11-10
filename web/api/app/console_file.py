"""`console.toml`: the console's settings in a file (RFC 0184 §1).

Precedence, highest first: environment variables, console secrets in the keychain, this file,
defaults. The file is `<EKOS_HOME>/control/console.toml`, or `EKOS_CONSOLE_CONFIG_FILE`; with
neither there is no file and the console reads the environment only, as before.

One key per `Settings` field, named as the field. `FIELDS` says how each is shown and whether
the console may change it. Secret fields are never written here by the console.
"""

from __future__ import annotations

import os
from contextlib import contextmanager
from contextvars import ContextVar
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import tomlkit
from pydantic.fields import FieldInfo
from pydantic_settings import BaseSettings, PydanticBaseSettingsSource

from . import keychain

FILE_NAME = "console.toml"
_DEMO = "Public demo"
_PEOPLE = "Channels"


@dataclass(frozen=True)
class Field:
    group: str
    help: str
    secret: bool = False
    # False for what says where the console is and where its state lives: the installer and
    # the environment decide those, not a page served by the console itself.
    editable: bool = True


FIELDS: dict[str, Field] = {
    "mode": Field(
        "Server",
        "`local`: one person on this machine, no sign-in provider. "
        "`team`: OIDC sign-in, tenants and roles",
    ),
    "host": Field("Server", "Address `ekos console` binds. Local mode accepts loopback only"),
    "port": Field("Server", "Port `ekos console` binds"),
    "ui_dir": Field("Server", "Directory of the built console UI", editable=False),
    "supervised": Field(
        "Server", "Set by a launcher that restarts the console when it exits", editable=False
    ),
    "dev_origin": Field("Server", "Origin of the UI development server, allowed through CORS"),
    "oidc_issuer": Field(
        "Sign-in", "OIDC issuer URL. Empty: no provider (development tokens in team mode)"
    ),
    "oidc_client_id": Field("Sign-in", "OIDC client id"),
    "oidc_client_secret": Field("Sign-in", "OIDC client secret", secret=True),
    "oidc_redirect_uri": Field(
        "Sign-in", "Where the provider sends the browser back: <console URL>/api/auth/callback"
    ),
    "oidc_scopes": Field(
        "Sign-in",
        "Scopes asked for, space-separated; must include `openid`. "
        "Add `groups` for Dex, Keycloak, Okta",
    ),
    "oidc_role_claim": Field("Sign-in", "Token claim whose values a group can follow"),
    "oidc_write_values": Field(
        "Sign-in",
        "Outside production mode: claim values that may "
        "change the `default` tenant, comma-separated",
    ),
    "post_login_redirect": Field("Sign-in", "Page after sign-in"),
    "session_secret": Field(
        "Sign-in",
        "Signs the session cookie. Unset: a random one per start, and sessions end at each restart",
        secret=True,
    ),
    "console_token": Field(
        "Sign-in", "Development read token (team mode without a provider)", secret=True
    ),
    "console_write_token": Field(
        "Sign-in", "Development owner token (team mode without a provider)", secret=True
    ),
    "owners": Field(
        "Access", "Platform owners: e-mail addresses or OIDC subjects, comma-separated"
    ),
    "production": Field("Access", "Refuse development credentials; access only through roles"),
    "workspaces_root": Field(
        "Access", "A workspace registered by path must be inside this directory"
    ),
    "mcp_token": Field("MCP", "Token for a hand-started MCP server", secret=True),
    "mcp_port_base": Field("MCP", "First loopback port for project MCP servers"),
    "mcp_idle_seconds": Field("MCP", "Stop a project's MCP server unused this long (0: never)"),
    "mcp_max_servers": Field("MCP", "Most project MCP servers running at once"),
    "mcp_call_timeout_seconds": Field(
        "MCP", "Seconds to wait for one reply from a project's MCP server"
    ),
    "chat_provider": Field(
        "Chat", "anthropic, openai (any OpenAI-compatible host), ollama, or empty for no chat"
    ),
    "chat_model": Field("Chat", "The model, e.g. claude-sonnet-5-5, qwen2.5:7b"),
    "chat_base_url": Field(
        "Chat",
        "The provider's address; empty for its default. A server "
        "on this machine is not paid per call",
    ),
    "chat_api_key_env": Field(
        "Chat",
        "Name of the variable holding the key (its value: the environment, or the keychain below)",
    ),
    "chat_max_tool_calls": Field("Chat", "Most EKOS tool calls the model may make per question"),
    "chat_max_tokens": Field("Chat", "Most tokens in one answer"),
    "inbox_upload_max_mb": Field(
        _PEOPLE, "Largest file a person may send to the inbox from the console or a channel (MB)"
    ),
    "channels_telegram_token_env": Field(
        _PEOPLE, "Name of the variable holding the Telegram bot token (its value: keychain below)"
    ),
    "channels_slack_bot_token_env": Field(
        _PEOPLE, "Name of the variable holding the Slack bot token (xoxb-…)"
    ),
    "channels_slack_app_token_env": Field(
        _PEOPLE,
        "Name of the variable holding the Slack app-level token (xapp-…) for Socket Mode; "
        "without it Slack posts to /api/channels/slack/events",
    ),
    "channels_slack_signing_secret_env": Field(
        _PEOPLE, "Name of the variable holding the Slack signing secret (Events API)"
    ),
    "channels_teams_app_id": Field(_PEOPLE, "Microsoft Teams bot's app (client) id"),
    "channels_teams_app_secret_env": Field(
        _PEOPLE, "Name of the variable holding the Teams bot's client secret"
    ),
    "channels_link_code_minutes": Field(
        _PEOPLE, "How long a `/ekos link` code a bot gives stays valid (minutes)"
    ),
    "channels_digest_cron": Field(
        _PEOPLE,
        "Cron (UTC) for sending each person their open questions through their linked channel; "
        "empty: none. Never sent from a public deployment",
    ),
    "mcp_metered_per_person_per_day": Field(
        _DEMO,
        "Calls a signed-in person may make per day to MCP tools that call a model "
        "(ekos_sql_generate, RFC 0250)",
    ),
    "mcp_metered_per_token_per_day": Field(
        _DEMO,
        "The same per API token: a service such as Superset, which limits its own users",
    ),
    "platform_auth": Field(
        "Sign-in",
        "aca: Azure Container Apps' authentication signs people in (GitHub, Google); "
        "only behind it (RFC 0251)",
    ),
    "platform_auth_providers": Field("Sign-in", "Providers on the sign-in page, comma-separated"),
    "signed_in_group": Field(
        "Sign-in", "A group every signed-in person is in: bind it to give everyone a role"
    ),
    "read_only": Field(
        _DEMO,
        "Read-only showcase: everyone, owners included, may only read; Run, Commands, Chat and "
        "ekos.toml are off (RFC 0251)",
    ),
    "moderation_enabled": Field(
        _DEMO,
        "Check every question before it is answered: bans, blocks and daily limits (RFC 0249)",
    ),
    "moderation_model": Field(
        _DEMO, "The model for the intent check, e.g. claude-haiku-5-5. Empty: no model"
    ),
    "moderation_base_url": Field(
        _DEMO, "The model host; empty for OpenCode Zen. A host on this machine is free"
    ),
    "moderation_key_env": Field(
        _DEMO,
        "Name of the variable holding the key (its value: the environment, or the keychain)",
    ),
    "moderation_trusted_hops": Field(
        _DEMO,
        "Proxies in front of the console whose X-Forwarded-For entry is trusted; 0 = none",
    ),
    "moderation_limits": Field(
        _DEMO,
        'The published numbers as JSON, e.g. {"questions_per_day": 30, "block_hours": 24}',
    ),
    "instance_lease": Field(
        "Cloud",
        "Keep one console writing this home: a new one takes over and the old one stops. On for "
        "a home on a network share (container platforms, Kubernetes)",
    ),
    "instance_lease_seconds": Field("Cloud", "Heartbeat of the instance lease"),
    "backup_url": Field(
        "Cloud",
        "Back up the home to object storage here (s3://, gs://, "
        "https://<account>.blob.core.windows.net/…). Empty: no scheduled backups",
    ),
    "backup_cron": Field("Cloud", "When to back up, as a crontab line in UTC"),
    "backup_restore_on_empty": Field(
        "Cloud",
        "When the home is empty at start (a new deployment or share), restore the newest backup "
        "under backup_url first. A location the console cannot read stops the start",
    ),
    "run_queue_depth": Field("Runs", "Commands queued per workspace before new ones are refused"),
    "run_stage_timeout_seconds": Field(
        "Runs",
        "Longest one stage of the pipeline or Init & compile may run (a large project's commit "
        "can take hours)",
    ),
    "ekos_bin": Field("Locations", "The `ekos` binary", editable=False),
    "ekos_home": Field("Locations", "The EKOS home", editable=False),
    "console_db": Field("Locations", "The console's database", editable=False),
    "runs_dir": Field("Locations", "Run logs", editable=False),
    "workspaces_json": Field(
        "Locations", "Workspaces to register on first start (JSON)", editable=False
    ),
}

SECRETS = frozenset(n for n, f in FIELDS.items() if f.secret)

# While a save is being validated: the would-be file content, read instead of the file.
_override: ContextVar[dict[str, Any] | None] = ContextVar("console_file_override", default=None)


def path(environ: dict[str, str] | None = None) -> Path | None:
    env = os.environ if environ is None else environ
    explicit = env.get("EKOS_CONSOLE_CONFIG_FILE", "").strip()
    if explicit:
        return Path(explicit).expanduser()
    home = env.get("EKOS_HOME", "").strip()
    return Path(home).expanduser() / "control" / FILE_NAME if home else None


def read(p: Path | None = None) -> dict[str, Any]:
    """The file's keys, as plain Python values. A missing file is empty."""
    override = _override.get()
    if override is not None:
        return dict(override)
    p = path() if p is None else p
    if p is None or not p.is_file():
        return {}
    return tomlkit.parse(p.read_text(encoding="utf-8")).unwrap()


def updated_text(current: str, values: dict[str, Any]) -> str:
    """`current` with each key of `values` set, or removed when its value is `None`. Comments
    and the order of untouched keys are kept."""
    doc = tomlkit.parse(current)
    for key, value in values.items():
        if value is None:
            if key in doc:
                del doc[key]
        else:
            doc[key] = value
    return tomlkit.dumps(doc)


def write(p: Path, values: dict[str, Any]) -> None:
    current = p.read_text(encoding="utf-8") if p.is_file() else ""
    text = updated_text(current, values)
    p.parent.mkdir(parents=True, exist_ok=True)
    tmp = p.with_name(p.name + ".tmp")
    tmp.write_text(text, encoding="utf-8")
    tmp.replace(p)


@contextmanager
def reading(content: dict[str, Any]):
    """Within the block, settings are built from `content` instead of the file."""
    token = _override.set(content)
    try:
        yield
    finally:
        _override.reset(token)


# ── settings sources ─────────────────────────────────────────────────────────


def _key(name: str, field: FieldInfo) -> str:
    """What the model validates a value under: the field's alias when it has one."""
    alias = field.validation_alias
    return alias if isinstance(alias, str) else name


class FileSource(PydanticBaseSettingsSource):
    def get_field_value(self, field: FieldInfo, field_name: str) -> tuple[Any, str, bool]:
        return None, field_name, False  # unused: `__call__` returns everything at once

    def __call__(self) -> dict[str, Any]:
        data = read()
        fields = self.settings_cls.model_fields
        return {_key(n, fields[n]): v for n, v in data.items() if n in fields}


class KeychainSource(PydanticBaseSettingsSource):
    def get_field_value(self, field: FieldInfo, field_name: str) -> tuple[Any, str, bool]:
        return None, field_name, False

    def __call__(self) -> dict[str, Any]:
        fields = self.settings_cls.model_fields
        out = {}
        for name in SECRETS:
            if name in fields:
                value = keychain.get_console(name)
                if value:
                    out[_key(name, fields[name])] = value
        return out


def env_name(name: str, field: FieldInfo, prefix: str) -> str:
    alias = field.validation_alias
    return alias if isinstance(alias, str) else (prefix + name).upper()


def _dotenv_keys(settings_cls: type[BaseSettings]) -> set[str]:
    """Keys of the `.env` file the settings read (a development convenience; it ranks with the
    environment)."""
    env_file = settings_cls.model_config.get("env_file")
    if not env_file or not Path(str(env_file)).is_file():
        return set()
    from dotenv import dotenv_values

    return {k.upper() for k in dotenv_values(str(env_file))}


def set_by_environment(settings_cls: type[BaseSettings], name: str) -> bool:
    field = settings_cls.model_fields[name]
    wanted = env_name(name, field, settings_cls.model_config.get("env_prefix", "")).upper()
    return any(k.upper() == wanted for k in os.environ) or wanted in _dotenv_keys(settings_cls)


def source_of(settings_cls: type[BaseSettings], name: str, file_values: dict[str, Any]) -> str:
    if set_by_environment(settings_cls, name):
        return "environment"
    if name in SECRETS and keychain.get_console(name):
        return "keychain"
    if name in file_values:
        return "file"
    return "default"
