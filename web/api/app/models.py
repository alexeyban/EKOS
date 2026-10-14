"""Console persistence — SQLite via SQLModel (RFC 0127 §8.1, RFC 0129 §3).

Phase 1 defines exactly one table: the workspace registry. `Run`, `Schedule`, and any `User`
table are added by the phases that need them (3, 4, and whichever introduces the role split).
The rows are low-volume and every query is by primary key, so the sync SQLModel `Session` is
used directly from async routes — a threadpool hop would cost more than the query.
"""

from __future__ import annotations

from datetime import UTC, datetime
from pathlib import Path
from typing import Any

from sqlalchemy import Column, inspect, text
from sqlalchemy.types import JSON
from sqlmodel import Field, Session, SQLModel, create_engine, select

DEFAULT_TENANT = "default"


def _now() -> datetime:
    return datetime.now(UTC)


class Workspace(SQLModel, table=True):
    """One registered workspace. `path` is an absolute directory containing `ekos.toml` and
    `.ekos/` — validated before the row is written."""

    id: str = Field(primary_key=True)
    name: str
    path: str
    # RFC 0180. Rows written before it belong to `default` (migration 1).
    tenant_id: str = Field(default=DEFAULT_TENANT, index=True)
    created_at: datetime = Field(default_factory=_now)


# ── RFC 0180: tenants, users, groups, roles, tokens, audit ───────────────────


class SchemaVersion(SQLModel, table=True):
    """One row: the number of the last migration applied."""

    id: int = Field(default=1, primary_key=True)
    version: int = 0


class Tenant(SQLModel, table=True):
    id: str = Field(primary_key=True)
    name: str
    # Directories a project of this tenant may read. Set by a platform owner only.
    source_roots: list[str] = Field(default_factory=list, sa_column=Column(JSON))
    # RFC 0183: names of the environment variables (model keys) this tenant's commands receive,
    # and whether they receive them at all. Names only; a value is never stored.
    model_env: list[str] = Field(default_factory=list, sa_column=Column(JSON))
    allow_metered: bool = False
    created_at: datetime = Field(default_factory=_now)


class User(SQLModel, table=True):
    """A person. `subject` is the OIDC `sub`; it is empty for someone given a role by e-mail who
    has not signed in yet, and the first sign-in with that e-mail fills it."""

    id: str = Field(primary_key=True)
    subject: str = Field(default="", index=True)
    email: str = Field(default="", index=True)
    created_at: datetime = Field(default_factory=_now)
    last_login_at: datetime | None = None


class Group(SQLModel, table=True):
    id: str = Field(primary_key=True)
    tenant_id: str = Field(index=True)
    name: str
    # When set, every user whose sign-in carries this value in the role claim is a member.
    oidc_value: str = ""
    created_at: datetime = Field(default_factory=_now)


class GroupMember(SQLModel, table=True):
    group_id: str = Field(primary_key=True)
    user_id: str = Field(primary_key=True)


class RoleBinding(SQLModel, table=True):
    """`role` for one principal on a tenant, or on one workspace of it when `workspace_id` is
    set."""

    id: str = Field(primary_key=True)
    tenant_id: str = Field(index=True)
    principal_kind: str  # "user" | "group" | "token"
    principal_id: str = Field(index=True)
    role: str  # "viewer" | "reviewer" | "operator" | "admin"
    workspace_id: str | None = None
    # RFC 0181: a rule narrower than the workspace. With any of `source`, `path`, `object_id`
    # set, an `allow` binding shows only what the rule matches and a `deny` binding hides it.
    effect: str = "allow"  # "allow" | "deny"
    source: str | None = None
    path: str | None = None
    object_id: str | None = None
    created_by: str = ""
    created_at: datetime = Field(default_factory=_now)


class ApiToken(SQLModel, table=True):
    """An agent's credential. Only the hash of the token is stored."""

    id: str = Field(primary_key=True)
    tenant_id: str = Field(index=True)
    name: str
    token_hash: str = Field(index=True, unique=True)
    prefix: str  # the first characters, shown in lists so a token can be recognised
    created_by: str = ""
    created_at: datetime = Field(default_factory=_now)
    expires_at: datetime | None = None
    last_used_at: datetime | None = None
    revoked_at: datetime | None = None


