"""Model calls per workflow step, from every run log of a project (EKOS RFC 0220/0218).

    python3 model_calls.py <state>/runs [<console runs dir> <console db> <workspace id>]

A run log ends with an `llm` line counting the model calls its process made; this sums them by the
step the command belongs to. Console runs are read from the console's runs directory, limited to the
workspace's runs through the console database.
"""
import collections, json, os, re, sqlite3, sys
from contextlib import closing
from pathlib import Path
from urllib.parse import quote

# A console workspace id: what the console accepts as a name.
_WORKSPACE_ID = re.compile(r"[A-Za-z0-9][A-Za-z0-9._-]{0,99}")

STEPS = [
    (1, ("build", "recover", "resolve", "compile", "commit", "init", "database test")),
    (2, ("docs",)),
    (3, ("signoff", "review", "conflicts", "identity")),
    (4, ("lakehouse requirements",)),
    (5, ("lakehouse extraction", "lakehouse plan")),
    (6, ("lakehouse run", "lakehouse generate", "lakehouse sync", "lakehouse reconcile", "lakehouse deletes")),
    (7, ("lakehouse marts", "lakehouse gold")),
]


def step(cmd: str) -> int:
    for n, prefixes in STEPS:
        if any(cmd == p or cmd.startswith(p + " ") for p in prefixes):
            return n
    return 0


def _bases() -> list[Path]:
    """Where run logs and the console database may be read from: the home directory, the working
    directory and the EKOS home, if one is set."""
    bases = [Path.home(), Path.cwd()]
    if os.environ.get("EKOS_HOME"):
        bases.append(Path(os.environ["EKOS_HOME"]).expanduser())
    return [b.resolve() for b in bases]


def _inside(raw: str, what: str) -> Path:
    p = Path(raw).expanduser().resolve()
    if not any(p.is_relative_to(b) for b in _bases()):
        sys.exit(f"{raw}: the {what} must be under the home, working or EKOS home directory")
    return p


def runs_dir(raw: str) -> Path:
    """A `runs` directory of run logs, resolved and checked."""
    p = _inside(raw, "runs directory")
    if not p.is_dir():
        sys.exit(f"{raw}: not a directory")
    return p


def run_logs(directory: Path) -> list[Path]:
    """The `*.jsonl` run logs directly in `directory`, never one a link points out of it to."""
    return sorted(f.resolve() for f in directory.glob("*.jsonl")
                  if f.resolve().parent == directory and f.resolve().is_file())


def console_db(raw: str) -> str:
    """The console database as a read-only SQLite URI: an existing file, the path quoted, so no
    part of the argument can become a URI parameter."""
    p = _inside(raw, "console database")
    if not p.is_file():
        sys.exit(f"{raw}: not a file")
    return f"file:{quote(str(p))}?mode=ro"


def workspace_id(raw: str) -> str:
    if not _WORKSPACE_ID.fullmatch(raw):
        sys.exit(f"{raw!r} is not a workspace id")
    return raw


def main(argv):
    if len(argv) not in (2, 5):
        sys.exit(__doc__)
    files = run_logs(runs_dir(argv[1]))
    if len(argv) == 5:
        control, ws = runs_dir(argv[2]), workspace_id(argv[4])
        with closing(sqlite3.connect(console_db(argv[3]), uri=True)) as conn:
            logged = conn.execute("select log_path from run where workspace_id = ?", (ws,))
            mine = {Path(r[0][: -len(".log")] + ".jsonl").resolve() for r in logged}
        files += [f for f in run_logs(control) if f in mine]
    runs, calls = collections.Counter(), collections.Counter()
    for f in files:
        for line in f.open(errors="replace"):
            try:
                e = json.loads(line)
            except ValueError:
                continue
            if e.get("stage") == "llm":
                n = step(e.get("cmd", ""))
                runs[n] += 1
                calls[n] += int(e.get("count") or 0)
    print("| Step | Runs with a run log | Model calls |\n|---|---|---|")
    for n in range(1, 8):
        print(f"| {n} | {runs[n]} | {calls[n] if runs[n] else '—'} |")


if __name__ == "__main__":
    main(sys.argv)
