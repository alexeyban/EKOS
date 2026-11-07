#!/usr/bin/env python3
"""The one input class where the original is not deterministic: EncodeEmailAddress uses System.Random.

Run the original twice and the port twice on an email autolink. All four outputs differ byte for byte;
after HTML-entity decoding all four are the same string.
"""
import html, os, pathlib, subprocess, sys

HERE = pathlib.Path(__file__).resolve().parent.parent
data = (HERE / "inputs/99-email-random.md").read_bytes()
env = dict(os.environ, WINEPREFIX=os.path.expanduser("~/.cache/ekos-characterize/prefix"), WINEDEBUG="-all")
outs = []
for label in ("original", "original", "port", "port"):
    if label == "original":
        out = subprocess.run(["wine", str(HERE / "work/original/mdcli.exe")], input=data, capture_output=True, env=env).stdout
    else:
        out = subprocess.run(["python3.13", "-m", "mdport"], input=data, capture_output=True, cwd=HERE / "python-port").stdout
    outs.append((label, out.decode("utf-8")))
for label, o in outs:
    o = o.strip()
    print(f"  {label:<8} {o[:56]}…{o[-44:]}")
same_raw = len({o for _, o in outs}) == 1
decoded = {html.unescape(o) for _, o in outs}
print(f"\n  byte-identical across the four runs : {same_raw}")
print(f"  identical after decoding entities   : {len(decoded) == 1}")
print(f"  decoded: {next(iter(decoded)).strip()}")
sys.exit(0 if len(decoded) == 1 else 1)
