#!/usr/bin/env python3
"""Turn inputs/*.md into an ekos-characterize case file: one case per fixture, calling Markdown.Transform."""
import json, pathlib, sys

here = pathlib.Path(__file__).resolve().parent.parent
cases = []
for f in sorted((here / "inputs").glob("*.md")):
    if f.name.startswith("99-"):  # System.Random in EncodeEmailAddress: nondeterministic by design
        continue
    text = f.read_bytes().decode("utf-8")
    cases.append({
        "id": f.stem,
        "type": "MarkdownSharp.Markdown",
        "method": "Transform",
        "args": [text],
        "python": {"call": "Markdown.transform", "ctor": []},
    })
cases.append({"id": "null-input", "type": "MarkdownSharp.Markdown", "method": "Transform", "args": [None],
              "python": {"call": "Markdown.transform", "ctor": []}})
out = {"assembly": "original/MarkdownSharp.dll", "python_module": "../python-port/port_entry.py", "timeout_ms": 20000, "cases": cases}
(here / "work" / "cases.json").write_text(json.dumps(out, indent=1, ensure_ascii=False))
print(f"{len(cases)} cases -> work/cases.json")
