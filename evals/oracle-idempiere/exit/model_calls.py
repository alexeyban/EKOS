"""Model calls per workflow step, from every run log of a project (EKOS RFC 0220/0218).

    python3 model_calls.py <state>/runs [<console runs dir> <console db> <workspace id>]

A run log ends with an `llm` line counting the model calls its process made; this sums them by the
step the command belongs to. Console runs are read from the console's runs directory, limited to the
workspace's runs through the console database.
"""
import collections, glob, json, sqlite3, sys

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


def main(argv):
    files = glob.glob(argv[1] + "/*.jsonl")
    if len(argv) == 5:
        _, _, control, db, ws = argv
        mine = {r[0][: -len(".log")] + ".jsonl"
                for r in sqlite3.connect(db).execute("select log_path from run where workspace_id = ?", (ws,))}
        files += [f for f in glob.glob(control + "/*.jsonl") if f in mine]
    runs, calls = collections.Counter(), collections.Counter()
    for f in files:
        for line in open(f, errors="replace"):
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
