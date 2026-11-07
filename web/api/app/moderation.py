"""Public-demo safety service (EKOS RFC 0249): the policy of `moderation_policy`, applied to a
database and a clock.

The order of checks is the point:

1. the address is rate-limited;
2. the person's standing (banned, blocked) ends a request before its text is read, then the
   address is checked against the blocked ones;
3. the daily question limit;
4. deterministic signals — which decide alone;
5. only a message they found nothing in goes to the model, within the daily model-call limit, and
   a model that cannot be reached **refuses without penalty** rather than allowing.

Every refusal, block, ban and unban is a row the owner can read (`ModEvent`).
"""

from __future__ import annotations

import hashlib
import json
import threading
from collections import deque
from collections.abc import Callable
from dataclasses import asdict, dataclass, field
from datetime import UTC, datetime, timedelta

from sqlmodel import select

from . import models
from .moderation_model import Classifier, ClassifierError, Verdict
from .moderation_policy import (
    Decision,
    Kind,
    Limits,
    Rules,
    Standing,
    Usage,
    check_limit,
    decide,
    pre_check,
    signals,
)

EXCERPT = 200


def _aware(dt: datetime | None) -> datetime | None:
    """SQLite hands back naive datetimes; everything here is UTC."""
    if dt is not None and dt.tzinfo is None:
        return dt.replace(tzinfo=UTC)
    return dt


def _day(now: datetime) -> str:
    return now.astimezone(UTC).strftime("%Y-%m-%d")


def _excerpt(text: str) -> str:
    """A short, escaped excerpt for the owner's log: invisible characters show as escapes."""
    return json.dumps(text[:EXCERPT], ensure_ascii=True)


@dataclass
class Outcome:
    """What `screen` tells the caller. `allowed` is the only field that grants anything."""

    allowed: bool
    action: str
    reason: str
    signal: str = ""
    until: datetime | None = None
    # False when no model was asked (not configured, a signal decided, or it was unreachable).
    model_checked: bool = False
    remaining: dict[str, int] = field(default_factory=dict)

    def as_json(self) -> dict:
        d = asdict(self)
        d["until"] = self.until.isoformat() if self.until else None
        return d


