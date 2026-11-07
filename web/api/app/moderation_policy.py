"""Public-demo safety policy (EKOS RFC 0249) — pure: no database, no clock of its own, no model.

Three decisions live here, each a plain function of its inputs so they can be tested exhaustively:

* **What a message is**: deterministic signals found in the text (:func:`signals`). Hard ones —
  injection, probing for secrets, hidden text, forbidden schemas — are unambiguous attacks; soft
  ones — destructive or abusive requests — are refused but are not by themselves proof of intent.
* **What happens to a person**: :func:`decide` turns the person's standing, the signals and the
  model's judgement (if one was asked for) into one outcome: allow, refuse, block for a day, or ban.
* **What a person may do today**: :func:`check_limit`.

The model never decides alone and never decides *to allow* something a deterministic signal
refused. Its only power is to refuse what the patterns did not catch, and that costs the person a
24-hour block, not a ban — a repeat does.
"""

# ruff: noqa: RUF001, RUF003  (look-alike characters are the subject of this file)
from __future__ import annotations

import re
import unicodedata
from dataclasses import dataclass, field
from datetime import datetime, timedelta
from typing import Literal

Action = Literal["allow", "refuse", "block", "ban", "limited"]

# ── what the text says ───────────────────────────────────────────────────────────────────────

HARD = frozenset({"injection", "secrets", "hidden_text", "forbidden_schema"})
SOFT = frozenset({"destructive", "abuse"})

# Characters that carry no visible text but can carry instructions: bidirectional controls, the
# Unicode "tag" block (invisible ASCII), and zero-width separators that split a word for a filter.
# ZWJ/ZWNJ (200C/200D) are legitimate in several scripts and are not counted on their own.
_HIDDEN = re.compile("[\u202a-\u202e\u2066-\u2069\U000e0000-\U000e007f\u200b\u2060\ufeff]")

# Look-alike letters that let "ignore" be written with a Cyrillic "о". Matching only.
_CONFUSABLES = str.maketrans(
    {
        "а": "a", "е": "e", "о": "o", "р": "p", "с": "c", "х": "x", "у": "y", "і": "i",
        "ѕ": "s", "ј": "j", "ԁ": "d", "ɡ": "g", "ᴏ": "o", "ο": "o", "ν": "v", "ρ": "p",
    }
)  # fmt: skip
# Digits and symbols standing in for letters ("1gn0re"). A second view: applied to everything it
# would turn `1=1` into `i=i`, so the plain view is matched too.
_LEET = str.maketrans(
    {"0": "o", "1": "i", "3": "e", "4": "a", "5": "s", "7": "t", "@": "a", "$": "s"}
)


def normalize(text: str, *, leet: bool = False) -> str:
    """The text as a pattern should see it: compatibility-folded, invisible and combining marks
    removed, look-alikes mapped, lower-cased, whitespace collapsed. `leet` also maps digits and
    symbols used as letters. For matching only."""
    folded = unicodedata.normalize("NFKD", text)
    kept = "".join(
        c for c in folded if unicodedata.category(c) not in ("Mn", "Cf", "Cc") or c in "\n\t"
    )
    mapped = kept.translate(_CONFUSABLES).lower()
    if leet:
        mapped = mapped.translate(_LEET)
    return re.sub(r"\s+", " ", mapped).strip()


def _rx(*patterns: str) -> re.Pattern[str]:
    return re.compile("|".join(f"(?:{p})" for p in patterns), re.IGNORECASE | re.DOTALL)


