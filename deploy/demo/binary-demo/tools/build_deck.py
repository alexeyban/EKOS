#!/usr/bin/env python3
"""Build docs/presentations/compiled-app-to-python.html from assets/binary-demo/manifest.json."""
import html, json, pathlib

ROOT = pathlib.Path(__file__).resolve().parents[3]
OUT = ROOT / "docs/presentations/compiled-app-to-python.html"
M = json.loads((ROOT / "docs/presentations/assets/binary-demo/manifest.json").read_text())

CAP = {
 1: ("Start", "A DLL and nothing else", "MarkdownSharp 2.0.5 (MIT), a 51 KB compiled .NET assembly. No repository, no source. Only its string constants are readable."),
 2: ("Baseline", "Run the original: this is the behaviour to reproduce", "A 12-line harness calls <code class='mono'>new Markdown().Transform(text)</code>. It runs on wine-mono, with no .NET SDK installed."),
 3: ("Observe", "<code class='mono'>ekos build</code> reads the bytes", "The assembly is parsed, never executed. It becomes a content-addressed artifact."),
 4: ("Recover", "Recovery: 101 method bodies, 99 fully structured", "A hand-written CIL decoder builds control-flow graphs and structures them into if/loop/switch/try statements. No LLM key was set, so none was used."),
 5: ("Ledger", "The result is queryable knowledge", "Types, methods and the call graph are evidence-backed objects in the append-only ledger."),
 6: ("Plan", "The spec, and the order to port it in", "<code class='mono'>ekos_binary_explain</code> over MCP, the same server an AI agent uses. Callees come first, so every port step only depends on finished work."),
 7: ("Spec", "One method, statement by statement", "Every recovered line cites its IL offset. Regexes and replacement templates arrive as exact constants."),
 8: ("Honesty", "Where recovery falls short, it says so", "<code class='mono'>Normalize</code> is <code class='mono'>control_flow</code> with one unstructured goto. EKOS marks it partial and tells the porter not to guess. Two of 101 methods are in this state."),
 9: ("Rewrite", "The Python port", "Written callees-first from the recovered statements. Long regex literals were lifted from the spec by a script, and every method cites its token."),
 10: ("Static check", "What the rewrite touches vs what the original touches", "Evidence, not a verdict. It caught the config-file constructor that was left out on purpose. The rest are constants that moved files."),
 11: ("Sandbox", "Executing untrusted code, so prove the sandbox first", "Five escape attempts, all blocked. The probe was itself validated by running it unsandboxed, where all five succeed."),
 12: ("Characterize", "Record the original, check the rewrite", "The original runs once in the sandbox and its results become a golden file. The check needs only Python."),
 13: ("Compare", "Same inputs, both programs, byte for byte", "<code class='mono'>wine mdcli.exe</code> against <code class='mono'>python -m mdport</code>. Identical sha256 on all 15 fixtures."),
 14: ("Rendered", "What a user sees", "The HTML of both programs rendered in a browser. Identical output, not just similar-looking output."),
 15: ("Fuzz", "3,000 generated documents, and a planted bug", "Identical on every one. To show the fuzzer can fail, one regex flag was removed from a copy of the port. It found 44 differences in 300 documents."),
 16: ("Limits", "The one non-deterministic path", "The original builds <code class='mono'>System.Random</code> for email obfuscation. The spec shows it, so the port is random too, and the comparison decodes entities."),
 17: ("Baseline", "What general Markdown knowledge produces instead", "Two mature generic libraries against the original's output. This is a proxy, not an LLM run. It shows how far <em>Markdown</em> the concept is from <em>this program</em>."),
 18: ("Result", "Scoreboard, and what this does not prove", "Numbers come from the frames, not from this page."),
}

A = "assets/binary-demo/code/"
DEC = f'<a href="{A}decompiled/MarkdownSharp.recovered.txt">Recovered pseudo-code, all types</a>'
PORT = (f'<a href="{A}python-port/mdport/markdown.py">markdown.py</a> · <a href="{A}python-port/mdport/_literals.py">_literals.py</a> · '
        f'<a href="{A}python-port/mdport/__main__.py">__main__.py</a>')
LINKS = {6: DEC, 7: DEC, 8: DEC, 9: DEC + " · Python rewrite: " + PORT, 10: "Python rewrite: " + PORT}