class Moderator:
    def __init__(
        self,
        *,
        limits: Limits | None = None,
        rules: Rules | None = None,
        classifier: Classifier | None = None,
        clock: Callable[[], datetime] | None = None,
    ) -> None:
        self.limits = limits or Limits()
        self.rules = rules or Rules()
        self.classifier = classifier
        self._clock = clock or (lambda: datetime.now(UTC))
        self._hits: dict[str, deque[float]] = {}
        self._verdicts: dict[str, Verdict] = {}
        self._lock = threading.Lock()
        # What the model cost this process, for the owner's cost report.
        self.tokens_in = 0
        self.tokens_out = 0

    # ── helpers ──────────────────────────────────────────────────────────────────────────────

    def _now(self) -> datetime:
        return self._clock()

    def _rate_limited(self, ip: str, now: datetime) -> bool:
        if not ip:
            return False
        window = self._hits.setdefault(ip, deque())
        t = now.timestamp()
        while window and window[0] <= t - 60:
            window.popleft()
        if len(window) >= self.limits.ip_per_minute:
            return True
        window.append(t)
        return False

    def _usage(self, s, subject: str, now: datetime) -> models.ModUsage:
        key = f"{subject}|{_day(now)}"
        row = s.get(models.ModUsage, key)
        if row is None:
            row = models.ModUsage(key=key, subject=subject, day=_day(now))
            s.add(row)
        return row

    def _standing(self, u: models.ModUser) -> Standing:
        return Standing(
            banned=u.banned,
            blocked_until=_aware(u.blocked_until),
            consecutive_unsafe=u.consecutive_unsafe,
            model_strikes=u.model_strikes,
        )

    def _user(self, s, subject: str, now: datetime) -> models.ModUser:
        u = s.get(models.ModUser, subject)
        if u is None:
            u = models.ModUser(subject=subject, first_seen=now, last_seen=now)
            s.add(u)
        u.last_seen = now
        return u

    def _apply(self, s, u: models.ModUser, d: Decision, now: datetime) -> None:
        st = d.standing
        was_banned = u.banned
        u.banned = st.banned
        u.blocked_until = st.blocked_until
        u.consecutive_unsafe = st.consecutive_unsafe
        u.model_strikes = st.model_strikes
        if st.banned and not was_banned:
            u.banned_at = now
            u.ban_reason = d.reason

    def _log(self, s, subject: str, ip: str, d: Decision, text: str, now: datetime) -> None:
        s.add(
            models.ModEvent(
                at=now,
                subject=subject,
                ip=ip,
                action=d.action,
                signal=d.signal,
                reason=d.reason,
                digest=hashlib.sha256(text.encode()).hexdigest(),
                excerpt=_excerpt(text),
            )
        )

    def _remaining(self, usage: models.ModUsage) -> dict[str, int]:
        lim = self.limits
        return {
            "questions": max(0, lim.questions_per_day - usage.questions),
            "model_calls": max(0, lim.model_calls_per_day - usage.model_calls),
            "additive": max(0, lim.additive_per_day - usage.additive),
            "modifying": max(0, lim.modifying_per_day - usage.modifying),
        }

    @staticmethod
    def _out(d: Decision, usage: models.ModUsage | None = None, **kw) -> Outcome:
        return Outcome(d.allowed, d.action, d.reason, d.signal, d.until, **kw)

    # ── the check ────────────────────────────────────────────────────────────────────────────

    async def screen(self, subject: str, ip: str, text: str) -> Outcome:
        """Decide one question. On `allowed` the day's question (and model call) is spent."""
        now = self._now()
        with self._lock:
            if self._rate_limited(ip, now):
                return Outcome(False, "limited", "too many requests from this address; slow down")
        with models.session() as s:
            u = self._user(s, subject, now)
            standing = self._standing(u)
            usage = self._usage(s, subject, now)

            # Standing first: someone banned is told so, not that their address is blocked.
            stop = pre_check(standing, now)
            if stop:
                s.commit()
                return self._out(stop)

            blocked = s.get(models.ModIpBlock, ip) if ip else None
            if blocked and _aware(blocked.until) > now:
                s.commit()
                return Outcome(
                    False, "block", "this address is blocked for now", until=_aware(blocked.until)
                )

            over = check_limit(
                "question",
                Usage(usage.questions, usage.model_calls, usage.additive, usage.modifying),
                self.limits,
                _aware(u.first_seen) or now,
                now,
            )
            if over:
                s.commit()
                return Outcome(False, "limited", over, remaining=self._remaining(usage))

            found = signals(text)
            verdict: Verdict | None = None
            if not found and self.classifier is not None:
                verdict, refusal = await self._ask_model(usage, text, u, now)
                if refusal:
                    s.commit()
                    return refusal
            decision = decide(
                standing, found, None if verdict is None else verdict.unsafe, now, self.rules
            )
            if decision.signal == "model" or decision.action in ("block", "ban"):
                pass
            self._apply(s, u, decision, now)
            if decision.allowed:
                usage.questions += 1
            else:
                self._log(s, subject, ip, decision, text, now)
                if decision.ip_block and ip:
                    s.merge(
                        models.ModIpBlock(
                            ip=ip, until=now + decision.ip_block, reason=decision.reason
                        )
                    )
            s.commit()
            return self._out(
                decision, model_checked=verdict is not None, remaining=self._remaining(usage)
            )

    async def _ask_model(
        self, usage: models.ModUsage, text: str, u: models.ModUser, now: datetime
    ) -> tuple[Verdict | None, Outcome | None]:
        """The model's verdict, or the refusal to give instead. A cached verdict costs nothing."""
        digest = hashlib.sha256(text.encode()).hexdigest()
        cached = self._verdicts.get(digest)
        if cached is not None:
            return cached, None
        over = check_limit(
            "model_call",
            Usage(usage.questions, usage.model_calls),
            self.limits,
            _aware(u.first_seen) or now,
            now,
        )
        if over:
            return None, Outcome(False, "limited", over, remaining=self._remaining(usage))
        assert self.classifier is not None
        try:
            verdict = await self.classifier(text)
        except ClassifierError:
            # Fail closed and free: the person did nothing wrong, the check just could not run.
            return None, Outcome(
                False, "refuse", "the safety check is unavailable; try again in a moment"
            )
        usage.model_calls += 1
        self.tokens_in += verdict.tokens_in
        self.tokens_out += verdict.tokens_out
        if len(self._verdicts) > 2000:
            self._verdicts.clear()
        self._verdicts[digest] = verdict
        return verdict, None

    # ── what a person may do (RFC 0249 §2.4) ─────────────────────────────────────────────────

    def eligibility(self, subject: str, kind: Kind, dashboards: int = 0) -> str | None:
        """Why `kind` is not allowed now, or `None`. Spends nothing."""
        now = self._now()
        with models.session() as s:
            u = self._user(s, subject, now)
            row = self._usage(s, subject, now)
            s.commit()
            usage = Usage(row.questions, row.model_calls, row.additive, row.modifying, dashboards)
            return check_limit(kind, usage, self.limits, _aware(u.first_seen) or now, now)

    def spend(self, subject: str, kind: Kind) -> str | None:
        """Spend one `additive` or `modifying` change; the refusal if it is not allowed now."""
        if kind not in ("additive", "modifying"):
            raise ValueError("only additive and modifying changes are spent here")
        now = self._now()
        with models.session() as s:
            u = self._user(s, subject, now)
            row = self._usage(s, subject, now)
            why = check_limit(
                kind,
                Usage(row.questions, row.model_calls, row.additive, row.modifying),
                self.limits,
                _aware(u.first_seen) or now,
                now,
            )
            if why is None:
                setattr(row, kind, getattr(row, kind) + 1)
            s.commit()
            return why

    def remaining(self, subject: str) -> dict[str, int]:
        now = self._now()
        with models.session() as s:
            self._user(s, subject, now)
            row = self._usage(s, subject, now)
            s.commit()
            return self._remaining(row)

    # ── the owner (RFC 0249 §2.3) ────────────────────────────────────────────────────────────

    def unban(self, subject: str, by: str) -> bool:
        """Lift a ban or a block and reset the counters. History stays in the log."""
        now = self._now()
        with models.session() as s:
            u = s.get(models.ModUser, subject)
            if u is None:
                return False
            u.banned = False
            u.blocked_until = None
            u.consecutive_unsafe = 0
            u.model_strikes = 0
            s.add(
                models.ModEvent(
                    at=now, subject=subject, action="unban", reason=f"lifted by {by}", by=by
                )
            )
            s.commit()
            return True

    def events(self, limit: int = 100, subject: str | None = None) -> list[dict]:
        with models.session() as s:
            q = select(models.ModEvent).order_by(models.ModEvent.at.desc()).limit(limit)
            if subject:
                q = q.where(models.ModEvent.subject == subject)
            return [{**e.model_dump(), "at": _aware(e.at).isoformat()} for e in s.exec(q).all()]

    def standing_of(self, subject: str) -> dict | None:
        with models.session() as s:
            u = s.get(models.ModUser, subject)
            if u is None:
                return None
            d = u.model_dump()
            for k in ("first_seen", "last_seen", "banned_at", "blocked_until"):
                d[k] = _aware(d[k]).isoformat() if d[k] else None
            return d

    def restricted(self) -> list[dict]:
        """Everyone banned or currently blocked."""
        now = self._now()
        with models.session() as s:
            rows = s.exec(select(models.ModUser)).all()
        out = []
        for u in rows:
            until = _aware(u.blocked_until)
            if u.banned or (until and until > now):
                out.append(
                    {
                        "subject": u.subject,
                        "banned": u.banned,
                        "blocked_until": until.isoformat() if until else None,
                        "reason": u.ban_reason,
                        "since": _aware(u.banned_at).isoformat() if u.banned_at else None,
                    }
                )
        return sorted(out, key=lambda r: r["subject"])


