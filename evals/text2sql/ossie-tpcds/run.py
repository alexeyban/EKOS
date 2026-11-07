"""RFC 0246 A/B: `ekos sql generate` on the TPC-DS fixture, without and with the Apache Ossie model
in the sources, on a local Ollama model only (never a billed one).

    python3 run.py --ekos <ekos binary> [--model qwen2.5-coder:3b] [--repeats 1] [--out report.json]

Each question is scored like BIRD EX: gold and predicted result sets compared as sets of rows,
floats rounded to 2 places. Needs PyYAML (run under `uv run --with pyyaml`).
"""

from __future__ import annotations

import argparse
import json
import shutil
import sqlite3
import subprocess
import sys
import tempfile
from pathlib import Path

import yaml

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
FIXTURE = ROOT / "tests" / "fixtures" / "ossie-tpcds"
sys.path.insert(0, str(HERE))
from make_db import build  # noqa: E402


def rows(db: Path, sql: str) -> set[tuple] | str:
    try:
        con = sqlite3.connect(f"file:{db}?mode=ro", uri=True)
        out = con.execute(sql).fetchall()
        con.close()
    except sqlite3.Error as e:
        return f"error: {e}"
    norm = lambda v: round(v, 2) if isinstance(v, float) else v  # noqa: E731
    return {tuple(norm(v) for v in r) for r in out}


def ekos(ekos_bin: str, ws: Path, *args: str) -> subprocess.CompletedProcess:
    return subprocess.run(
        [ekos_bin, *args], cwd=ws, capture_output=True, text=True, timeout=900,
        env={"PATH": "/usr/bin:/bin", "HOME": str(ws), "EKOS_HOME": str(ws / ".home"),
             "NO_COLOR": "1"},
    )


def workspace(base: Path, with_ossie: bool, ekos_bin: str, model: str) -> Path:
    ws = base / ("with" if with_ossie else "without")
    shutil.copytree(FIXTURE / "db", ws / "db")
    if with_ossie:
        shutil.copytree(FIXTURE / "semantic", ws / "semantic")
    ekos(ekos_bin, ws, "init")
    toml = ws / "ekos.toml"
    toml.write_text(toml.read_text() + '\n[recover.sql]\ndefault-dialect = "postgres"\n')
    for stage in ("build", "recover", "resolve", "compile", "commit"):
        r = ekos(ekos_bin, ws, stage)
        if r.returncode != 0:
            sys.exit(f"{stage} failed:\n{r.stdout}\n{r.stderr}")
    # The model route only after the pipeline: an [llm] section makes `recover` call it too.
    toml.write_text(toml.read_text() + f'\n[llm]\nprovider = "ollama"\nmodel = "{model}"\n')
    return ws


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--ekos", required=True)
    ap.add_argument("--model", default="qwen2.5-coder:3b")
    ap.add_argument("--repeats", type=int, default=1)
    ap.add_argument("--out")
    a = ap.parse_args()
    questions = yaml.safe_load((HERE / "questions.yaml").read_text())["questions"]
    with tempfile.TemporaryDirectory() as tmp:
        base = Path(tmp)
        db = base / "tpcds.sqlite"
        build(str(db))
        results: dict[str, list[dict]] = {}
        for variant in ("without", "with"):
            ws = workspace(base, variant == "with", a.ekos, a.model)
            results[variant] = []
            for q in questions:
                gold = rows(db, q["gold"])
                for rep in range(a.repeats):
                    r = ekos(a.ekos, ws, "sql", "generate", q["question"], "--sqlite", str(db),
                             "--json")
                    try:
                        out = json.loads(r.stdout)
                        sql = out.get("sql")
                        tools = [t if isinstance(t, str) else t.get("tool") for t in out.get("tools", [])]
                    except json.JSONDecodeError:
                        sql, tools = None, []
                    got = rows(db, sql) if sql else "no SQL"
                    ok = got == gold
                    results[variant].append({"id": q["id"], "rep": rep, "ok": ok, "sql": sql,
                                             "tools": tools})
                    print(f"{variant:8} {q['id']} {'PASS' if ok else 'fail'}  {sql!r}"[:200],
                          flush=True)
    summary = {
        v: {"correct": sum(r["ok"] for r in rs), "total": len(rs)} for v, rs in results.items()
    }
    print(json.dumps(summary))
    if a.out:
        Path(a.out).write_text(json.dumps({"model": a.model, "summary": summary,
                                           "results": results}, indent=2))


if __name__ == "__main__":
    main()
