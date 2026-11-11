"""SonarCloud Python rules with no ruff equivalent, for the changed Python files.

* S5850 — an alternation whose anchors (`^`, `$`) sit on some branches only: `^a|b` reads as
  `(^a)|b`, which is rarely what was meant. Group the branches to say it.

`pyrules.py <base>` → JSON findings for changed_lines.py. Regular expressions are the string
literals passed to `re.compile` / `re.search` / `re.match` / `re.fullmatch` / `re.sub` /
`re.split` / `re.findall` / `re.finditer` (adjacent literals joined). The files come from git,
never from the command line. Standard library only.
"""

from __future__ import annotations

import ast
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from changed_lines import changed, git_diff, repo_root
from pycogc import NOT_MAIN

RE_FUNCS = {
    "compile",
    "search",
    "match",
    "fullmatch",
    "sub",
    "subn",
    "split",
    "findall",
    "finditer",
}


def top_level_branches(pattern: str) -> list[str]:
    """The alternatives of `pattern` at depth 0 (outside groups and character classes)."""
    branches, depth, start, i = [], 0, 0, 0
    in_class = False
    while i < len(pattern):
        c = pattern[i]
        if c == "\\":
            i += 2
            continue
        if in_class:
            in_class = c != "]"
        elif c == "[":
            in_class = True
        elif c == "(":
            depth += 1
        elif c == ")":
            depth -= 1
        elif c == "|" and depth == 0:
            branches.append(pattern[start:i])
            start = i + 1
        i += 1
    branches.append(pattern[start:])
    return branches


def ambiguous_anchors(pattern: str) -> bool:
    branches = top_level_branches(pattern)
    if len(branches) < 2:
        return False
    starts = [b.startswith("^") for b in branches]
    ends = [b.endswith("$") and not b.endswith("\\$") for b in branches]
    return (any(starts) and not all(starts)) or (any(ends) and not all(ends))


def regexes(tree: ast.AST):
    for node in ast.walk(tree):
        if not (isinstance(node, ast.Call) and node.args):
            continue
        f = node.func
        if (
            isinstance(f, ast.Attribute)
            and f.attr in RE_FUNCS
            and isinstance(f.value, ast.Name)
            and f.value.id == "re"
        ):
            arg = node.args[0]
            if isinstance(arg, ast.Constant) and isinstance(arg.value, str):
                yield arg.lineno, arg.value


def main() -> int:
    root = repo_root()
    for rel in sorted(changed(git_diff(sys.argv[1]))):
        path = (root / rel).resolve()
        if (
            not rel.endswith(".py")
            or NOT_MAIN.search(rel)
            or not path.is_relative_to(root)
        ):
            continue
        if not path.is_file():
            continue
        tree = ast.parse(path.read_text(encoding="utf-8"), filename=rel)
        for line, pattern in regexes(tree):
            if ambiguous_anchors(pattern):
                print(
                    json.dumps(
                        {
                            "file": rel,
                            "line": line,
                            "rule": "S5850",
                            "anyline": True,
                            "message": "group the alternation to make the anchors' precedence explicit: "
                            + pattern[:60],
                        }
                    )
                )
    return 0


if __name__ == "__main__":
    sys.exit(main())
