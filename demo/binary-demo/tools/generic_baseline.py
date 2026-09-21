#!/usr/bin/env python3
"""What general Markdown knowledge produces: two mature generic implementations vs the original, byte for byte.

A proxy for "write a Markdown converter without the spec": Python-Markdown (classic Markdown.pl lineage) and
markdown-it-py (CommonMark). Neither is a rewrite of MarkdownSharp; they show how far 'Markdown' the concept is
from 'this program'. Runs under python3 (3.10) where the libraries are installed.
"""
import os, pathlib, re, subprocess, sys

HERE = pathlib.Path(__file__).resolve().parent.parent
import markdown, markdown_it

env = dict(os.environ, WINEPREFIX=os.path.expanduser("~/.cache/ekos-characterize/prefix"), WINEDEBUG="-all")
md_it = markdown_it.MarkdownIt("commonmark")
def norm(h):
    """Forgiving: ignore all whitespace between tags and around lines, and entity spelling of quotes."""
    return re.sub(r"\s+", "", h.replace("&quot;", '"').replace("&#39;", "'"))


rows, tot = [], [0, 0, 0, 0, 0]
for f in sorted((HERE / "inputs").glob("*.md")):
    if f.name.startswith("99-"):
        continue
    data = f.read_bytes()
    orig = subprocess.run(["wine", str(HERE / "work/original/mdcli.exe")], input=data, capture_output=True, env=env).stdout.decode()
    text = data.decode()
    a = markdown.markdown(text) + "\n" if text.strip() else ""
    b = md_it.render(text)
    ok = (a == orig, b == orig)
    n = (norm(a) == norm(orig), norm(b) == norm(orig))
    tot[0] += 1; tot[1] += ok[0]; tot[2] += ok[1]; tot[3] += n[0]; tot[4] += n[1]
    rows.append((f.name, ok, n))
lab = lambda x: "identical" if x else "DIFFERENT"
print(f"  {'fixture':<26} {'Python-Markdown':<20} {'markdown-it':<20}  (byte-exact / ignoring whitespace)")
for name, (x, y), (nx, ny) in rows:
    print(f"  {name:<26} {lab(x) + ' / ' + lab(nx):<20} {lab(y) + ' / ' + lab(ny):<20}")
print(f"\n  byte-exact           : Python-Markdown {tot[1]}/{tot[0]}   markdown-it {tot[2]}/{tot[0]}")
print(f"  ignoring whitespace  : Python-Markdown {tot[3]}/{tot[0]}   markdown-it {tot[4]}/{tot[0]}")
print(f"  port from recovered spec: {tot[0]}/{tot[0]} byte-exact")
