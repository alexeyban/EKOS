"""Tests for scripts/wiki-snapshot.py (RFC 0221). Run: python3 -m unittest discover scripts/tests"""

import importlib.util
import io
import json
import shutil
import sys
import tempfile
import unittest
import urllib.error
from pathlib import Path

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location(
    "wiki_snapshot", HERE.parent / "wiki-snapshot.py"
)
ws = importlib.util.module_from_spec(spec)
sys.modules["wiki_snapshot"] = ws  # dataclasses look their module up
spec.loader.exec_module(ws)

FIXTURE = HERE / "fixtures" / "wiki-export.xml"


@unittest.skipIf(shutil.which("pandoc") is None, "pandoc is not installed")
class ExportSnapshot(unittest.TestCase):
    def run_snapshot(self, *extra):
        out = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, out)
        code = ws.main(["--xml", str(FIXTURE), "--out", str(out), *extra])
        self.assertEqual(code, 0)
        return out

    def test_each_page_gets_markdown_with_provenance_and_redirects_are_skipped(self):
        out = self.run_snapshot("--licence", "CC BY-SA 4.0")
        page = (out / "main" / "Sales Order Process.md").read_text()
        self.assertTrue(page.startswith('---\ntitle: "Sales Order Process"\n'))
        self.assertIn(
            'source_url: "https://wiki.example.org/en/Sales_Order_Process"', page
        )
        self.assertIn('source_revision: "4242"', page)
        self.assertIn('revision_timestamp: "2024-05-06T07:08:09Z"', page)
        self.assertIn('licence: "CC BY-SA 4.0"', page)
        self.assertIn("## Completing", page)
        self.assertIn("**C_Order.DocStatus**", page)
        self.assertIn(
            "[IDEMPIERE-6203](https://wiki.example.org/en/IDEMPIERE-6203)", page
        )
        self.assertNotIn("old text", page)
        self.assertTrue((out / "Category" / "Workflows_Order.md").exists())
        listing = json.loads((out / "SNAPSHOT.json").read_text())
        self.assertEqual([p["revision"] for p in listing["pages"]], ["102", "4242"])
        self.assertEqual(listing["redirects_skipped"], 1)

    def test_the_same_export_gives_the_same_files(self):
        a, b = self.run_snapshot(), self.run_snapshot()
        for f in sorted(a.rglob("*.md")):
            self.assertEqual(f.read_bytes(), (b / f.relative_to(a)).read_bytes())

    def test_without_a_licence_it_says_unknown(self):
        out = self.run_snapshot()
        self.assertIn(
            'licence: "unknown"', (out / "main" / "Sales Order Process.md").read_text()
        )


class ApiSnapshot(unittest.TestCase):
    def test_a_403_stops_the_run_without_a_retry(self):
        calls = []

        def opener(req, timeout):
            calls.append(req.full_url)
            raise urllib.error.HTTPError(
                req.full_url, 403, "Forbidden", {}, io.BytesIO(b"")
            )

        out = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, out)
        if shutil.which("pandoc") is None:
            self.skipTest("pandoc is not installed")
        code = ws.main(
            ["--api", "https://w.example/api.php", "--page", "X", "--out", str(out)],
            opener,
        )
        self.assertEqual(code, 3)
        self.assertEqual(len(calls), 1)
        self.assertFalse((out / "SNAPSHOT.json").exists())

    def test_a_challenge_page_is_blocked_not_parsed(self):
        class Resp(io.BytesIO):
            headers = {"Content-Type": "text/html; charset=UTF-8"}

            def __enter__(self):
                return self

            def __exit__(self, *a):
                return False

        api = ws.Api(
            "https://w.example/api.php",
            0,
            lambda req, timeout: Resp(b"Just a moment..."),
        )
        with self.assertRaises(ws.Blocked):
            api.get(action="query")

    def test_requests_carry_no_personal_header_and_are_spaced(self):
        self.assertEqual(ws.USER_AGENT, f"ekos-wiki-snapshot/{ws.VERSION}")
        self.assertGreaterEqual(ws.Api("u", 0).delay, 1.0)


if __name__ == "__main__":
    unittest.main()
