#!/usr/bin/env python3
"""MediaWiki pages -> Markdown files with provenance front matter (RFC 0221).

The output is an ordinary directory for an EKOS `[[sources]]` entry: one Markdown file per page,
opening with `title`, `source_url`, `source_revision`, `revision_timestamp`, `licence` and
`generator`, plus a `SNAPSHOT.json` listing every page written. The same revision always gives
the same file (no retrieval time is recorded).

Two inputs:

  wiki-snapshot.py --xml export.xml --out DIR [--base-url URL] [--licence TEXT]
      A `Special:Export` file saved in a browser. Use this for a wiki behind bot protection.

  wiki-snapshot.py --api https://wiki.example.org/api.php (--page TITLE ... | --category NAME) --out DIR
      The MediaWiki API, one request at a time, at least --delay seconds apart. A 403 or a
      challenge page stops the run: it is never retried around.

Needs `pandoc` on PATH (MediaWiki -> GitHub-flavoured Markdown). A page pandoc cannot convert is
kept as its raw wikitext in a fenced block, and counted.
"""

from __future__ import annotations

import argparse
import json
import re
import shutil
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
import xml.etree.ElementTree as ET
from dataclasses import dataclass
from pathlib import Path

VERSION = "0.1"
USER_AGENT = f"ekos-wiki-snapshot/{VERSION}"
MIN_DELAY_SECONDS = 1.0


class Blocked(Exception):
    """The wiki refused the request (bot protection, rate limit or no access)."""


@dataclass
class Page:
    title: str
    namespace: str
    revision: str
    timestamp: str
    text: str
    redirect: bool = False


@dataclass
class Stats:
    written: int = 0
    redirects: int = 0
    unconverted: int = 0


# ── Input: a Special:Export file ─────────────────────────────────────────────


def read_export(path: Path) -> tuple[list[Page], str | None]:
    """The pages of an export (each page's last revision) and the wiki's article base URL."""
    root = ET.parse(path).getroot()
    ns = root.tag[: root.tag.index("}") + 1] if root.tag.startswith("{") else ""
    names = {el.get("key"): (el.text or "") for el in root.iter(f"{ns}namespace")}
    base = root.findtext(f"{ns}siteinfo/{ns}base")
    pages = []
    for el in root.iter(f"{ns}page"):
        revisions = el.findall(f"{ns}revision")
        if not revisions:
            continue
        rev = revisions[-1]
        pages.append(
            Page(
                title=el.findtext(f"{ns}title", ""),
                namespace=names.get(el.findtext(f"{ns}ns", "0"), ""),
                revision=rev.findtext(f"{ns}id", ""),
                timestamp=rev.findtext(f"{ns}timestamp", ""),
                text=rev.findtext(f"{ns}text", ""),
                redirect=el.find(f"{ns}redirect") is not None,
            )
        )
    return pages, article_base(base)


def article_base(main_page_url: str | None) -> str | None:
    """`https://wiki.x.org/en/Main_Page` -> `https://wiki.x.org/en/`."""
    if not main_page_url:
        return None
    return main_page_url[: main_page_url.rfind("/") + 1]


# ── Input: the MediaWiki API ─────────────────────────────────────────────────