def slide(e):
    n = e["step"]; kind, head, cap = CAP[n]
    links = f'<p class="codelinks mono">{LINKS[n]}</p>' if n in LINKS else ""
    imgs = "".join(f'<img src="assets/binary-demo/{i}" alt="{html.escape(e["title"])}" loading="lazy">' for i in e["images"])
    return f'''<section class="slide" id="s@@">
  <div class="stage-num mono">§ {n:02d} / {kind.lower()}</div>
  <h2 style="font-size:clamp(1.5rem,3vw,2.3rem); max-width:30ch;">{head}</h2>
  <p class="lede" style="max-width:62rem;">{cap}</p>
  {links}
  <div class="shot">{imgs}</div>
</section>'''

HELP = '''<section class="slide" id="s@@">
  <div class="stage-num mono">§ 17 / part 2</div>
  <div class="eyebrow">How EKOS helps in this demo</div>
  <h2 style="font-size:clamp(1.6rem,3.2vw,2.5rem); max-width:30ch;">EKOS does not write the port. It makes the input trustworthy and the output checkable.</h2>
  <table class="ttable" style="max-width:74rem; text-align:left;">
    <thead><tr><th>Step</th><th>What EKOS did here</th><th>What you would do without it</th></tr></thead>
    <tbody>
      <tr><td class="tool">Read the binary</td><td class="desc">In-process CIL decoder, control-flow graphs, structuring: 101 method bodies, 99 as full statements. No SDK, nothing executed.</td><td class="desc">A PE file is not text an LLM can read. You need a decompiler first, and its output has no per-line evidence.</td></tr>
      <tr><td class="tool">Constants</td><td class="desc">37 regex and format literals, 12,009 characters, delivered verbatim and copied into the port by a script.</td><td class="desc">Recall or retype them. One wrong character in a verbose regex changes behaviour and nothing complains.</td></tr>
      <tr><td class="tool">Evidence</td><td class="desc">Every statement cites its IL offset; every fact is a ledger object with provenance.</td><td class="desc">Claims about what a method does cannot be traced back to anything.</td></tr>
      <tr><td class="tool">Gaps</td><td class="desc">2 methods marked <code class="mono">control_flow</code> / partial, with "do not fill the gaps by guessing".</td><td class="desc">A model fills a gap with plausible code, and nobody sees where.</td></tr>
      <tr><td class="tool">Order</td><td class="desc">Migration order, callees first, for all 78 methods of the type.</td><td class="desc">Ad hoc, and easy to port a caller before what it depends on.</td></tr>
      <tr><td class="tool">Verify</td><td class="desc">Static check, sandboxed record of the original, replay against the port, and a fuzz that caught a planted bug.</td><td class="desc">"Looks right" review of code that was never compared to the running original.</td></tr>
    </tbody>
  </table>
</section>

<section class="slide" id="s@@">
  <div class="stage-num mono">§ 17 / part 2</div>
  <div class="eyebrow">Why this is harder for a plain LLM</div>
  <h2 style="font-size:clamp(1.6rem,3.2vw,2.5rem); max-width:30ch;">Knowing Markdown is not the same as knowing this program.</h2>
  <ul class="facts" style="max-width:74rem;">
    <li><span class="k">Behaviour, not concept</span><span class="v">MarkdownSharp has its own quirks, for example a list that follows another gets nested inside the previous item. Two mature generic libraries match the original on 1 of 15 fixtures byte for byte, and 10 and 8 of 15 ignoring whitespace. The port written from the spec matches 15 of 15.</span></li>
    <li><span class="k">Dialect traps</span><span class="v">.NET and Python regexes differ: <code class="mono">\z</code> vs <code class="mono">\Z</code>, variable-width lookbehind, atomic groups need Python 3.11+, options are integers in the IL. None of these show in a plain read of the code. Tests found them.</span></li>
    <li><span class="k">Silent guessing</span><span class="v">Where the recovered structure is incomplete EKOS says so per method. A model asked to "port this" gives no such signal.</span></li>
    <li><span class="k">No ground truth</span><span class="v">Without running the original there is nothing to compare against. Here the original ran in a sandbox, and a one-flag bug that a review would likely miss showed up as 44 differing documents in 300.</span></li>
  </ul>
  <div class="fineprint" style="max-width:74rem;">Honest limits: no head-to-head against an LLM was run, and the rewrite here was itself written by an LLM using EKOS's spec, check and harness. The generic libraries are a proxy. A plain LLM given a general decompiler such as ILSpy would do better than that proxy; it would still lack per-line IL evidence, fidelity flags, callee ordering, and the run-against-the-original loop.</div>
</section>'''
blocks = [slide(e) for e in M]
pos = next(i for i, e in enumerate(M) if e["step"] == 17)
blocks[pos:pos] = HELP.split("\n\n")
slides = "\n\n".join(blocks)
total = len(blocks) + 3
CUR = ' class="current"'
rail = "".join('<a href="#s%d"%s></a>' % (i, CUR if i == 1 else "") for i in range(1, total + 1))