class AuditLog(SQLModel, table=True):
    id: int | None = Field(default=None, primary_key=True)
    at: datetime = Field(default_factory=_now, index=True)
    actor: str
    actor_kind: str  # "user" | "token" | "static" | "anonymous"
    action: str
    tenant_id: str = Field(default="", index=True)
    workspace_id: str = ""
    outcome: str  # "allowed" | "denied" | "ok" | "failed"
    detail: str = ""


class Conversation(SQLModel, table=True):
    """RFC 0187: a chat in one workspace, owned by the person who started it."""

    id: str = Field(primary_key=True)
    workspace_id: str = Field(index=True)
    owner: str = Field(index=True)  # `Principal.kind:Principal.id`
    title: str = ""
    created_at: datetime = Field(default_factory=_now)
    updated_at: datetime = Field(default_factory=_now)


class ChatMessage(SQLModel, table=True):
    id: int | None = Field(default=None, primary_key=True)
    conversation_id: str = Field(index=True)
    # The internal message shape of `app/chat/providers.py`, as JSON.
    body: dict = Field(default_factory=dict, sa_column=Column(JSON))
    created_at: datetime = Field(default_factory=_now)


TERMINAL = {"succeeded", "failed", "cancelled", "timed_out", "interrupted"}


class Run(SQLModel, table=True):
    """One command execution (RFC 0131 §3)."""

    id: str = Field(primary_key=True)
    workspace_id: str = Field(index=True)
    command: str
    params: dict[str, Any] = Field(default_factory=dict, sa_column=Column(JSON))
    status: str = Field(default="queued", index=True)
    stages: list[dict[str, Any]] = Field(default_factory=list, sa_column=Column(JSON))
    exit_code: int | None = None
    log_path: str = ""
    created_at: datetime = Field(default_factory=_now)
    started_at: datetime | None = None
    ended_at: datetime | None = None


class Schedule(SQLModel, table=True):
    """A recurring command run (RFC 0132). This row is the source of truth; APScheduler is
    rebuilt from the enabled rows on every console start."""

    id: str = Field(primary_key=True)
    workspace_id: str = Field(index=True)
    command: str
    params: dict[str, Any] = Field(default_factory=dict, sa_column=Column(JSON))
    trigger_kind: str  # "cron" | "interval"
    trigger_expr: str  # cron: 5-field crontab string; interval: integer seconds as text
    notify_url: str  # required — POSTed on a non-succeeded terminal run
    enabled: bool = True
    created_at: datetime = Field(default_factory=_now)
    last_run_at: datetime | None = None
    last_run_id: str | None = None
    last_status: str | None = None


_engine = None


def _migration_1(conn) -> None:
    """RFC 0180: every workspace belongs to a tenant. `create_all` adds new tables and never
    alters an existing one, so the column is added here."""
    columns = {c["name"] for c in inspect(conn).get_columns("workspace")}
    if "tenant_id" not in columns:
        conn.execute(
            text("ALTER TABLE workspace ADD COLUMN tenant_id VARCHAR NOT NULL DEFAULT 'default'")
        )
        conn.execute(
            text("CREATE INDEX IF NOT EXISTS ix_workspace_tenant_id ON workspace (tenant_id)")
        )


def _migration_2(conn) -> None:
    """RFC 0181: a role binding may carry a rule (effect, source, path, object)."""
    columns = {c["name"] for c in inspect(conn).get_columns("rolebinding")}
    for name, ddl in [
        ("effect", "VARCHAR NOT NULL DEFAULT 'allow'"),
        ("source", "VARCHAR"),
        ("path", "VARCHAR"),
        ("object_id", "VARCHAR"),
    ]:
        if name not in columns:
            conn.execute(text(f"ALTER TABLE rolebinding ADD COLUMN {name} {ddl}"))


