"""Tests for scripts/ekos-entrypoint.sh (RFC 0232): secret files become variables."""

import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parent.parent / "ekos-entrypoint.sh"


@unittest.skipUnless(shutil.which("sh"), "no POSIX shell")
class EntrypointTest(unittest.TestCase):
    def run_with(self, files, env):
        d = Path(tempfile.mkdtemp())
        try:
            for name, value in files.items():
                (d / name).write_text(value)
            base = {"PATH": os.environ["PATH"], "EKOS_SECRETS_DIR": str(d), **env}
            out = subprocess.run(
                ["sh", str(SCRIPT), "env"], env=base, capture_output=True, text=True, check=True
            )
            return dict(line.split("=", 1) for line in out.stdout.splitlines() if "=" in line)
        finally:
            shutil.rmtree(d)

    def test_files_become_variables_and_the_platform_wins(self):
        got = self.run_with(
            {
                "OPENAI_API_KEY": "sk-file\n",
                "EKOS_CONSOLE_SESSION_SECRET": "from-file",
                "TRICKY": "it's $(id) `x`",
                "bad-name": "x",
                "1X": "x",
            },
            {"EKOS_CONSOLE_SESSION_SECRET": "from-platform"},
        )
        self.assertEqual(got["OPENAI_API_KEY"], "sk-file")
        self.assertEqual(got["EKOS_CONSOLE_SESSION_SECRET"], "from-platform")
        self.assertEqual(got["TRICKY"], "it's $(id) `x`")
        self.assertNotIn("bad-name", got)
        self.assertNotIn("1X", got)

    def test_without_a_directory_it_only_runs_the_command(self):
        out = subprocess.run(
            ["sh", str(SCRIPT), "echo", "ran"],
            env={"PATH": os.environ["PATH"]},
            capture_output=True,
            text=True,
            check=True,
        )
        self.assertEqual(out.stdout.strip(), "ran")


if __name__ == "__main__":
    unittest.main()