_INJECTION = _rx(
    r"\b(ignore|disregard|forget|override|bypass|circumvent|skip)\b.{0,40}"
    r"\b(previous|prior|above|earlier|all|any|your|the|these|those|safety)\b.{0,30}"
    r"\b(instructions?|rules?|prompts?|guidelines?|constraints?|policy|policies|guardrails?|"
    r"filters?|restrictions?|checks?|gates?)\b",
    r"\b(reveal|show|print|display|repeat|output|leak|tell me|give me)\b.{0,30}"
    r"\b(system|hidden|initial|original|developer|secret)\s+(prompt|message|instructions?)\b",
    r"\bsystem\s+prompt\b",
    r"\b(developer|god|dan|jailbreak|debug|admin|sudo|unrestricted)\s+mode\b",
    r"\bjailbreak\b",
    r"\bdo\s+anything\s+now\b",
    r"\byou\s+are\s+now\b",
    r"\bpretend\s+(you|to)\s+(are|be|have\s+no)\b",
    r"\bact\s+as\b.{0,30}\b(unrestricted|unfiltered|without\s+(any\s+)?(rules|restrictions))\b",
    # Breaking out of the data blocks and chat templates this system uses.
    r"</?\s*(system|assistant|user_notes|instructions?|message)\s*>",
    r"<\|\s*(im_start|im_end|system|endoftext)\s*\|>",
    r"\[/?inst\]",
    r"^#{1,4}\s*(system|instructions?)\b",
    # Classic SQL injection shapes (people do not write SQL here, so these are probes).
    r"\bunion\s+(all\s+)?select\b",
    r"\bor\s+1\s*=\s*1\b",
    r"['\"]\s*;\s*(drop|delete|insert|update|select|truncate)\b",
)

_PROBE_VERB = (
    r"(reveal|leak|dump|print|show(\s+me)?|give(\s+me)?|tell(\s+me)?|what\s+is|what's|"
    r"read|list|expose|extract|exfiltrate|send|echo|cat)"
)
_SECRET_NOUN = (
    r"(api[ _-]?keys?|secret\s+keys?|access\s+tokens?|bearer\s+tokens?|passwords?|"
    r"credentials?|private\s+keys?|connection\s+strings?|env(ironment)?\s+vars?|"
    r"env(ironment)?\s+variables?|key\s*vault|secrets?)"
)
_SECRETS = _rx(
    # A verb, a determiner, then a secret — not "show password reset counts".
    _PROBE_VERB
    + r"\b.{0,25}\b(your|the|our|its|any|all)\s+"
    + _SECRET_NOUN
    + r"\b(?!\s+(reset|change|policy|expir|attempt|fail|length|strength|hash|complexity|"
    r"rotation|manager|audit))",
    r"\b(opencode|anthropic|openai)[ _-]?(api)?[ _-]?key\b",
    r"\bopencode_api_key\b",
    r"\bekos_console_\w+",
    r"\b(az|azure)[ _-]?(client|tenant)[ _-]?(secret|id)\b",
    r"\bmanaged\s+identity\s+token\b",
)

_FORBIDDEN_SCHEMA = _rx(
    r"\binformation[ _]schema\b",
    r"\bpg[ _]catalog\b",
    r"\bpg_(shadow|authid|user|roles|stat_activity|settings|database)\b",
    r"\bsys\.(tables|objects|columns|users|databases|sql_logins)\b",
    r"\bmysql\.user\b",
    r"\bperformance_schema\b",
    r"\bsystem\.(runtime|metadata|jdbc|information_schema)\b",
    r"\biceberg\.system\b",
    r"\$(snapshots|files|history|manifests|partitions|refs)\b",
    r"\bbronze_\w+",
    r"\bsilver_\w+\.\w*(pii|secret|password|token)\w*",
)

_DESTRUCTIVE = _rx(
    r"\b(drop|truncate|wipe|destroy|erase|nuke|obliterate)\b.{0,25}"
    r"\b(table|tables|mart|marts|schema|database|dashboards?|everything|warehouse|lakehouse)\b",
    r"\bdelete\s+(all|every|everything)\b",
    r"\bdelete\s+from\b",
    r"\b(rebuild|recreate)\b.{0,30}\b(empty|blank|from\s+nothing)\b",
    r"\bmass[ -]?(delete|update|overwrite)\b",
)

