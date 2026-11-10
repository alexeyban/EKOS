"""The channels service (RFC 0253): one dispatcher for every adapter, and the background tasks
that receive from Telegram (long polling) and Slack (Socket Mode). Teams posts to the console.

It starts only when a channel is configured (its secrets named in the settings and present in
the environment or keychain). For each message it decides, the same way in every channel:

1. `/ekos link` works for anyone and answers with a one-time code (privately); any other message
   from an unlinked account gets the link instructions and nothing else;
2. the project is the channel's binding (set by an owner), or the person's own choice in a
   direct message;
3. a file or a lone link goes to the project's inbox as the person's;
4. `/ekos tell` and `/ekos answer` make a card of proposals only the person can save;
5. anything else is a question, answered with the person's read scope — privately when the
   answer read anything outside what every member of a group may read.
"""

from __future__ import annotations

import asyncio
import contextlib
import logging
import os
from typing import Any, Protocol

from .. import authz, keychain, models, people_write, readproc
from ..settings import Settings
from . import actions
from . import agent as channel_agent
from .core import (
    HELP,
    UNLINKED,
    Attachment,
    ButtonClick,
    ChannelMessage,
    OutMessage,
    answer_privately,
    command,
    link_instructions,
)

log = logging.getLogger("ekos.channels")


class Adapter(Protocol):
    name: str

    async def send(self, out: OutMessage) -> str: ...
    async def answer_click(self, click: ButtonClick, text: str) -> None: ...
    async def download(self, att: Attachment) -> bytes: ...
    async def members(self, msg: ChannelMessage) -> list[str] | None: ...
    async def aclose(self) -> None: ...


def secret(env_name: str) -> str:
    """A channel secret by its variable's name: the environment, else the keychain. Never logged."""
    if not env_name:
        return ""
    return os.environ.get(env_name) or keychain.get_env(env_name) or ""


def configured(settings: Settings) -> dict[str, bool]:
    """Which channels have what they need to run."""
    return {
        "telegram": bool(secret(settings.channels_telegram_token_env)),
        "slack": bool(secret(settings.channels_slack_bot_token_env))
        and bool(
            secret(settings.channels_slack_app_token_env)
            or secret(settings.channels_slack_signing_secret_env)
        ),
        "teams": bool(settings.channels_teams_app_id)
        and bool(secret(settings.channels_teams_app_secret_env)),
    }


def _short(value: str) -> str:
    return value[:8]


