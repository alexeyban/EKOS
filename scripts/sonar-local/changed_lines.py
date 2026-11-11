"""Keep only findings on lines a change touched — SonarCloud's "new code".

`changed_lines.py <base>` asks git for the diff since <base> (committed, staged or not) and reads JSON
findings ({"file","line","rule","message"[,"anyline"]} per line) on stdin; it prints those on
changed lines. Paths are never taken from the command line: the changed files come from git and
are read relative to the repository root. A new file counts once it is added (`git add`): files git
does not know are not checked, as SonarCloud never sees them. Standard library only.
"""

from __future__ import annotations

import json
import re
import subprocess
import sys
from pathlib import Path

HUNK = re.compile(r"^@@ -\d+(?:,\d+)? \+(\d+)(?:,(\d+))? @@")
REV = re.compile(r"^[0-9A-Za-z_./~^@{}-]{1,200}$")


def repo_root() -> Path:
    out = subprocess.run(
        ["git", "rev-parse", "--show-toplevel"],
        check=True,
        capture_output=True,
        text=True,
    )
    return Path(out.stdout.strip()).resolve()


def git_diff(base: str) -> str:
    if not REV.match(base) or base.startswith("-"):
        raise SystemExit(f"not a git revision: {base!r}")
    out = subprocess.run(
        ["git", "diff", "-U0", base, "--"], check=True, capture_output=True, text=True
    )
    return out.stdout


def changed(diff: str) -> dict[str, set[int]]:
    """Repository-relative path → the line numbers the diff added or changed."""
    out: dict[str, set[int]] = {}
    current = None
    for line in diff.splitlines():
        if line.startswith("+++ "):
            path = line[4:].strip()
            current = path[2:] if path.startswith("b/") else None
            continue
        m = HUNK.match(line)
        if m and current:
            start, count = int(m.group(1)), int(m.group(2) or "1")
            out.setdefault(current, set()).update(range(start, start + count))
    return out


def relative(name: str, root: Path) -> str:
    p = Path(name)
    if p.is_absolute():
        try:
            return str(p.resolve().relative_to(root))
        except ValueError:
            return name
    return name


def main() -> int:
    root = repo_root()
    lines = changed(git_diff(sys.argv[1]))
    shown = 0
    for raw in sys.stdin:
        raw = raw.strip()
        if not raw:
            continue
        f = json.loads(raw)
        rel = relative(f["file"], root)
        # A function-level rule (complexity) counts when any line of the file changed: its
        # finding sits on the function's first line, which a change to the body leaves alone.
        hit = (
            rel in lines if f.get("anyline") else f.get("line") in lines.get(rel, set())
        )
        if hit:
            print(f"{rel}:{f['line']}  {f['rule']}  {f['message']}")
            shown += 1
    return 1 if shown else 0


if __name__ == "__main__":
    sys.exit(main())
