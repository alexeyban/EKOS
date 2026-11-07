#!/usr/bin/env python3
"""Publish the two code artifacts the deck links to, into docs/presentations/assets/binary-demo/code/:
  decompiled/MarkdownSharp.recovered.txt   recovered pseudo-code of every type (IL offset on every line)
  python-port/*.py                         the Python rewrite
"""
import os, pathlib, re, shutil, subprocess, sys

HERE = pathlib.Path(__file__).resolve().parent.parent
OUT = HERE.parent.parent / "docs/presentations/assets/binary-demo/code"
REPO = pathlib.Path(os.environ.get("EKOS_BINARY_REPO", pathlib.Path.home() / "PycharmProjects/ekos-binary"))
DLL = HERE / "work/original/MarkdownSharp.dll"
ekos = HERE / "work/bin/ekos"

names = set()
out = subprocess.run([str(ekos), "query", "find", "MarkdownSharp"], capture_output=True, text=True, cwd=HERE / "work/demo-ws").stdout
for line in out.splitlines():
    parts = line.split(None, 1)
    if len(parts) == 2 and re.fullmatch(r"MarkdownSharp\.[A-Za-z_+<>0-9]+", parts[1].strip()):
        names.add(parts[1].strip())
names = sorted(names, key=lambda n: (n.count("+"), n))

(OUT / "decompiled").mkdir(parents=True, exist_ok=True)
with open(OUT / "decompiled/MarkdownSharp.recovered.txt", "w", encoding="utf-8") as f:
    f.write("// Recovered from MarkdownSharp.dll 2.0.5 (net40, MIT) by ekos-binary. Pseudo-code, not compilable C#.\n")
    f.write("// Every line cites its IL offset. Types: " + ", ".join(names) + "\n")
    for n in names:
        f.write(f"\n\n// {'=' * 100}\n// TYPE {n}\n// {'=' * 100}\n\n")
        f.write(subprocess.run([str(REPO / "target/release/examples/render"), str(DLL), n], capture_output=True, text=True).stdout)

dst = OUT / "python-port"
dst.mkdir(parents=True, exist_ok=True)
for p in (HERE / "python-port").rglob("*.py"):
    rel = p.relative_to(HERE / "python-port")
    (dst / rel).parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(p, dst / rel)
print(f"published {len(names)} types and {len(list(dst.rglob('*.py')))} python files to {OUT}")