# Ordered. Migration N brings a database at version N-1 to version N. Never edit one that has
# shipped; add the next.
def _migration_3(conn) -> None:
    """RFC 0183: per-tenant model credentials by variable name, and the metered switch."""
    columns = {c["name"] for c in inspect(conn).get_columns("tenant")}
    if "model_env" not in columns:
        conn.execute(text("ALTER TABLE tenant ADD COLUMN model_env JSON"))
    if "allow_metered" not in columns:
        conn.execute(text("ALTER TABLE tenant ADD COLUMN allow_metered BOOLEAN NOT NULL DEFAULT 0"))


MIGRATIONS = [_migration_1, _migration_2, _migration_3]


def _migrate(engine) -> None:
    """Bring the database to the current schema. A database from before versioning has the
    `workspace` table and no version row: that is version 0. A new one starts at the latest."""
    existed = "workspace" in inspect(engine).get_table_names()
    SQLModel.metadata.create_all(engine)
    with engine.begin() as conn:
        row = conn.execute(text("SELECT version FROM schemaversion WHERE id = 1")).first()
        if row is None:
            version = 0 if existed else len(MIGRATIONS)
            conn.execute(
                text("INSERT INTO schemaversion (id, version) VALUES (1, :v)"), {"v": version}
            )
        else:
            version = row[0]
        for number, migration in enumerate(MIGRATIONS, start=1):
            if number > version:
                migration(conn)
                conn.execute(
                    text("UPDATE schemaversion SET version = :v WHERE id = 1"), {"v": number}
                )


def init_engine(db_path: str) -> None:
    """Create the SQLite file (and its parent dir), bring its schema up to date, and make sure
    the `default` tenant exists. Idempotent."""
    global _engine
    p = Path(db_path)
    p.parent.mkdir(parents=True, exist_ok=True)
    _engine = create_engine(f"sqlite:///{p}", connect_args={"check_same_thread": False})
    _migrate(_engine)
    with session() as s:
        if s.get(Tenant, DEFAULT_TENANT) is None:
            s.add(Tenant(id=DEFAULT_TENANT, name="Default"))
            s.commit()


def session() -> Session:
    if _engine is None:  # pragma: no cover - guarded by lifespan
        raise RuntimeError("models.init_engine() has not been called")
    # expire_on_commit=False: callers read attributes off the returned rows after the `with`
    # block closes. `Workspace` has no relationships, so every column is already loaded — the
    # detached instances stay fully usable.
    return Session(_engine, expire_on_commit=False)


def list_workspaces() -> list[Workspace]:
    with session() as s:
        return list(s.exec(select(Workspace).order_by(Workspace.created_at)))


def get_workspace(workspace_id: str) -> Workspace | None:
    with session() as s:
        return s.get(Workspace, workspace_id)


def add_workspace(ws: Workspace) -> None:
    with session() as s:
        s.add(ws)
        s.commit()


def delete_workspace(workspace_id: str) -> bool:
    with session() as s:
        row = s.get(Workspace, workspace_id)
        if row is None:
            return False
        s.delete(row)
        s.commit()
        return True


# ── Run ──────────────────────────────────────────────────────────────────────


def add_run(run: Run) -> None:
    with session() as s:
        s.add(run)
        s.commit()


def get_run(run_id: str) -> Run | None:
    with session() as s:
        return s.get(Run, run_id)


def list_runs(
    workspace_id: str | None = None, status: str | None = None, limit: int = 100
) -> list[Run]:
    with session() as s:
        q = select(Run).order_by(Run.created_at.desc()).limit(limit)
        if workspace_id:
            q = q.where(Run.workspace_id == workspace_id)
        if status:
            q = q.where(Run.status == status)
        return list(s.exec(q))


def update_run(run_id: str, **fields: Any) -> None:
    with session() as s:
        row = s.get(Run, run_id)
        if row is None:  # pragma: no cover - the runner always creates the row first
            return
        for k, v in fields.items():
            setattr(row, k, v)
        s.add(row)
        s.commit()


