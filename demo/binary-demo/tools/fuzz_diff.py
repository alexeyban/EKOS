#!/usr/bin/env python3
"""Seeded differential fuzz: N generated Markdown documents through the ORIGINAL (wine-mono) and the PORT."""
import os, random, subprocess, sys, pathlib

HERE = pathlib.Path(__file__).resolve().parent.parent
N = int(sys.argv[1]) if len(sys.argv) > 1 else 300
SEED = int(sys.argv[2]) if len(sys.argv) > 2 else 20260921

BANK = [
    "# Heading {i}", "## Sub *heading*", "Setext\n======", "Setext two\n----------", "###### deep ######",
    "plain paragraph number {i}", "text with *emph* and **strong** and ***both***", "snake_case_word and __init__",
    "a line with `code <b>` inside", "``double `tick` code``", "trailing two spaces  \nnext line",
    "* item a\n* item b\n    * nested\n* item c", "1. one\n2. two\n3. three", "7. seven\n8. eight",
    "- loose\n\n- items\n\n- here", "+ plus item\n+ another",
    "> quote line\n> second line", "> > nested quote\n> back", "    indented code\n    more code",
    "\tcode by tab", "[link](http://x.org/a_b \"t\")", "[ref][r1] and [r1] and [imp][]", "![img](/a.png \"cap\")",
    "![refimg][r1]", "<http://auto.example/p?a=1&b=2>", "<div>\n<p>raw</p>\n</div>", "<!-- comment -->", "<hr />",
    "---", "* * *", "_ _ _", "&copy; &amp; & < > \" '", "escaped \\* \\_ \\` \\\\ \\# \\[ \\]", "<span class=\"a\">inline</span>",
    "[r1]: http://example.com/one \"One\"", "[imp]: <http://example.com/imp>", "unclosed *emph", "unclosed **strong", "*",
    "**bold spanning\nlines** here", "*emph spanning\nlines* here", "[link text\nover lines](/u)", "> quote *with\nemph* inside",
    "* item with **bold\ncontinued**\n* next", "`code\nspanning` lines", "text<br/>with <b>tags</b> and <i>x</i>",
    "1 < 2 and 3 > 2", "café ünïcödé 🙂", "a\r\nb\r\nc", "text with (parens) and [brackets] and {braces}",
    "http://bare.example.com/path(with)parens", "line one\nline two\nline three", "   ", "",
]
rnd = random.Random(SEED)
docs = []
for i in range(N):
    parts = [rnd.choice(BANK).replace("{i}", str(i)) for _ in range(rnd.randint(1, 9))]
    docs.append(("\n\n" if rnd.random() < 0.8 else "\n").join(parts))
blob = "\0".join(docs).encode("utf-8")

env = dict(os.environ, WINEPREFIX=os.path.expanduser("~/.cache/ekos-characterize/prefix"), WINEDEBUG="-all")
orig = subprocess.run(["wine", str(HERE / "work/original/mdbatch.exe")], input=blob, capture_output=True, env=env).stdout.decode("utf-8").split("\0")

sys.path.insert(0, os.environ.get("PORT_DIR") or str(HERE / "python-port"))
from mdport import Markdown
port = []
for d in docs:
    try:
        port.append(Markdown().transform(d))
    except Exception as e:
        port.append("!EXC " + type(e).__name__)

bad = [i for i in range(N) if orig[i] != port[i]]
print(f"  seed {SEED}: {N} generated documents, {N - len(bad)} identical, {len(bad)} different")
for i in bad[:5]:
    print(f"  --- doc {i}: {docs[i]!r}\n  original: {orig[i]!r}\n  port:     {port[i]!r}")
sys.exit(1 if bad else 0)