class ChannelService:
    def __init__(self, settings: Settings, supervisor, adapters: dict[str, Any]) -> None:
        self.settings = settings
        self.supervisor = supervisor
        self.adapters = adapters
        self._stop = asyncio.Event()
        self._tasks: list[asyncio.Task] = []
        self._pending: set[asyncio.Task] = set()

    # ── messages ─────────────────────────────────────────────────────────────

    async def on_message(self, adapter, msg: ChannelMessage) -> list[OutMessage]:
        """Handle one message and send the replies; returns them (for tests and logs)."""
        try:
            replies = await self.handle(adapter, msg)
        except Exception as exc:
            log.warning("%s message failed: %s", msg.channel, type(exc).__name__)
            replies = [self._reply(msg, "Something went wrong; try again later.", private=True)]
        for out in replies:
            try:
                await adapter.send(out)
            except Exception as exc:
                log.warning("%s send failed: %s", msg.channel, exc)
        return replies

    def _reply(
        self, msg: ChannelMessage, text: str, *, private: bool = False, buttons=None
    ) -> OutMessage:
        return OutMessage(
            conversation=msg.conversation,
            text=text,
            buttons=list(buttons or []),
            private_to=msg.account if private and not msg.direct else "",
            thread=msg.thread,
            extra=dict(msg.extra),
        )

    async def handle(self, adapter, msg: ChannelMessage) -> list[OutMessage]:
        cmd = command(msg.text)
        if cmd and cmd[0] == "link":
            code = actions.new_link_code(msg.channel, msg.account, self.settings)
            text = (
                f"Your code: {code}\nEnter it on the console's Channels page while signed in, "
                f"within {self.settings.channels_link_code_minutes} minutes."
            )
            return [self._reply(msg, text, private=True)]
        account = models.get_channel_account(msg.channel, msg.account)
        if account is None:
            return [self._reply(msg, link_instructions(), private=True)]
        principal = actions.principal_of(account, self.settings)
        if principal is None:
            return [self._reply(msg, "The person this account was linked to is gone; link again.")]
        if msg.direct:
            self._remember_dm(account, adapter, msg)
        if cmd and cmd[0] == "help":
            return [self._reply(msg, HELP, private=True)]
        if cmd and cmd[0] == "project":
            return [self._reply(msg, self._choose_project(account, principal, msg, cmd[1]))]
        ws = self._workspace(account, msg)
        if ws is None or not authz.sees_workspace(principal, ws):
            return [self._reply(msg, self._no_project(principal, msg), private=True)]
        return await self._in_project(adapter, msg, cmd, principal, ws)

    async def _in_project(self, adapter, msg, cmd, principal, ws) -> list[OutMessage]:
        try:
            if msg.attachments or (not cmd and people_write.is_link(msg.text)):
                return [await self._intake(adapter, msg, principal, ws)]
            if cmd and cmd[0] in ("tell", "answer"):
                return [await self._tell(msg, cmd, principal, ws)]
            if cmd and cmd[0] == "edit":
                pid, _, text = cmd[1].partition(" ")
                card = actions.edit(principal, ws, pid, text)
                return [self._card(msg, card, ws)]
            if cmd and cmd[0] == "withdraw":
                await actions.withdraw(self.settings, principal, ws, cmd[1].strip())
                return [self._reply(msg, "Withdrawn: answers stop citing it.", private=True)]
            return [await self._ask(adapter, msg, principal, ws)]
        except (actions.ActionRefused, channel_agent.ChannelModelError) as exc:
            return [self._reply(msg, str(exc), private=True)]

    def _remember_dm(self, account: models.ChannelAccount, adapter, msg: ChannelMessage) -> None:
        ref = adapter.dm_ref(msg) if hasattr(adapter, "dm_ref") else msg.conversation
        if account.dm_ref != ref:
            account.dm_ref = ref
            models.save_channel_account(account)

    def _workspace(self, account: models.ChannelAccount, msg: ChannelMessage):
        if not msg.direct:
            b = models.get_channel_binding(msg.channel, msg.conversation)
            return models.get_workspace(b.workspace_id) if b else None
        return (
            models.get_workspace(account.default_workspace) if account.default_workspace else None
        )

    def _visible(self, principal) -> list[models.Workspace]:
        return [w for w in models.list_workspaces() if authz.sees_workspace(principal, w)]

    def _no_project(self, principal, msg: ChannelMessage) -> str:
        if not msg.direct:
            return (
                "This channel is not bound to a project you can read. An owner binds it on the "
                "console's Channels page."
            )
        names = ", ".join(w.id for w in self._visible(principal)) or "none"
        return f"Choose a project first: /ekos project <name>. Yours: {names}."

    def _choose_project(self, account, principal, msg: ChannelMessage, name: str) -> str:
        if not msg.direct:
            return "In a channel the project is the one an owner bound it to."
        ws = models.get_workspace(name.strip())
        if ws is None or not authz.sees_workspace(principal, ws):
            names = ", ".join(w.id for w in self._visible(principal)) or "none"
            return f"No project {name!r} for you. Yours: {names}."
        account.default_workspace = ws.id
        models.save_channel_account(account)
        return f"Direct messages now use {ws.name} ({ws.id})."

    async def _intake(self, adapter, msg: ChannelMessage, principal, ws) -> OutMessage:
        lines = []
        for att in msg.attachments:
            if att.size and att.size > self.settings.inbox_upload_max_mb * 1024 * 1024:
                lines.append(f"{att.name}: larger than {self.settings.inbox_upload_max_mb} MB")
                continue
            data = await adapter.download(att)
            got = await actions.intake_bytes(
                self.settings, principal, ws, att.name, data,
                channel=msg.channel, permalink=msg.permalink,
            )  # fmt: skip
            lines.append(self._stored_line(got))
        if not msg.attachments:
            got = await actions.intake_link(
                self.settings, principal, ws, msg.text.strip(),
                channel=msg.channel, permalink=msg.permalink,
            )  # fmt: skip
            lines.append(self._stored_line(got))
        return self._reply(msg, "\n".join(lines))

    @staticmethod
    def _stored_line(got: dict) -> str:
        what = f"{got.get('name', 'it')} (id {str(got.get('id', ''))[:8]})"
        if got.get("status") == "pending":
            return f"Stored {what}; it waits for a reviewer to accept it before it is compiled."
        return f"Stored {what}; it is compiled on the project's next build."

    async def _tell(self, msg: ChannelMessage, cmd, principal, ws) -> OutMessage:
        if not authz.authorize(principal, "review", ws.tenant_id, ws.id):
            raise actions.ActionRefused(
                "Recording what you know needs the reviewer role on this project; you can still "
                "send files and ask questions."
            )
        statement, answers = cmd[1], ""
        if cmd[0] == "answer":
            answers, _, statement = cmd[1].partition(" ")
            if not people_write._OBJECT_ID.match(answers):
                raise actions.ActionRefused("/ekos answer <question id> <your answer>")
        statement = statement.strip()
        if not statement:
            raise actions.ActionRefused("Say what you know after the command.")
        kept, dropped = await channel_agent.propose(
            self.settings, self.supervisor, principal, ws, statement=statement
        )
        from ..chat import tell

        card = tell.new_proposal(
            ws.id,
            f"{principal.kind}:{principal.id}",
            statement,
            kept,
            channel=msg.channel,
            channel_account=f"{msg.channel}:{msg.account}",
            message_ref=msg.message_ref,
            permalink=msg.permalink,
            answers=answers,
        )
        out = self._card(msg, card, ws)
        if dropped:
            out.text += f"\n({dropped} proposal(s) dropped: they did not quote your words.)"
        return out

    def _card(self, msg: ChannelMessage, card: models.Proposal, ws) -> OutMessage:
        from ..chat import tell

        lines = tell.card_lines(card.items) or ["(nothing to link it to; your words alone)"]
        text = (
            "I understood:\n"
            + "\n".join(f"• {line}" for line in lines)
            + f'\nSave records your words: "{card.text}"  (card {_short(card.id)})'
        )
        ch, acc = msg.channel, msg.account
        buttons = [
            actions.button("Save", ch, acc, ws.id, action="save", proposal=card.id),
            actions.button("Edit", ch, acc, ws.id, action="edit", proposal=card.id),
            actions.button("Discard", ch, acc, ws.id, action="discard", proposal=card.id),
        ]
        # The card shows what the model read: the author's alone.
        return self._reply(msg, text, private=True, buttons=buttons)

    async def _ask(self, adapter, msg: ChannelMessage, principal, ws) -> OutMessage:
        answer = await channel_agent.ask(
            self.settings, self.supervisor, principal, ws,
            channel=msg.channel, conversation=msg.conversation, account=msg.account,
            question=msg.text,
        )  # fmt: skip
        scopes = None if msg.direct else await self._member_scopes(adapter, msg, ws)
        private = answer_privately(
            direct=msg.direct, used_knowledge=answer.used_knowledge, member_scopes=scopes
        )
        return self._reply(msg, answer.text, private=private)

    async def _member_scopes(self, adapter, msg: ChannelMessage, ws) -> list[object] | None:
        """Each member's read scope on the project (RFC 0181), or `None` when unknown."""
        try:
            members = await adapter.members(msg)
        except Exception:
            return None
        if members is None:
            return None
        out: list[object] = []
        for m in members:
            acc = models.get_channel_account(msg.channel, m)
            p = actions.principal_of(acc, self.settings) if acc else None
            if p is None or not authz.sees_workspace(p, ws):
                out.append(UNLINKED)
            else:
                out.append(authz.scope_for(p, ws))
        return out

    # ── buttons ──────────────────────────────────────────────────────────────

    async def on_click(self, adapter, click: ButtonClick) -> str:
        try:
            text = await self.handle_click(click)
        except actions.ActionRefused as exc:
            text = str(exc)
        except Exception as exc:
            log.warning("%s button failed: %s", click.channel, type(exc).__name__)
            text = "Something went wrong; try again later."
        with contextlib.suppress(Exception):
            await adapter.answer_click(click, text)
        return text

    async def handle_click(self, click: ButtonClick) -> str:
        row = actions.pressed(click.channel, click.account, click.ref)
        account = models.get_channel_account(click.channel, click.account)
        principal = actions.principal_of(account, self.settings) if account else None
        ws = models.get_workspace(row.workspace_id)
        if principal is None or ws is None or not authz.sees_workspace(principal, ws):
            raise actions.ActionRefused(link_instructions())
        act = row.payload.get("action")
        pid = str(row.payload.get("proposal", ""))
        qid = str(row.payload.get("question", ""))
        if act == "save":
            await actions.save(self.settings, principal, ws, pid, channel=click.channel)
            return f"Saved as what {actions.label_of(principal)} said."
        if act == "discard":
            actions.discard(principal, ws, pid)
            return "Discarded."
        if act == "edit":
            return f"Reply with: /ekos edit {_short(pid)} <your corrected text>"
        if act == "not_me":
            await actions.not_me(self.settings, principal, ws, qid)
            return "Thanks: the question goes to the next person who may know."
        if act == "answer":
            return f"Reply with: /ekos answer {qid} <your answer>"
        raise actions.ActionRefused("that button does nothing any more")

    # ── background ───────────────────────────────────────────────────────────

    async def _dispatch(self, adapter, event) -> None:
        if isinstance(event, ButtonClick):
            await self.on_click(adapter, event)
        else:
            await self.on_message(adapter, event)

    def spawn(self, coro) -> None:
        """Handle an inbound request after answering it (Slack wants an answer in 3 seconds)."""
        task = asyncio.create_task(coro)
        self._pending.add(task)
        task.add_done_callback(self._pending.discard)

    async def drain(self) -> None:
        """Wait for everything `spawn` started (tests; shutdown)."""
        while self._pending:
            await asyncio.gather(*list(self._pending), return_exceptions=True)

    def start(self) -> None:
        for name in ("telegram", "slack"):
            adapter = self.adapters.get(name)
            if adapter is not None and hasattr(adapter, "run") and getattr(adapter, "polls", True):
                self._tasks.append(asyncio.create_task(adapter.run(self._dispatch, self._stop)))

    async def aclose(self) -> None:
        self._stop.set()
        await self.drain()
        for t in self._tasks:
            t.cancel()
            with contextlib.suppress(asyncio.CancelledError, Exception):
                await t
        for a in self.adapters.values():
            with contextlib.suppress(Exception):
                await a.aclose()

    # ── digest (RFC 0254 §3.3) ───────────────────────────────────────────────

    async def digest(self) -> dict[str, Any]:
        """Each person's open questions (`ekos questions digest`), through their linked channel.
        Nothing is sent from a public deployment."""
        if self.settings.public:
            return {"sent": 0, "skipped": "a public deployment sends no questions"}
        sent, unreachable = 0, []
        for ws in models.list_workspaces():
            try:
                data = await readproc.read_json(
                    self.settings.ekos_bin, ws.path, ["questions", "digest", "--json"],
                    time_limit=300.0,
                )  # fmt: skip
            except readproc.ReadProcError as exc:
                log.warning("digest for %s: %s", ws.id, exc)
                continue
            for person, questions in (data.get("per_person") or {}).items():
                n = await self._deliver(ws, person, questions)
                if n:
                    sent += n
                else:
                    unreachable.append(person)
        return {"sent": sent, "unreachable": sorted(set(unreachable))}

    async def _deliver(self, ws: models.Workspace, person: str, questions: list[dict]) -> int:
        for acc in models.channel_accounts_labelled(person):
            adapter = self.adapters.get(acc.channel)
            if adapter is None or not acc.dm_ref:
                continue
            principal = actions.principal_of(acc, self.settings)
            if principal is None or not authz.sees_workspace(principal, ws):
                continue
            extra, conversation = _dm_target(acc)
            await adapter.send(
                OutMessage(
                    conversation=conversation,
                    text=f"{len(questions)} question(s) about {ws.name} you may be able to answer:",
                    extra=extra,
                )
            )
            for q in questions:
                qid = str(q.get("id", ""))
                buttons = [
                    actions.button("Answer", acc.channel, acc.account, ws.id, action="answer",
                                   question=qid),
                    actions.button("Not me", acc.channel, acc.account, ws.id, action="not_me",
                                   question=qid),
                ]  # fmt: skip
                await adapter.send(
                    OutMessage(
                        conversation=conversation,
                        text=f"{q.get('text', '')}\n(question {qid[:8]})",
                        buttons=buttons,
                        extra=extra,
                    )
                )
            return len(questions)
        return 0


