"""Microsoft Teams (RFC 0253 §3.8): Bot Framework activities, Adaptive Card buttons.

Teams needs an inbound HTTPS endpoint (`POST /api/channels/teams/messages`). Every request
carries a JWT the Bot Framework signed; `verify_token` checks it against the keys the Bot
Framework publishes (RS256, issuer `https://api.botframework.com`, audience = this bot's app id,
not expired, and its `serviceUrl` claim equal to the activity's), so nobody else can post as a
user. A button is an `Action.Execute`; its press arrives as an `adaptiveCard/action` invoke whose
`from` is the presser, inside the same signed request.

Replies use a token from the Bot Framework's client-credentials grant. A private answer in a
channel goes to a personal conversation the bot opens with the person.
"""

from __future__ import annotations

import json
import re
import time
from typing import Any

import httpx

from .core import Attachment, ButtonClick, ChannelMessage, OutMessage, chunks

OPENID = "https://login.botframework.com/v1/.well-known/openidconfiguration"
ISSUER = "https://api.botframework.com"
TOKEN_URL = "https://login.microsoftonline.com/botframework.com/oauth2/v2.0/token"
SCOPE = "https://api.botframework.com/.default"
MAX_TEXT = 20_000
_MENTION = re.compile(r"<at>[^<]*</at>")
_FILE_INFO = "application/vnd.microsoft.teams.file.download.info"


class TeamsError(RuntimeError):
    pass


class TeamsAuthError(TeamsError):
    """The request is not the Bot Framework's (→ 401)."""


def _attachment(a: object) -> Attachment | None:
    """One Teams attachment as a file to fetch, or `None` (inline HTML, no file)."""
    if not isinstance(a, dict):
        return None
    name = str(a.get("name") or "file")
    if a.get("contentType") == _FILE_INFO:
        url = str((a.get("content") or {}).get("downloadUrl") or "")
        return Attachment(name=name, fetch=url) if url else None
    if a.get("contentUrl") and not str(a.get("contentType", "")).startswith("text/html"):
        return Attachment(
            name=name, media_type=str(a.get("contentType") or ""), fetch=str(a["contentUrl"])
        )
    return None


def _attachments(activity: dict) -> tuple[Attachment, ...]:
    found = (_attachment(a) for a in activity.get("attachments") or [])
    return tuple(a for a in found if a is not None)


def _account(activity: dict) -> str:
    sender = activity.get("from") or {}
    # The AAD object id names a person across chats; the Bot Framework id is per bot.
    return str(sender.get("aadObjectId") or sender.get("id") or "")


def _routing(activity: dict) -> dict[str, str]:
    conv = activity.get("conversation") or {}
    tenant = (activity.get("channelData") or {}).get("tenant") or {}
    return {
        "service_url": str(activity.get("serviceUrl") or ""),
        "conversation_type": str(conv.get("conversationType") or ""),
        "tenant": str(tenant.get("id") or conv.get("tenantId") or ""),
        "bot_id": str((activity.get("recipient") or {}).get("id") or ""),
        "user_id": str((activity.get("from") or {}).get("id") or ""),
    }


def _parse_click(activity: dict) -> ButtonClick | None:
    """An `Action.Execute` on one of the bot's cards."""
    action = (activity.get("value") or {}).get("action") or {}
    ref = str((action.get("data") or {}).get("ekos") or "")
    if action.get("type") != "Action.Execute" or not ref:
        return None
    return ButtonClick(
        channel="teams",
        account=_account(activity),
        conversation=str((activity.get("conversation") or {}).get("id") or ""),
        ref=ref,
        message_ref=str(activity.get("replyToId") or ""),
        extra=_routing(activity),
    )


