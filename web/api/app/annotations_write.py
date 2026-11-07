"""Annotations (RFC 0247) — the console's human write seam for `ekos annotate`.

`ekos annotate add|withdraw|approve|confirm` are human-only: no MCP tool reaches them (the only MCP
tool, `ekos_annotations`, reads). The console is a human surface, so it runs exactly those commands
for a signed-in person and records who with `--as`, by the same identity rule as a semantics review
(`semantics_write.reviewer_for`).

A person's text is data. It is never a flag: every value rides on its own `--flag=value`, and the
positionals (`add`'s target, kind and text) come after a literal `--`, so a note that starts with
`-` cannot become an option.
"""

from __future__ import annotations

import json
from typing import Any

from .semantics_write import _UUID, ReviewError, _clean

KINDS = ("comment", "instruction", "synonym")
# The closed rule vocabulary of `ekos_kir::annotations::Rule` (a value is checked again by the CLI).
RULES = frozenset({"default_filter", "preferred_join", "unit", "fiscal_year_start", "exclude"})
_MAX_RULE = 4000


def target_ok(target: str) -> str:
    """An entity id, or `project` — the only forms the console passes as a target."""
    if target == "project" or _UUID.match(target):
        return target
    raise ReviewError("target is an entity id (a UUID) or `project`")


def _rule_json(rule: dict[str, Any] | None) -> str | None:
    if rule is None:
        return None
    if not isinstance(rule, dict) or rule.get("rule") not in RULES:
        raise ReviewError(f"a rule is an object whose `rule` is one of {sorted(RULES)}")
    text = json.dumps(rule, separators=(",", ":"), ensure_ascii=False)
    if len(text) > _MAX_RULE:
        raise ReviewError("the rule is too long")
    if "\x00" in text:
        raise ReviewError("the rule contains a NUL byte")
    return text


def add_argv(
    target: str,
    kind: str,
    text: str,
    author: str,
    *,
    rule: dict[str, Any] | None = None,
    term: str | None = None,
    shared: bool = False,
) -> list[str]:
    """`ekos annotate add --as=… [--shared] [--rule=…] [--term=…] -- <target> <kind> <text>`."""
    if kind not in KINDS:
        raise ReviewError(f"kind is one of {', '.join(KINDS)}")
    body = _clean("text", text)
    if not body:
        raise ReviewError("an annotation needs text")
    target = target_ok(target)
    rule_text = _rule_json(rule)
    term = _clean("term", term)
    if kind == "comment" and rule_text:
        raise ReviewError("a comment carries no rule")
    if kind == "synonym" and not term:
        raise ReviewError("a synonym needs the business word (term)")
    if kind != "synonym" and term:
        raise ReviewError("only a synonym has a term")
    argv = ["annotate", "add", f"--as={author}"]
    if shared:
        argv.append("--shared")
    if rule_text:
        argv.append(f"--rule={rule_text}")
    if term:
        argv.append(f"--term={term}")
    return [*argv, "--", target, kind, body]


def act_argv(action: str, annotation_id: str, person: str) -> list[str]:
    """`ekos annotate withdraw|approve|confirm <id> --as=…` — the full id only."""
    if action not in ("withdraw", "approve", "confirm"):
        raise ReviewError(f"unknown action {action!r}")
    if not _UUID.match(annotation_id):
        raise ReviewError("annotation id must be a UUID")
    return ["annotate", action, annotation_id, f"--as={person}"]