def sweep_stale_runs() -> int:
    """On startup, any run left `queued`/`running` (the console died mid-run) is `interrupted`."""
    with session() as s:
        rows = list(s.exec(select(Run).where(Run.status.in_(["queued", "running"]))))
        for row in rows:
            row.status = "interrupted"
            row.ended_at = _now()
            s.add(row)
        s.commit()
        return len(rows)


# ── Schedule ─────────────────────────────────────────────────────────────────


def add_schedule(sched: Schedule) -> None:
    with session() as s:
        s.add(sched)
        s.commit()


def get_schedule(schedule_id: str) -> Schedule | None:
    with session() as s:
        return s.get(Schedule, schedule_id)


def list_schedules(workspace_id: str | None = None) -> list[Schedule]:
    with session() as s:
        q = select(Schedule).order_by(Schedule.created_at)
        if workspace_id:
            q = q.where(Schedule.workspace_id == workspace_id)
        return list(s.exec(q))


def update_schedule(schedule_id: str, **fields: Any) -> Schedule | None:
    with session() as s:
        row = s.get(Schedule, schedule_id)
        if row is None:
            return None
        for k, v in fields.items():
            setattr(row, k, v)
        s.add(row)
        s.commit()
        s.refresh(row)
        return row


def delete_schedule(schedule_id: str) -> bool:
    with session() as s:
        row = s.get(Schedule, schedule_id)
        if row is None:
            return False
        s.delete(row)
        s.commit()
        return True


# ── RFC 0180 ─────────────────────────────────────────────────────────────────


def add(row: SQLModel) -> None:
    with session() as s:
        s.add(row)
        s.commit()


def get_tenant(tenant_id: str) -> Tenant | None:
    with session() as s:
        return s.get(Tenant, tenant_id)


def list_tenants() -> list[Tenant]:
    with session() as s:
        return list(s.exec(select(Tenant).order_by(Tenant.id)))


def update_tenant(tenant_id: str, **fields: Any) -> Tenant | None:
    with session() as s:
        row = s.get(Tenant, tenant_id)
        if row is None:
            return None
        for k, v in fields.items():
            setattr(row, k, v)
        s.add(row)
        s.commit()
        s.refresh(row)
        return row


def get_user(user_id: str) -> User | None:
    with session() as s:
        return s.get(User, user_id)


def user_by_subject(subject: str) -> User | None:
    with session() as s:
        return s.exec(select(User).where(User.subject == subject)).first()


def user_by_email(email: str) -> User | None:
    with session() as s:
        return s.exec(select(User).where(User.email == email.lower())).first()


def touch_user(user_id: str, **fields: Any) -> None:
    with session() as s:
        row = s.get(User, user_id)
        if row is None:
            return
        for k, v in fields.items():
            setattr(row, k, v)
        s.add(row)
        s.commit()


def list_groups(tenant_id: str) -> list[Group]:
    with session() as s:
        return list(s.exec(select(Group).where(Group.tenant_id == tenant_id).order_by(Group.name)))


def get_group(group_id: str) -> Group | None:
    with session() as s:
        return s.get(Group, group_id)


def delete_group(group_id: str) -> None:
    """The group, its memberships and its bindings."""
    with session() as s:
        for m in s.exec(select(GroupMember).where(GroupMember.group_id == group_id)):
            s.delete(m)
        for b in s.exec(
            select(RoleBinding).where(
                RoleBinding.principal_kind == "group", RoleBinding.principal_id == group_id
            )
        ):
            s.delete(b)
        row = s.get(Group, group_id)
        if row is not None:
            s.delete(row)
        s.commit()


def group_members(group_id: str) -> list[str]:
    with session() as s:
        return [
            m.user_id for m in s.exec(select(GroupMember).where(GroupMember.group_id == group_id))
        ]


def set_group_member(group_id: str, user_id: str, present: bool) -> None:
    with session() as s:
        row = s.get(GroupMember, (group_id, user_id))
        if present and row is None:
            s.add(GroupMember(group_id=group_id, user_id=user_id))
        elif not present and row is not None:
            s.delete(row)
        s.commit()