class Api:
    def __init__(self, url: str, delay: float, opener=urllib.request.urlopen):
        self.url = url
        self.delay = max(delay, MIN_DELAY_SECONDS)
        self.opener = opener
        self.last = 0.0

    def get(self, **params) -> dict:
        wait = self.last + self.delay - time.monotonic()
        if wait > 0:
            time.sleep(wait)
        params.update(format="json", formatversion="2", maxlag="5")
        req = urllib.request.Request(
            f"{self.url}?{urllib.parse.urlencode(params)}",
            headers={"User-Agent": USER_AGENT},
        )
        try:
            with self.opener(req, timeout=60) as resp:
                kind = resp.headers.get("Content-Type", "")
                body = resp.read()
        except urllib.error.HTTPError as e:
            raise Blocked(f"HTTP {e.code} from {self.url}") from e
        finally:
            self.last = time.monotonic()
        if "json" not in kind:
            raise Blocked(f"{self.url} answered {kind or 'no content type'}, not JSON")
        data = json.loads(body)
        if "error" in data:
            raise Blocked(f"{self.url}: {data['error'].get('info', data['error'])}")
        return data

    def site(self) -> tuple[str | None, str | None, dict[int, str]]:
        """(article base URL, licence text, namespace names)."""
        q = self.get(
            action="query", meta="siteinfo", siprop="general|rightsinfo|namespaces"
        )["query"]
        names = {int(k): v.get("name", "") for k, v in q.get("namespaces", {}).items()}
        rights = q.get("rightsinfo", {})
        licence = rights.get("text") or rights.get("url") or None
        return article_base(q.get("general", {}).get("base")), licence, names

    def category(self, name: str) -> list[str]:
        titles, cont = [], {}
        while True:
            q = self.get(
                action="query",
                list="categorymembers",
                cmtitle=name if name.startswith("Category:") else f"Category:{name}",
                cmlimit="200",
                cmtype="page",
                **cont,
            )
            titles += [m["title"] for m in q["query"]["categorymembers"]]
            if "continue" not in q:
                return titles
            cont = {"cmcontinue": q["continue"]["cmcontinue"]}

    def pages(self, titles: list[str], names: dict[int, str]) -> list[Page]:
        out = []
        for i in range(0, len(titles), 20):
            q = self.get(
                action="query",
                prop="revisions",
                rvprop="ids|timestamp|content",
                rvslots="main",
                titles="|".join(titles[i : i + 20]),
            )
            for p in q["query"].get("pages", []):
                if p.get("missing") or not p.get("revisions"):
                    continue
                rev = p["revisions"][0]
                text = rev.get("slots", {}).get("main", {}).get("content", "")
                out.append(
                    Page(
                        title=p["title"],
                        namespace=names.get(p.get("ns", 0), ""),
                        revision=str(rev["revid"]),
                        timestamp=rev["timestamp"],
                        text=text,
                        redirect=bool(p.get("redirect")),
                    )
                )
        return out


# ── Output ───────────────────────────────────────────────────────────────────

REDIRECT = re.compile(r"^\s*#REDIRECT", re.IGNORECASE)


def to_markdown(wikitext: str) -> str | None:
    """GitHub-flavoured Markdown, or None when pandoc fails."""
    try:
        done = subprocess.run(
            ["pandoc", "-f", "mediawiki", "-t", "gfm", "--wrap=none"],
            input=wikitext,
            capture_output=True,
            text=True,
            timeout=120,
            check=False,
        )
    except subprocess.TimeoutExpired:
        return None
    return done.stdout if done.returncode == 0 else None


INTRAWORD_UNDERSCORE = re.compile(r"(?<=\w)\\_(?=\w)")
WIKILINK = re.compile(r'\]\(([^)"]*?) "wikilink"\)')


def tidy(body: str, base: str | None) -> str:
    """Pandoc's GFM, readable by the analyzers: `C_Order` rather than `C\\_Order` (GFM does not
    emphasise inside a word), and links to other wiki pages as absolute URLs."""
    body = INTRAWORD_UNDERSCORE.sub("_", body)
    return WIKILINK.sub(
        lambda m: f"]({page_url(base, urllib.parse.unquote(m.group(1)))})", body
    )


def file_name(page: Page) -> str:
    """`<namespace or main>/<title>.md`, the title's subpage slashes and unsafe characters replaced."""
    title = page.title
    if page.namespace and title.startswith(page.namespace + ":"):
        title = title[len(page.namespace) + 1 :]
    safe = re.sub(r'[\\/:*?"<>|\x00-\x1f]', "_", title).strip(". ") or "_"
    return f"{page.namespace or 'main'}/{safe}.md"