def parse_activity(activity: dict) -> ChannelMessage | ButtonClick | None:
    kind = activity.get("type")
    conv = activity.get("conversation") or {}
    if kind == "invoke" and activity.get("name") == "adaptiveCard/action":
        return _parse_click(activity)
    if kind != "message":
        return None
    routing = _routing(activity)
    direct = routing["conversation_type"] in ("personal", "")
    text = _MENTION.sub("", str(activity.get("text") or "")).strip()
    return ChannelMessage(
        channel="teams",
        account=_account(activity),
        conversation=str(conv.get("id") or ""),
        message_ref=str(activity.get("id") or ""),
        text=text,
        direct=direct,
        attachments=_attachments(activity),
        reply_to=str(activity.get("replyToId") or ""),
        thread=str(activity.get("id") or "") if not direct else "",
        sent_at=str(activity.get("timestamp") or ""),
        extra=routing,
    )


def card(out: OutMessage, text: str) -> dict:
    """An Adaptive Card: the text, and one `Action.Execute` per button."""
    return {
        "contentType": "application/vnd.microsoft.card.adaptive",
        "content": {
            "type": "AdaptiveCard",
            "version": "1.4",
            "body": [{"type": "TextBlock", "text": text, "wrap": True}],
            "actions": [
                {
                    "type": "Action.Execute",
                    "title": b.label,
                    "verb": "ekos",
                    "data": {"ekos": b.ref},
                }
                for b in out.buttons
            ],
        },
    }


def render(out: OutMessage) -> list[dict[str, Any]]:
    pieces = chunks(out.text, MAX_TEXT)
    bodies: list[dict[str, Any]] = []
    for n, piece in enumerate(pieces):
        body: dict[str, Any] = {"type": "message", "text": piece}
        if out.buttons and n == len(pieces) - 1:
            body = {"type": "message", "attachments": [card(out, piece)]}
        if out.thread:
            body["replyToId"] = out.thread
        bodies.append(body)
    return bodies


def invoke_response(text: str) -> dict:
    """What a button press is answered with, in the HTTP response itself."""
    return {"statusCode": 200, "type": "application/vnd.microsoft.activity.message", "value": text}