def groups_of(user_id: str, claim_values: set[str]) -> list[str]:
    """Ids of every group the user is in: by a membership row, or by a claim value the group
    names."""
    with session() as s:
        ids = {
            m.group_id for m in s.exec(select(GroupMember).where(GroupMember.user_id == user_id))
        }
        if claim_values:
            for g in s.exec(select(Group).where(Group.oidc_value.in_(claim_values))):
                if g.oidc_value:
                    ids.add(g.id)
        return sorted(ids)


def list_bindings(tenant_id: str | None = None) -> list[RoleBinding]:
    with session() as s:
        q = select(RoleBinding).order_by(RoleBinding.created_at)
        if tenant_id is not None:
            q = q.where(RoleBinding.tenant_id == tenant_id)
        return list(s.exec(q))


def bindings_for(principals: list[tuple[str, str]]) -> list[RoleBinding]:
    """Every binding held by any of `(kind, id)`."""
    out: list[RoleBinding] = []
    with session() as s:
        for kind, pid in principals:
            out.extend(
                s.exec(
                    select(RoleBinding).where(
                        RoleBinding.principal_kind == kind, RoleBinding.principal_id == pid
                    )
                )
            )
    return out


def get_binding(binding_id: str) -> RoleBinding | None:
    with session() as s:
        return s.get(RoleBinding, binding_id)


def delete_binding(binding_id: str) -> None:
    with session() as s:
        row = s.get(RoleBinding, binding_id)
        if row is not None:
            s.delete(row)
            s.commit()


def token_by_hash(token_hash: str) -> ApiToken | None:
    with session() as s:
        return s.exec(select(ApiToken).where(ApiToken.token_hash == token_hash)).first()


def get_token(token_id: str) -> ApiToken | None:
    with session() as s:
        return s.get(ApiToken, token_id)


def list_tokens(tenant_id: str) -> list[ApiToken]:
    with session() as s:
        return list(
            s.exec(
                select(ApiToken)
                .where(ApiToken.tenant_id == tenant_id)
                .order_by(ApiToken.created_at)
            )
        )


def update_token(token_id: str, **fields: Any) -> None:
    with session() as s:
        row = s.get(ApiToken, token_id)
        if row is None:
            return
        for k, v in fields.items():
            setattr(row, k, v)
        s.add(row)
        s.commit()


def list_audit(tenant_id: str | None, limit: int = 200) -> list[AuditLog]:
    with session() as s:
        q = select(AuditLog).order_by(AuditLog.id.desc()).limit(limit)
        if tenant_id is not None:
            q = q.where(AuditLog.tenant_id == tenant_id)
        return list(s.exec(q))


# ── chat (RFC 0187) ──────────────────────────────────────────────────────────


def list_conversations(workspace_id: str, owner: str) -> list[Conversation]:
    with session() as s:
        stmt = (
            select(Conversation)
            .where(Conversation.workspace_id == workspace_id, Conversation.owner == owner)
            .order_by(Conversation.updated_at.desc())  # type: ignore[attr-defined]
        )
        return list(s.exec(stmt).all())


def get_conversation(conversation_id: str) -> Conversation | None:
    with session() as s:
        return s.get(Conversation, conversation_id)


def chat_messages(conversation_id: str) -> list[dict]:
    with session() as s:
        stmt = (
            select(ChatMessage)
            .where(ChatMessage.conversation_id == conversation_id)
            .order_by(ChatMessage.id)  # type: ignore[arg-type]
        )
        return [m.body for m in s.exec(stmt).all()]


def add_chat_messages(conversation_id: str, bodies: list[dict]) -> None:
    with session() as s:
        for b in bodies:
            s.add(ChatMessage(conversation_id=conversation_id, body=b))
        conv = s.get(Conversation, conversation_id)
        if conv is not None:
            conv.updated_at = _now()
            s.add(conv)
        s.commit()


def delete_conversation(conversation_id: str) -> None:
    with session() as s:
        for m in s.exec(
            select(ChatMessage).where(ChatMessage.conversation_id == conversation_id)
        ).all():
            s.delete(m)
        conv = s.get(Conversation, conversation_id)
        if conv is not None:
            s.delete(conv)
        s.commit()