HTML = f'''<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Compiled App to Python — EKOS</title>
<meta name="description" content="EKOS recovers the logic of a compiled .NET library with no source, a Python rewrite is checked against the running original, and both give identical output. Every step has a screenshot.">
<link rel="stylesheet" href="../assets/theme.css">
<link rel="icon" href="../assets/favicon.svg" type="image/svg+xml">
<style>
.codelinks{{font-size:.85rem;margin:.4rem 0 0}} .codelinks a{{color:var(--accent-a)}}
.shot{{margin-top:1.6rem;display:flex;flex-direction:column;gap:1rem}}
.shot img{{width:100%;max-width:1180px;height:auto;border-radius:12px;border:1px solid var(--rule);box-shadow:var(--glow-shadow)}}
</style>
</head>
<body>

<nav class="rail" aria-hidden="true">{rail}</nav>

<div class="deck">

<section class="slide hero" id="s1">
  <div class="eyebrow">EKOS — Enterprise Knowledge Operating System</div>
  <h1>A compiled app, no source.<br><span class="arrow-word">Same results</span> in Python.</h1>
  <p class="lede">EKOS reads a compiled .NET library it has never seen the source of, recovers what every method does,
  and a Python rewrite is written from that. The rewrite is then checked against the running original
  and produces byte-identical output. Below is every step, with the real screen for each.</p>
  <div class="hero-diagram">
    <span class="chip">MarkdownSharp.dll</span><span class="sep mono">→</span>
    <span class="chip hot">ekos recover</span><span class="sep mono">→</span>
    <span class="chip">statement-level spec</span><span class="sep mono">→</span>
    <span class="chip">Python rewrite</span><span class="sep mono">→</span>
    <span class="chip hot">characterize + fuzz</span>
  </div>
  <div class="byline"><span class="dot"></span> RFC 0148 · RFC 0150 · RFC 0149 (private extension)</div>
</section>

<section class="slide" id="s2">
  <div class="stage-num mono">§ 00 / why this app</div>
  <h2 style="font-size:clamp(1.9rem,3.8vw,2.9rem); max-width:24ch;">Open source, real, deterministic, and small enough to check completely.</h2>
  <div class="two-col">
    <p class="lede" style="margin-top:0;">MarkdownSharp is the Markdown engine that powered Stack Overflow. It is a text-to-HTML function with no
    files, network or database, so "same results" means identical bytes. It leans on regular expressions,
    which is where a port most easily goes wrong.</p>
    <ul class="facts">
      <li><span class="k">Input</span><span class="v">The compiled net40 assembly from NuGet, MIT licensed.</span></li>
      <li><span class="k">Recovered</span><span class="v">99 of 101 method bodies as fully structured statements, 98%.</span></li>
      <li><span class="k">Scope</span><span class="v">.NET only today. A Java jar would stop at structural facts; see the last slide.</span></li>
    </ul>
  </div>
</section>

{slides}

<section class="slide cta" id="s{total}">
  <div class="eyebrow">Reproduce it</div>
  <h2>Every screenshot comes from one script.</h2>
  <div class="codeblock">
    <div class="tag">terminal</div>
<pre style="margin:0;"><span class="c"># runs all 18 stages, real commands, writes frames/NN-*.txt</span>
$ demo/binary-demo/run_demo.sh

<span class="c"># renders each frame to a PNG (and the browser comparison)</span>
$ python3 demo/binary-demo/capture.py

<span class="c"># later: string the frames into a GIF or video</span>
$ demo/binary-demo/tools/make_gif.sh 3</pre>
  </div>
  <p class="mono" style="margin-top:1.4rem;font-size:.9rem;">The code: {DEC} · Python rewrite: {PORT}</p>
  <div class="fineprint">Not proven here: behaviour on the .NET Framework itself (the original ran on wine-mono); inputs outside the fuzzer's grammar; the app.config constructor (not ported). The decompiler is a private RFC 0149 extension; the screenshots show its output.</div>
</section>

</div>

<script src="../assets/rail.js"></script>
</body>
</html>
'''
import re
n = iter(range(3, 999))
HTML = re.sub(r'id="s@@"', lambda m: f'id="s{next(n)}"', HTML)
HTML = HTML.replace('id="s{total}"', f'id="s{total}"')
OUT.write_text(HTML)
print("wrote", OUT, f"({total} slides)")
