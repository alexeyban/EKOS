"""Keep only findings on lines a change touched — SonarCloud's "new code".

Reads `git diff -U0 <base>` (stdin of `--diff`) for the added/changed line ranges per file, then
JSON findings ({"file","line","rule","message"} per line) and prints those that fall on them.
Run by scripts/sonar-local.sh; standard library only.
"""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path

HUNK = re.compile(r"^@@ -\d+(?:,\d+)? \+(\d+)(?:,(\d+))? @@")


def changed(diff: str) -> dict[str, set[int]]:
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


def main() -> int:
    diff = Path(sys.argv[1]).read_text()
    root = Path(sys.argv[2]).resolve()
    lines = changed(diff)
    shown = 0
    for raw in sys.stdin:
        raw = raw.strip()
        if not raw:
            continue
        f = json.loads(raw)
        path = Path(f["file"])
        rel = str(path.resolve().relative_to(root)) if path.is_absolute() else f["file"]
        # A function-level rule (complexity) counts when any line of the file changed: its
        # finding sits on the function's first line, which a change to the body leaves alone.
        hit = rel in lines if f.get("anyline") else f.get("line") in lines.get(rel, set())
        if hit:
            print(f"{rel}:{f['line']}  {f['rule']}  {f['message']}")
            shown += 1
    return 1 if shown else 0


if __name__ == "__main__":
    sys.exit(main())