_RULE_KEYS = {
    "consecutive_limit": int,
    "model_strike_limit": int,
    "block_hours": float,
    "ip_block_hours": float,
}


def parse_numbers(text: str) -> tuple[Limits, Rules]:
    """The published numbers from the `moderation_limits` setting (JSON; empty = the defaults).
    An unknown name is an error: a typo must not leave a cap at its default unnoticed."""
    if not text.strip():
        return Limits(), Rules()
    try:
        raw = json.loads(text)
    except ValueError as exc:
        raise ValueError(f"moderation_limits is not valid JSON: {exc}") from exc
    if not isinstance(raw, dict):
        raise ValueError("moderation_limits is a JSON object")
    limit_fields = {f for f in Limits.__dataclass_fields__}
    unknown = set(raw) - limit_fields - set(_RULE_KEYS)
    if unknown:
        raise ValueError(f"moderation_limits: unknown name(s) {sorted(unknown)}")
    for k, v in raw.items():
        if isinstance(v, bool) or not isinstance(v, (int, float)) or v < 0:
            raise ValueError(f"moderation_limits: {k} is a non-negative number")
    limits = Limits(**{k: int(v) for k, v in raw.items() if k in limit_fields})
    rules = Rules(
        consecutive_limit=int(raw.get("consecutive_limit", Rules().consecutive_limit)),
        model_strike_limit=int(raw.get("model_strike_limit", Rules().model_strike_limit)),
        block=timedelta(hours=raw.get("block_hours", 24)),
        ip_block=timedelta(hours=raw.get("ip_block_hours", 24)),
    )
    return limits, rules


def client_ip(forwarded_for: str, peer: str, trusted_hops: int) -> str:
    """The caller's address. With no trusted proxy it is the socket peer, which a client cannot
    choose. With `n` trusted proxies it is the entry the n-th from the right appended; anything
    to its left is the client's own claim and is ignored."""
    if trusted_hops <= 0:
        return peer
    chain = [p.strip() for p in forwarded_for.split(",") if p.strip()]
    if len(chain) < trusted_hops:
        return peer
    return chain[-trusted_hops]


def build(settings, classifier: Classifier | None = None) -> Moderator | None:
    """The service for this console, or `None` when the public-demo checks are off."""
    if not settings.moderation_enabled:
        return None
    from . import moderation_model
    from .chat import providers

    limits, rules = parse_numbers(settings.moderation_limits)
    if classifier is None and settings.moderation_model:
        key = providers.key_for(settings.moderation_key_env)
        if not key and not settings.moderation_base_url:
            raise ValueError(
                f"moderation_model is set but ${settings.moderation_key_env} holds no key"
            )
        classifier = moderation_model.make_classifier(
            moderation_model.provider_for(
                settings.moderation_model, settings.moderation_base_url, key
            )
        )
    return Moderator(limits=limits, rules=rules, classifier=classifier)