_ABUSE = _rx(
    r"\bcross\s+join\b",
    r"\b(every|all)\s+rows?\s+of\s+(every|all)\b",
    r"\b(every|all)\s+(tables?|columns?)\b.{0,40}\b(every|all)\s+(rows?|records?)\b",
    r"\bjoin(ed)?\b.{0,40}\b(to|with)\s+itself\b.{0,20}\b(\d{2,}|ten|twenty|fifty|hundred|"
    r"thousand|million)\s+times\b",
    r"\brepeat\b.{0,30}\b(\d{4,}|thousand|million|billion)\s+times\b",
    r"\b(\d{5,}|million|billion)\s+(queries|requests|dashboards)\b",
)


@dataclass(frozen=True)
class Signal:
    name: str
    severity: Literal["hard", "soft"]
    # The words that matched, shortened; for the owner's log, never shown as a rule.
    evidence: str


def signals(text: str) -> list[Signal]:
    """Every deterministic signal in `text`, hard ones first. Empty for an ordinary question."""
    found: list[Signal] = []
    if _HIDDEN.search(text):
        found.append(Signal("hidden_text", "hard", "invisible or bidirectional characters"))
    views = (normalize(text), normalize(text, leet=True))
    for name, rx in (
        ("injection", _INJECTION),
        ("secrets", _SECRETS),
        ("forbidden_schema", _FORBIDDEN_SCHEMA),
        ("destructive", _DESTRUCTIVE),
        ("abuse", _ABUSE),
    ):
        m = next((m for v in views if (m := rx.search(v))), None)
        if m:
            sev = "hard" if name in HARD else "soft"
            found.append(Signal(name, sev, m.group(0)[:80]))
    found.sort(key=lambda s: (s.severity != "hard", s.name))
    return found


# ── what happens to a person ────────────────────────────────────────────────────────────────


@dataclass(frozen=True)
class Standing:
    """A person's standing as the policy sees it. Stored by the service; passed in here."""

    banned: bool = False
    blocked_until: datetime | None = None
    # Unsafe refusals in a row (a safe request resets it).
    consecutive_unsafe: int = 0
    # Requests only the model judged unsafe; a second one is a ban.
    model_strikes: int = 0


@dataclass(frozen=True)
class Rules:
    """The numbers the rules page publishes. Set before launch (RFC 0249 §2.4)."""

    # Unsafe refusals in a row that end in a ban.
    consecutive_limit: int = 3
    # Model-only judgements that end in a ban (the second one).
    model_strike_limit: int = 2
    block: timedelta = timedelta(hours=24)
    # How long an automatic ban also blocks the address.
    ip_block: timedelta = timedelta(hours=24)


@dataclass(frozen=True)
class Decision:
    action: Action
    # One sentence the person is shown: they always see the reason.
    reason: str
    signal: str = ""
    until: datetime | None = None
    # Also block the address for this long (automatic bans).
    ip_block: timedelta | None = None
    # The standing after this decision.
    standing: Standing = field(default_factory=Standing)
    # A model verdict is only asked for when nothing deterministic decided.
    ask_model: bool = False

    @property
    def allowed(self) -> bool:
        return self.action == "allow"


def pre_check(standing: Standing, now: datetime) -> Decision | None:
    """Standing alone can end a request before its text is read."""
    if standing.banned:
        return Decision("ban", "this account is banned; the owner can lift it", standing=standing)
    if standing.blocked_until and standing.blocked_until > now:
        return Decision(
            "block",
            "you are blocked for now after an earlier request was refused",
            until=standing.blocked_until,
            standing=standing,
        )
    return None


