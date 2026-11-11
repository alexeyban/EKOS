"""SonarCloud's shell rules that have fired on this repository, for the changed shell scripts.

* S7688 — a `[ … ]` test where `[[ … ]]` should be used;
* S131  — a `case` without a default `*)` branch;
* S6505 — `npm install` / `npm ci` without `--ignore-scripts`;
* S7679 — a positional parameter (`$1` … `$9`) used other than by assigning it to a variable.

`shcheck.py <base>` → JSON findings for changed_lines.py, one per line. The files come from git,
never from the command line. Standard library only.
"""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from changed_lines import changed, git_diff, repo_root

SINGLE_TEST = re.compile(r"(?:^|[\s;&|(!])\[\s.*?\s\](?:\s|;|$)")
COMMENT = re.compile(r"(?:^|(?<=\s))#.*$")
NPM = re.compile(r"\bnpm\s+(?:install|ci|i)\b")
POSITIONAL = re.compile(r"\$\{?[1-9]")
# `name="$1"`, `name="${1:-default}"`, `local name="${2}"`: the parameter goes into a variable.
ASSIGN = re.compile(
    r"^\s*(?:local\s+|readonly\s+)?[A-Za-z_][A-Za-z0-9_]*=\"?\$\{?[1-9](?:\}|:-|:=|\"|\s|;|$)"
)


LINE_RULES = (
    (
        "S7688",
        "use '[[' instead of '[' for conditional tests",
        lambda line: bool(SINGLE_TEST.search(line)) and "[[" not in line,
    ),
    (
        "S6505",
        "pass --ignore-scripts so no lifecycle script runs",
        lambda line: bool(NPM.search(line)) and "--ignore-scripts" not in line,
    ),
    (
        "S7679",
        "assign the positional parameter to a variable first",
        lambda line: bool(POSITIONAL.search(line)) and not ASSIGN.match(line),
    ),
)


def code(raw: str) -> str:
    """The line without its comment (a `#!` line is no code)."""
    # A `#` starts a comment only at the start of a word: `${#a[@]}` and `"#"` are code.
    return "" if raw.lstrip().startswith("#!") else COMMENT.sub("", raw)


def cases_without_default(lines: list[str]):
    """The first line of every `case … esac` that has no `*)` branch."""
    start = None
    default = False
    for n, line in enumerate(lines, 1):
        if re.search(r"(^|\s)case\s", line):
            start, default = n, False
        default = default or bool(re.search(r"(^|\s)\*\)", line))
        if start is not None and re.search(r"(^|[\s;])esac\b", line):
            if not default:
                yield start
            start = None


def findings(rel: str, text: str):
    lines = [code(raw) for raw in text.splitlines()]
    for n, line in enumerate(lines, 1):
        for rule, message, hit in LINE_RULES:
            if hit(line):
                yield {"file": rel, "line": n, "rule": rule, "message": message}
    for n in cases_without_default(lines):
        yield {
            "file": rel,
            "line": n,
            "rule": "S131",
            "message": "add a default case (*) for unexpected values",
        }


def main() -> int:
    root = repo_root()
    for rel in sorted(changed(git_diff(sys.argv[1]))):
        path = (root / rel).resolve()
        if (
            not rel.endswith(".sh")
            or rel.startswith("docs/")
            or not path.is_relative_to(root)
        ):
            continue
        if path.is_file():
            for x in findings(rel, path.read_text(encoding="utf-8")):
                print(json.dumps(x))
    return 0


if __name__ == "__main__":
    sys.exit(main())
