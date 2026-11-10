"""What a person does through a channel (RFC 0253 §3.2, §3.3, §3.6): link their account, send
material, save or discard a card, say "not me", withdraw what they said.

Every action here is the linked person's own, checked here: a button press counts only from the
account it was sent to, a card is saved only by its author, a write needs the same right as in
the console. The writers run the CLI through `people_write`; no agent code reaches this module.
"""

from __future__ import annotations

import hashlib
import secrets
import uuid
from datetime import UTC, datetime, timedelta

from .. import authz, models, people_write, proposals, semantics_write
from ..auth import LOCAL, Principal, user_principal
from ..settings import Settings
from .core import Button

# A code a person types: 8 characters from an alphabet with nothing that looks alike.
_ALPHABET = "ABCDEFGHJKMNPQRSTUVWXYZ23456789"
_CODE_LEN = 8


class ActionRefused(RuntimeError):
    """Said back to the person, as it is."""


# ── linking ──────────────────────────────────────────────────────────────────


def _hash(code: str) -> str:
    return hashlib.sha256(code.strip().upper().encode()).hexdigest()


def new_link_code(channel: str, account: str, settings: Settings) -> str:
    """A one-time code for `/ekos link`; only its hash is kept."""
    code = "".join(secrets.choice(_ALPHABET) for _ in range(_CODE_LEN))
    models.put_link_code(
        models.LinkCode(
            code_hash=_hash(code),
            channel=channel,
            account=account,
            expires_at=datetime.now(UTC) + timedelta(minutes=settings.channels_link_code_minutes),
        )
    )
    return code


def label_of(principal: Principal) -> str:
    """How a linked person is named in writes and digests."""
    if principal.kind == "local":
        return "local"
    return principal.email or principal.subject or principal.id


def complete_link(principal: Principal, code: str) -> models.ChannelAccount:
    """The signed-in person enters the code: their account is linked. A code works once."""
    if principal.kind not in ("user", "local"):
        raise ActionRefused("only a signed-in person can link a channel account")
    row = models.take_link_code(_hash(code))
    if row is None:
        raise ActionRefused("that code is unknown or already used; write /ekos link again")
    expires = row.expires_at if row.expires_at.tzinfo else row.expires_at.replace(tzinfo=UTC)
    if expires < datetime.now(UTC):
        raise ActionRefused("that code has expired; write /ekos link again")
    previous = models.get_channel_account(row.channel, row.account)
    account = models.ChannelAccount(
        id=f"{row.channel}:{row.account}",
        channel=row.channel,
        account=row.account,
        principal_kind=principal.kind,
        principal_id=principal.id,
        label=label_of(principal),
        default_workspace=previous.default_workspace if previous else "",
        dm_ref=previous.dm_ref if previous else "",
    )
    models.save_channel_account(account)
    authz.audit(principal, f"channels: link {row.channel}", "ok", detail=f"account={row.account}")
    return account


def principal_of(account: models.ChannelAccount, settings: Settings) -> Principal | None:
    """The person a linked account acts as, with the roles they hold now (claims from an OIDC
    sign-in are not carried: only bindings and named groups count from a channel)."""
    if account.principal_kind == "local":
        return LOCAL if settings.local else None
    if account.principal_kind != "user":
        return None
    user = models.get_user(account.principal_id)
    return user_principal(user, [], settings) if user is not None else None


# ── buttons ──────────────────────────────────────────────────────────────────


def button(label: str, channel: str, account: str, ws_id: str, **payload: str) -> Button:
    ref = uuid.uuid4().hex[:20]
    models.add(
        models.ButtonRef(
            id=ref, channel=channel, account=account, workspace_id=ws_id, payload=payload
        )
    )
    return Button(label=label, ref=ref)


def pressed(channel: str, account: str, ref: str) -> models.ButtonRef:
    """The button's row when this account may press it: the one it was sent to."""
    row = models.get_button(ref)
    if row is None or row.channel != channel:
        raise ActionRefused("that button is no longer valid")
    if row.account != account:
        raise ActionRefused("this button is for the person it was sent to")
    return row


# ── material ─────────────────────────────────────────────────────────────────


def may_send(settings: Settings) -> None:
    if settings.public:
        raise ActionRefused("this deployment does not take material")