def decide(
    standing: Standing,
    found: list[Signal],
    model_unsafe: bool | None,
    now: datetime,
    rules: Rules = Rules(),  # noqa: B008 — frozen
) -> Decision:
    """The outcome for one request, after :func:`pre_check`.

    `model_unsafe` is `None` when the model was not asked (a deterministic signal decided, or it
    was unavailable), `True`/`False` for its verdict. The model can add a refusal; it can never
    undo one.
    """
    hard = [s for s in found if s.severity == "hard"]
    if hard:
        s = hard[0]
        return Decision(
            "ban",
            f"banned: the request {_why(s)}",
            signal=s.name,
            ip_block=rules.ip_block,
            standing=Standing(banned=True, consecutive_unsafe=standing.consecutive_unsafe + 1),
        )
    if found:  # soft
        s = found[0]
        streak = standing.consecutive_unsafe + 1
        if streak >= rules.consecutive_limit:
            return Decision(
                "ban",
                f"banned: {streak} unsafe requests in a row (the last {_why(s)})",
                signal=s.name,
                ip_block=rules.ip_block,
                standing=replace_standing(standing, banned=True, consecutive_unsafe=streak),
            )
        return Decision(
            "refuse",
            f"refused: the request {_why(s)}",
            signal=s.name,
            standing=replace_standing(standing, consecutive_unsafe=streak),
        )
    if model_unsafe:
        strikes = standing.model_strikes + 1
        if strikes >= rules.model_strike_limit:
            return Decision(
                "ban",
                "banned: a second request was judged unsafe",
                signal="model",
                ip_block=rules.ip_block,
                standing=replace_standing(
                    standing,
                    banned=True,
                    model_strikes=strikes,
                    consecutive_unsafe=standing.consecutive_unsafe + 1,
                ),
            )
        until = now + rules.block
        return Decision(
            "block",
            "refused: the request looked unsafe; you are blocked for 24 hours "
            "(a repeat leads to a ban)",
            signal="model",
            until=until,
            standing=replace_standing(
                standing,
                blocked_until=until,
                model_strikes=strikes,
                consecutive_unsafe=standing.consecutive_unsafe + 1,
            ),
        )
    return Decision(
        "allow", "ok", standing=replace_standing(standing, consecutive_unsafe=0), ask_model=False
    )


def replace_standing(s: Standing, **changes) -> Standing:
    return Standing(**{**s.__dict__, **changes})


def _why(s: Signal) -> str:
    return {
        "injection": "tried to override the rules or break out of the data it is given",
        "secrets": "asked for a key, password or other secret",
        "hidden_text": "contained hidden text",
        "forbidden_schema": "reached for data outside the allowed schemas",
        "destructive": "asked to delete or break existing data",
        "abuse": "was built to overload the system",
    }.get(s.name, "was unsafe")


# ── what a person may do today ──────────────────────────────────────────────────────────────


@dataclass(frozen=True)
class Limits:
    """Per person per day, and the other published caps. The numbers are set before launch."""

    questions_per_day: int = 30
    model_calls_per_day: int = 60
    additive_per_day: int = 5
    modifying_per_day: int = 1
    dashboards: int = 20
    # Requests per minute from one address.
    ip_per_minute: int = 20
    # Days since first sign-in before a change to an existing mart is allowed.
    modifying_after_days: int = 3
    # Days of inactivity before a dashboard is archived.
    archive_after_days: int = 30


@dataclass(frozen=True)
class Usage:
    questions: int = 0
    model_calls: int = 0
    additive: int = 0
    modifying: int = 0
    dashboards: int = 0


Kind = Literal["question", "model_call", "additive", "modifying", "dashboard"]


def check_limit(
    kind: Kind,
    usage: Usage,
    limits: Limits,
    first_seen: datetime,
    now: datetime,
) -> str | None:
    """Why `kind` is not allowed now, or `None` when it is. Does not spend anything."""
    cap = {
        "question": (usage.questions, limits.questions_per_day, "questions today"),
        "model_call": (usage.model_calls, limits.model_calls_per_day, "model calls today"),
        "additive": (usage.additive, limits.additive_per_day, "additive changes today"),
        "modifying": (usage.modifying, limits.modifying_per_day, "changes to existing marts today"),
        "dashboard": (usage.dashboards, limits.dashboards, "dashboards in all"),
    }[kind]
    used, allowed, what = cap
    if kind == "modifying" and now - first_seen < timedelta(days=limits.modifying_after_days):
        return (
            f"changes to existing marts open {limits.modifying_after_days} days "
            "after your first sign-in"
        )
    if used >= allowed:
        return f"you have used all {allowed} {what}"
    return None
