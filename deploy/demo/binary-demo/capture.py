#!/usr/bin/env python3
"""Render every frames/NN-name.txt as a terminal-window PNG, plus a rendered-HTML comparison for step 14.

Fixed viewport, font and theme, so the PNGs are consistent frames: tools/make_gif.sh strings them into a GIF/video.
Output: docs/presentations/assets/binary-demo/NN-name.png and manifest.json.
"""
import html, json, pathlib, re, sys
from playwright.sync_api import sync_playwright

HERE = pathlib.Path(__file__).resolve().parent
FR = HERE / "frames"
OUT = HERE.parent.parent / "docs/presentations/assets/binary-demo"
OUT.mkdir(parents=True, exist_ok=True)
WIDTH = 1440

SGR = {"1;32": "g b", "1;35": "m b", "1": "b", "90": "d", "0": ""}
KEYWORDS = [
    (r"\b(PASS|identical|IDENTICAL|blocked|caught|ready|matches)\b", "ok"),
    (r"\b(FAIL|DIFFERENT|different: [1-9]\d*)\b", "bad"),
    (r"\b(GAP|differs|control_flow|partial|missing)\b", "warn"),
    (r"(//\s*IL_[0-9A-F]+)", "dim"),
]

CSS = """
*{box-sizing:border-box} body{margin:0;background:#0b0a12;padding:28px;font-family:'DejaVu Sans Mono',monospace}
.window{width:%dpx;background:#100d1c;border:1px solid rgba(255,255,255,.12);border-radius:12px;overflow:hidden;
  box-shadow:0 20px 60px -20px rgba(153,69,255,.45)}
.bar{display:flex;align-items:center;gap:8px;padding:11px 16px;background:#1a1530;border-bottom:1px solid rgba(255,255,255,.08);
  color:#a7a2c4;font-size:14px}
.dot{width:12px;height:12px;border-radius:50%%} .r{background:#ff5f56}.y{background:#ffbd2e}.gr{background:#27c93f}
.bar .step{margin-left:14px;color:#c9a3ff;font-weight:700}.bar .ttl{color:#e9e4f7}
pre{margin:0;padding:18px 22px 22px;color:#e9e4f7;font-size:15px;line-height:1.42;white-space:pre-wrap;word-break:break-word}
.g{color:#4be3ac}.m{color:#c9a3ff}.b{font-weight:700}.d{color:#726c94}.ok{color:#4be3ac;font-weight:700}
.bad{color:#ff5c7a;font-weight:700}.warn{color:#ffbd2e}.dim{color:#726c94}
""" % WIDTH


def ansi_to_html(line):
    parts, out, cur = re.split(r"(\x1b\[[0-9;]*m)", line), [], ""
    for tok in parts:
        m = re.fullmatch(r"\x1b\[([0-9;]*)m", tok)
        if m:
            if cur:
                out.append("</span>")
            cur = SGR.get(m.group(1) or "0", "")
            if cur:
                out.append(f'<span class="{cur}">')
        else:
            t = html.escape(tok)
            if not cur:
                for pat, cls in KEYWORDS:
                    t = re.sub(pat, lambda k, c=cls: f'<span class="{c}">{k.group(0)}</span>', t)
            out.append(t)
    if cur:
        out.append("</span>")
    return "".join(out)


def frame_page(step, total, title, body):
    return (f"<!doctype html><meta charset=utf-8><style>{CSS}</style><div class=window><div class=bar>"
            f"<i class='dot r'></i><i class='dot y'></i><i class='dot gr'></i>"
            f"<span class=step>step {step}/{total}</span><span class=ttl>{html.escape(title)}</span></div>"
            f"<pre>{body}</pre></div>")


def rendered_page(orig, port):
    card = lambda label, doc: (f"<div class=card><div class=cap>{label}</div><div class=doc>{doc}</div></div>")
    css = CSS + """
    .cmp{display:flex;gap:22px;width:%dpx;align-items:stretch} .card{flex:1;background:#fff;color:#171326;border-radius:10px;
    overflow:hidden;font-family:-apple-system,'DejaVu Sans',sans-serif} .cap{background:#1a1530;color:#c9a3ff;padding:10px 16px;
    font:700 14px 'DejaVu Sans Mono',monospace} .doc{padding:8px 24px 20px;font-size:16px;line-height:1.5}
    .doc h1{font-size:28px;border-bottom:1px solid #e3ddf5} .doc h2{font-size:21px} .doc pre{background:#f2effa;padding:10px;color:#171326;font-size:14px}
    .doc blockquote{border-left:4px solid #c9a3ff;margin:0;padding-left:14px;color:#5c5578}
    .verdict{width:%dpx;margin:18px 0 0;text-align:center;color:#4be3ac;font:700 20px 'DejaVu Sans Mono',monospace}
    """ % (WIDTH, WIDTH)
    return (f"<!doctype html><meta charset=utf-8><style>{css}</style><div id=all><div class=cmp>"
            + card("original — MarkdownSharp.dll (wine-mono)", orig) + card("rewrite — python -m mdport", port)
            + "</div><div class=verdict>the two HTML documents are byte-identical</div></div>")


def main():
    frames = sorted(FR.glob("[0-9][0-9]-*.txt"))
    total = len(frames)
    manifest = []
    with sync_playwright() as pw:
        b = pw.chromium.launch()
        pg = b.new_page(viewport={"width": WIDTH + 60, "height": 900}, device_scale_factor=1.5)
        for f in frames:
            lines = f.read_text(encoding="utf-8").splitlines()
            title = lines[0][3:] if lines and lines[0].startswith("@@ ") else f.stem
            body = "\n".join(ansi_to_html(l) for l in lines[1:])
            step = int(f.stem[:2])
            pg.set_content(frame_page(step, total, title, body))
            pg.locator(".window").screenshot(path=str(OUT / f"{f.stem}.png"))
            entry = {"step": step, "name": f.stem, "title": title, "images": [f"{f.stem}.png"]}
            if f.stem.startswith("14-"):
                pg.set_content(rendered_page((FR / "14-original.html").read_text(), (FR / "14-port.html").read_text()))
                pg.locator("#all").screenshot(path=str(OUT / "14-rendered-browser.png"))
                entry["images"].insert(0, "14-rendered-browser.png")
            manifest.append(entry)
            print("captured", f.stem)
        b.close()
    (OUT / "manifest.json").write_text(json.dumps(manifest, indent=1, ensure_ascii=False))
    print(f"{len(manifest)} frames -> {OUT}")


if __name__ == "__main__":
    sys.exit(main())