async def intake_bytes(
    settings: Settings,
    principal: Principal,
    ws: models.Workspace,
    name: str,
    data: bytes,
    *,
    channel: str,
    permalink: str,
) -> dict:
    """A file a person sent: into the project's inbox as theirs; pending unless they may accept."""
    from ..routes import inbox

    may_send(settings)
    if len(data) > settings.inbox_upload_max_mb * 1024 * 1024:
        raise ActionRefused(f"{name} is larger than {settings.inbox_upload_max_mb} MB")
    return await inbox.store_bytes(
        settings,
        ws,
        name,
        data,
        label_of(principal),
        channel=channel,
        permalink=permalink or None,
        pending=inbox.pending_for(principal, ws),
    )


async def intake_link(
    settings: Settings,
    principal: Principal,
    ws: models.Workspace,
    url: str,
    *,
    channel: str,
    permalink: str,
) -> dict:
    from ..routes import inbox

    may_send(settings)
    return await inbox.store_link(
        settings,
        ws,
        url,
        label_of(principal),
        channel=channel,
        permalink=permalink or None,
        pending=inbox.pending_for(principal, ws),
    )


# ── cards ────────────────────────────────────────────────────────────────────


def _card(pid: str, ws: models.Workspace, principal: Principal) -> models.Proposal:
    p = models.get_proposal(pid)
    if p is None or p.workspace_id != ws.id or p.author != proposals.author_key(principal):
        raise ActionRefused("only the person who said it can save or change this card")
    if p.status != "open":
        raise ActionRefused(f"this card is already {p.status}")
    return p


async def _run(settings: Settings, ws: models.Workspace, argv: list[str]) -> dict:
    try:
        return await people_write.run_json(settings.ekos_bin, ws.path, argv)
    except semantics_write.LedgerBusy as exc:
        raise ActionRefused("the project is busy (a run holds the ledger); try again") from exc
    except (semantics_write.CliFailed, semantics_write.ReviewError) as exc:
        raise ActionRefused(str(exc)) from exc


async def save(
    settings: Settings, principal: Principal, ws: models.Workspace, pid: str, *, channel: str
) -> list[dict]:
    """Save: the author's own words become testimony, attributed to the author."""
    p = _card(pid, ws, principal)
    if not authz.authorize(principal, "review", ws.tenant_id, ws.id):
        raise ActionRefused(
            "recording what you said needs the reviewer role on this project; "
            "ask an admin, or tell a reviewer"
        )
    who = label_of(principal)
    out = [await _run(settings, ws, argv) for argv in proposals.save_argvs(p, who)]
    models.set_proposal_status(p.id, "saved", saved_by=who)
    authz.audit(
        principal,
        "testimony: save",
        "ok",
        tenant_id=ws.tenant_id,
        workspace_id=ws.id,
        detail=f"channel={channel} message={p.message_ref}",
    )
    return out


def discard(principal: Principal, ws: models.Workspace, pid: str) -> None:
    models.set_proposal_status(_card(pid, ws, principal).id, "discarded")


def edit(principal: Principal, ws: models.Workspace, pid_prefix: str, text: str) -> models.Proposal:
    """`/ekos edit <card> <text>`: the person's corrected words replace theirs on the card."""
    text = text.strip()
    if not text:
        raise ActionRefused("give your corrected text after the card id")
    cards = [
        p
        for p in _open_cards(principal, ws)
        if p.id.startswith(pid_prefix.lower()) and len(pid_prefix) >= 4
    ]
    if len(cards) != 1:
        raise ActionRefused("no single open card of yours starts with that id")
    with models.session() as s:
        row = s.get(models.Proposal, cards[0].id)
        assert row is not None
        row.text = text[:4000]
        s.add(row)
        s.commit()
        s.refresh(row)
        return row


def _open_cards(principal: Principal, ws: models.Workspace) -> list[models.Proposal]:
    from sqlmodel import select

    with models.session() as s:
        stmt = select(models.Proposal).where(
            models.Proposal.workspace_id == ws.id,
            models.Proposal.author == proposals.author_key(principal),
            models.Proposal.status == "open",
        )
        return list(s.exec(stmt).all())


async def not_me(settings: Settings, principal: Principal, ws: models.Workspace, qid: str) -> None:
    await _run(settings, ws, people_write.decline_argv(qid, label_of(principal)))


async def withdraw(
    settings: Settings, principal: Principal, ws: models.Workspace, tid: str
) -> dict:
    if not authz.authorize(principal, "review", ws.tenant_id, ws.id):
        raise ActionRefused("withdrawing needs the reviewer role on this project")
    try:
        argv = people_write.testimony_withdraw_argv(tid, label_of(principal))
    except people_write.ReviewError as exc:
        raise ActionRefused(str(exc)) from exc
    return await _run(settings, ws, argv)