class TeamsAdapter:
    name = "teams"

    def __init__(self, app_id: str, app_secret: str, client: httpx.AsyncClient | None = None):
        self.app_id = app_id
        self._secret = app_secret
        self._client = client or httpx.AsyncClient(timeout=30.0)
        self._keys: Any = None
        self._keys_at = 0.0
        self._token = ""
        self._token_until = 0.0

    # ── inbound ──────────────────────────────────────────────────────────────

    async def _key_set(self):
        from joserfc.jwk import KeySet  # authlib's own JOSE library

        if self._keys is None or time.time() - self._keys_at > 24 * 3600:
            meta = (await self._client.get(OPENID)).json()
            jwks = (await self._client.get(str(meta["jwks_uri"]))).json()
            self._keys = KeySet.import_key_set(jwks)
            self._keys_at = time.time()
        return self._keys

    async def verify_token(self, authorization: str, activity: dict) -> dict:
        """The Bot Framework's claims, or `TeamsAuthError`."""
        from joserfc import jwt
        from joserfc.errors import JoseError

        if not self.app_id or not authorization.startswith("Bearer "):
            raise TeamsAuthError("no Bot Framework token")
        try:
            token = jwt.decode(authorization[7:], await self._key_set(), algorithms=["RS256"])
        except (JoseError, ValueError, KeyError) as exc:
            raise TeamsAuthError(f"bad token: {type(exc).__name__}") from None
        claims = token.claims
        now = time.time()
        if claims.get("iss") != ISSUER:
            raise TeamsAuthError("wrong issuer")
        aud = claims.get("aud")
        if aud != self.app_id and not (isinstance(aud, list) and self.app_id in aud):
            raise TeamsAuthError("wrong audience")
        if float(claims.get("exp", 0)) < now - 300 or float(claims.get("nbf", 0)) > now + 300:
            raise TeamsAuthError("expired")
        service = str(activity.get("serviceUrl") or "")
        if claims.get("serviceurl", claims.get("serviceUrl")) not in (None, service):
            raise TeamsAuthError("serviceUrl does not match")
        return claims

    # ── outbound ─────────────────────────────────────────────────────────────

    async def _bearer(self) -> str:
        if self._token and time.time() < self._token_until - 60:
            return self._token
        try:
            r = await self._client.post(
                TOKEN_URL,
                data={
                    "grant_type": "client_credentials",
                    "client_id": self.app_id,
                    "client_secret": self._secret,
                    "scope": SCOPE,
                },
            )
        except httpx.HTTPError as exc:
            raise TeamsError(f"token: {type(exc).__name__}") from None
        if r.status_code != 200:
            raise TeamsError(f"token: {r.status_code}")
        data = r.json()
        self._token = str(data["access_token"])
        self._token_until = time.time() + float(data.get("expires_in", 3600))
        return self._token

    async def _post(self, url: str, body: dict) -> dict:
        try:
            r = await self._client.post(
                url, json=body, headers={"Authorization": f"Bearer {await self._bearer()}"}
            )
        except httpx.HTTPError as exc:
            raise TeamsError(f"post: {type(exc).__name__}") from None
        if r.status_code >= 300:
            raise TeamsError(f"post: {r.status_code}")
        return r.json() if r.content else {}

    async def _personal(self, extra: dict[str, str], user: str) -> str:
        """The personal conversation with `user` (created if new)."""
        data = await self._post(
            f"{extra['service_url'].rstrip('/')}/v3/conversations",
            {
                "bot": {"id": extra.get("bot_id", "")},
                "members": [{"id": user}],
                "tenantId": extra.get("tenant", ""),
                "channelData": {"tenant": {"id": extra.get("tenant", "")}},
                "isGroup": False,
            },
        )
        return str(data.get("id") or "")

    async def send(self, out: OutMessage) -> str:
        service = out.extra.get("service_url", "")
        if not service:
            raise TeamsError("no service URL to reply through")
        conversation = out.conversation
        thread = out.thread
        if out.private_to:
            conversation = await self._personal(
                out.extra, out.extra.get("user_id") or out.private_to
            )
            thread = ""
        ref = ""
        for body in render(OutMessage(conversation, out.text, out.buttons, "", thread, out.extra)):
            path = f"/v3/conversations/{conversation}/activities"
            if thread:
                path += f"/{thread}"
            data = await self._post(service.rstrip("/") + path, body)
            ref = str(data.get("id") or "")
        return ref

    async def answer_click(self, click: ButtonClick, text: str) -> None:
        """Teams takes the answer in the invoke's own HTTP response (`invoke_response`)."""

    async def download(self, att: Attachment) -> bytes:
        headers = {}
        # A personal-chat upload's `downloadUrl` is pre-authorised; anything else needs the
        # bot's token.
        if "/v3/attachments/" in att.fetch:
            headers["Authorization"] = f"Bearer {await self._bearer()}"
        try:
            r = await self._client.get(att.fetch, headers=headers)
        except httpx.HTTPError as exc:
            raise TeamsError(f"download: {type(exc).__name__}") from None
        if r.status_code != 200:
            raise TeamsError(f"download: {r.status_code}")
        return r.content

    async def members(self, msg: ChannelMessage) -> list[str] | None:
        if msg.direct:
            return [msg.account]
        service = msg.extra.get("service_url", "")
        try:
            r = await self._client.get(
                f"{service.rstrip('/')}/v3/conversations/{msg.conversation}/members",
                headers={"Authorization": f"Bearer {await self._bearer()}"},
            )
        except httpx.HTTPError:
            return None
        if r.status_code != 200:
            return None
        return [
            str(m.get("aadObjectId") or m.get("id"))
            for m in r.json()
            if str(m.get("id")) != msg.extra.get("bot_id")
        ]

    @staticmethod
    def dm_ref(msg: ChannelMessage) -> str:
        """How to reach this person later (a digest): their personal conversation and service."""
        return json.dumps(
            {"conversation": msg.conversation, **{k: msg.extra.get(k, "") for k in
             ("service_url", "tenant", "bot_id", "user_id")}}
        )  # fmt: skip

    async def aclose(self) -> None:
        await self._client.aclose()