def _dm_target(acc: models.ChannelAccount) -> tuple[dict[str, str], str]:
    if acc.channel == "teams":
        import json

        ref = json.loads(acc.dm_ref)
        return {k: str(v) for k, v in ref.items()}, str(ref.get("conversation", ""))
    return {}, acc.dm_ref


def digest_trigger(cron: str):
    """The digest's schedule (cron in UTC), or `None` when unset or not a valid cron line."""
    if not cron.strip():
        return None
    from apscheduler.triggers.cron import CronTrigger

    try:
        return CronTrigger.from_crontab(cron, timezone="UTC")
    except ValueError as exc:
        log.warning("channels_digest_cron %r is not a cron line: %s", cron, exc)
        return None


def build(settings: Settings, supervisor, *, client_factory=None) -> ChannelService | None:
    """The service with every configured channel, or `None` when none is."""
    from . import slack, teams, telegram

    have = configured(settings)
    if not any(have.values()):
        return None
    make = client_factory or (lambda: None)
    adapters: dict[str, Any] = {}
    if have["telegram"]:
        adapters["telegram"] = telegram.TelegramAdapter(
            secret(settings.channels_telegram_token_env), client=make()
        )
    if have["slack"]:
        a = slack.SlackAdapter(
            secret(settings.channels_slack_bot_token_env),
            signing_secret=secret(settings.channels_slack_signing_secret_env),
            app_token=secret(settings.channels_slack_app_token_env),
            client=make(),
        )
        # Without an app-level token Slack posts events to the console instead.
        a.polls = bool(secret(settings.channels_slack_app_token_env))  # type: ignore[attr-defined]
        adapters["slack"] = a
    if have["teams"]:
        adapters["teams"] = teams.TeamsAdapter(
            settings.channels_teams_app_id,
            secret(settings.channels_teams_app_secret_env),
            client=make(),
        )
    return ChannelService(settings, supervisor, adapters)