def page_url(base: str | None, title: str) -> str:
    if not base:
        return ""
    return base + urllib.parse.quote(title.replace(" ", "_"), safe="/:()_,-.'")


def render(page: Page, base: str | None, licence: str, body: str) -> str:
    front = {
        "title": page.title,
        "source_url": page_url(base, page.title),
        "source_revision": page.revision,
        "revision_timestamp": page.timestamp,
        "licence": licence,
        "generator": f"ekos-wiki-snapshot {VERSION}",
    }
    # JSON strings are valid YAML double-quoted scalars.
    lines = [f"{k}: {json.dumps(v, ensure_ascii=False)}" for k, v in front.items()]
    return (
        "---\n"
        + "\n".join(lines)
        + f"\n---\n\n# {page.title}\n\n"
        + body.rstrip()
        + "\n"
    )


def write(pages: list[Page], out: Path, base: str | None, licence: str) -> Stats:
    stats = Stats()
    listed = []
    for page in sorted(pages, key=lambda p: p.title):
        if page.redirect or REDIRECT.match(page.text):
            stats.redirects += 1
            continue
        body = to_markdown(page.text)
        if body is not None:
            body = tidy(body, base)
        else:
            stats.unconverted += 1
            body = "```mediawiki\n" + page.text.replace("```", "` ` `") + "\n```"
        rel = file_name(page)
        target = out / rel
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(render(page, base, licence, body), encoding="utf-8")
        listed.append({"title": page.title, "file": rel, "revision": page.revision})
        stats.written += 1
    (out / "SNAPSHOT.json").write_text(
        json.dumps(
            {
                "generator": f"ekos-wiki-snapshot {VERSION}",
                "base_url": base,
                "licence": licence,
                "pages": listed,
                "redirects_skipped": stats.redirects,
                "kept_as_wikitext": stats.unconverted,
            },
            indent=2,
            ensure_ascii=False,
        )
        + "\n",
        encoding="utf-8",
    )
    return stats


def main(argv: list[str] | None = None, opener=urllib.request.urlopen) -> int:
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    src = ap.add_mutually_exclusive_group(required=True)
    src.add_argument("--xml", type=Path, help="a Special:Export file")
    src.add_argument("--api", help="the wiki's api.php URL")
    ap.add_argument(
        "--page",
        action="append",
        default=[],
        help="a page title (API mode, repeatable)",
    )
    ap.add_argument(
        "--category",
        action="append",
        default=[],
        help="every page of a category (API mode)",
    )
    ap.add_argument("--out", type=Path, required=True)
    ap.add_argument(
        "--base-url", help="article URL prefix, e.g. https://wiki.idempiere.org/en/"
    )
    ap.add_argument("--licence", help="the content licence, recorded on every page")
    ap.add_argument(
        "--delay",
        type=float,
        default=MIN_DELAY_SECONDS,
        help="seconds between API requests (min 1)",
    )
    args = ap.parse_args(argv)

    if shutil.which("pandoc") is None:
        print("wiki-snapshot: pandoc is not on PATH", file=sys.stderr)
        return 2
    if args.xml:
        pages, base = read_export(args.xml)
        licence = args.licence
    else:
        if not (args.page or args.category):
            ap.error("--api needs --page or --category")
        api = Api(args.api, args.delay, opener)
        try:
            base, site_licence, names = api.site()
            titles = list(args.page)
            for c in args.category:
                titles += api.category(c)
            pages = api.pages(sorted(set(titles)), names)
        except Blocked as e:
            print(
                f"wiki-snapshot: stopped — {e}. The wiki refuses scripted access; save the pages "
                "with Special:Export in a browser and run again with --xml.",
                file=sys.stderr,
            )
            return 3
        licence = args.licence or site_licence
    stats = write(pages, args.out, args.base_url or base, licence or "unknown")
    print(
        f"wiki-snapshot: {stats.written} page(s) written to {args.out}, "
        f"{stats.redirects} redirect(s) skipped, {stats.unconverted} kept as wikitext"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
