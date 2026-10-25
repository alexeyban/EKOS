"""Review decisions and sign-off (RFC 0213) — the console's third human write seam.

`ekos review scope|pii|write-behaviour|identity` and `ekos signoff approve|revoke` are human-only:
no MCP tool reaches them (`review_guard::HUMAN_ONLY`). The console runs exactly those commands for
a signed-in person with the `review` right, recording who with `--as` by the same rule as a
semantics review (`semantics_write.reviewer_for`). Options come first and the targets after `--`,
so no table name can become an option.
"""

from __future__ import annotations

import re

from .readproc import REVIEW_SUBJECTS
from .semantics_write import ReviewError, _clean

# Subjects whose items are decided on the Review page; conflicts and the glossary have their own
# pages (Conflicts, Semantics).
DECIDED_HERE = ("scope", "pii", "write-behaviour", "identity")
_VERDICTS = {
    "scope": ("in", "out"),
    "pii": ("confirm", "reject"),
    "write-behaviour": ("confirm",),
    "identity": ("confirm", "reject"),
}
_MASKS = ("hash", "redact", "none")
_OVERRIDE_FIELDS = ("update_class", "delete_classes", "change_log")
_MAX_TARGETS = 500
_MAX_TARGET = 300
_DIGEST = re.compile(r"^[0-9a-f]{64}$")


def _subject(subject: str) -> str:
    if subject not in REVIEW_SUBJECTS:
        raise ReviewError(f"unknown review subject {subject!r}")
    return subject


def _targets(targets: list[str]) -> list[str]:
    if not targets:
        raise ReviewError("no items selected")
    if len(targets) > _MAX_TARGETS:
        raise ReviewError(f"at most {_MAX_TARGETS} items at once")
    if len(set(targets)) != len(targets):
        raise ReviewError("an item is selected twice")
    for t in targets:
        if not isinstance(t, str) or not t.strip() or len(t) > _MAX_TARGET:
            raise ReviewError(f"an item is named by 1 to {_MAX_TARGET} characters")
        if any(c in t for c in "\x00\n\r"):
            raise ReviewError("an item name contains a control character")
    return targets


def decide_argv(
    subject: str,
    targets: list[str],
    reviewer: str,
    *,
    verdict: str,
    mask: str | None = None,
    reason: str | None = None,
    overrides: dict[str, str] | None = None,
) -> list[str]:
    """The exact argv for one batch of decisions on one subject."""
    if _subject(subject) not in DECIDED_HERE:
        raise ReviewError(f"{subject} items are decided on their own page")
    if verdict not in _VERDICTS[subject] and not (subject == "write-behaviour" and overrides):
        raise ReviewError(f"a {subject} decision is one of {', '.join(_VERDICTS[subject])}")
    targets = _targets(targets)
    reason = _clean("reason", reason)
    argv = ["review", subject]
    if subject == "scope":
        argv.append(f"--{verdict}")
    elif subject == "pii":
        argv.append(f"--{verdict}")
        if mask is not None:
            if verdict != "confirm" or mask not in _MASKS:
                raise ReviewError(f"a mask ({', '.join(_MASKS)}) goes with a confirmation")
            argv.append(f"--mask={mask}")
    elif subject == "write-behaviour":
        if overrides:
            if not reason:
                raise ReviewError("an override needs a reason")
            for k, v in overrides.items():
                if k not in _OVERRIDE_FIELDS:
                    raise ReviewError(f"cannot override {k!r}")
                v = _clean(k, v)
                if not v:
                    raise ReviewError(f"{k} needs a value")
                argv.append(f"--set={k}={v}")
        else:
            argv.append("--confirm")
    else:  # identity
        argv.append(f"--{verdict}")
        if verdict == "reject" and not reason:
            raise ReviewError("say why the match is wrong")
        if reason:
            argv.append(f"--note={reason}")
        reason = None
    if reason:
        argv.append(f"--reason={reason}")
    argv.append(f"--as={reviewer}")
    return [*argv, "--", *targets]


def approve_argv(subject: str, digest: str, reviewer: str, *, note: str | None) -> list[str]:
    """`ekos signoff approve <subject> --digest=<the digest the person was shown>`."""
    if not _DIGEST.match(digest or ""):
        raise ReviewError("the digest is the 64 hex characters the review showed")
    argv = ["signoff", "approve", _subject(subject), f"--digest={digest}", f"--as={reviewer}"]
    note = _clean("note", note)
    if note:
        argv.append(f"--note={note}")
    return argv


def revoke_argv(subject: str, reason: str | None, reviewer: str) -> list[str]:
    reason = _clean("reason", reason)
    if not reason:
        raise ReviewError("say why the sign-off is withdrawn")
    return ["signoff", "revoke", _subject(subject), f"--reason={reason}", f"--as={reviewer}"]
