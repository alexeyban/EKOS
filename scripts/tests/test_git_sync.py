"""Tests for scripts/ekos-git-sync.py (RFC 0232). Real git, local repositories, no network."""

import base64
import importlib.util
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("git_sync", HERE.parent / "ekos-git-sync.py")
gs = importlib.util.module_from_spec(spec)
sys.modules["git_sync"] = gs
spec.loader.exec_module(gs)

ENV = {
    "GIT_AUTHOR_NAME": "t",
    "GIT_AUTHOR_EMAIL": "t@example.com",
    "GIT_COMMITTER_NAME": "t",
    "GIT_COMMITTER_EMAIL": "t@example.com",
}


def git(*args, cwd):
    import os

    return subprocess.run(
        ["git", *args],
        cwd=cwd,
        check=True,
        capture_output=True,
        text=True,
        env={**os.environ, **ENV},
    ).stdout.strip()


@unittest.skipUnless(shutil.which("git"), "git is not installed")
class SyncTest(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp())
        self.origin = self.tmp / "origin"
        self.origin.mkdir()
        git("init", "-q", "-b", "main", cwd=self.origin)
        (self.origin / "a.sql").write_text("CREATE TABLE a (id int);\n")
        git("add", ".", cwd=self.origin)
        git("commit", "-qm", "one", cwd=self.origin)
        self.dest = self.tmp / "sources"

    def tearDown(self):
        shutil.rmtree(self.tmp)

    def env(self):
        return gs.git_env(
            None, "x", {"PATH": __import__("os").environ["PATH"], "HOME": str(self.tmp)}
        )

    def test_clone_then_follow_the_branch_and_drop_local_edits(self):
        repo = gs.Repo("shop", str(self.origin), None)
        first = gs.sync(repo, self.dest, self.env())
        self.assertEqual(first, git("rev-parse", "HEAD", cwd=self.origin))
        self.assertTrue((self.dest / "shop" / "a.sql").exists())

        (self.origin / "b.sql").write_text("CREATE TABLE b (id int);\n")
        git("add", ".", cwd=self.origin)
        git("commit", "-qm", "two", cwd=self.origin)
        (self.dest / "shop" / "a.sql").write_text("edited by hand")
        (self.dest / "shop" / "stray.txt").write_text("x")

        second = gs.sync(repo, self.dest, self.env())
        self.assertEqual(second, git("rev-parse", "HEAD", cwd=self.origin))
        self.assertEqual((self.dest / "shop" / "a.sql").read_text(), "CREATE TABLE a (id int);\n")
        self.assertTrue((self.dest / "shop" / "b.sql").exists())
        self.assertFalse((self.dest / "shop" / "stray.txt").exists())

    def test_a_named_branch_is_followed(self):
        git("checkout", "-qb", "release", cwd=self.origin)
        (self.origin / "r.sql").write_text("-- release\n")
        git("add", ".", cwd=self.origin)
        git("commit", "-qm", "rel", cwd=self.origin)
        git("checkout", "-q", "main", cwd=self.origin)
        repo = gs.Repo("shop", str(self.origin), "release")
        gs.sync(repo, self.dest, self.env())
        self.assertTrue((self.dest / "shop" / "r.sql").exists())
        gs.sync(repo, self.dest, self.env())
        self.assertTrue((self.dest / "shop" / "r.sql").exists())

    def test_a_directory_that_is_not_a_checkout_is_left_alone(self):
        (self.dest / "shop").mkdir(parents=True)
        (self.dest / "shop" / "mine.txt").write_text("keep")
        with self.assertRaises(RuntimeError):
            gs.sync(gs.Repo("shop", str(self.origin), None), self.dest, self.env())
        self.assertEqual((self.dest / "shop" / "mine.txt").read_text(), "keep")

    def test_one_failing_repository_does_not_stop_the_others(self):
        repos = [
            gs.Repo("missing", str(self.tmp / "nope"), None),
            gs.Repo("shop", str(self.origin), None),
        ]
        self.assertFalse(gs.round_(repos, self.dest, self.env(), 0))
        self.assertTrue((self.dest / "shop" / "a.sql").exists())

    def test_main_runs_one_round_with_interval_zero(self):
        code = gs.main(
            {
                "PATH": __import__("os").environ["PATH"],
                "HOME": str(self.tmp),
                "EKOS_GIT_SYNC_REPOS": f"shop={self.origin}",
                "EKOS_GIT_SYNC_DEST": str(self.dest),
                "EKOS_GIT_SYNC_INTERVAL": "0",
            }
        )
        self.assertEqual(code, 0)
        self.assertTrue((self.dest / "shop" / "a.sql").exists())


class ConfigTest(unittest.TestCase):
    def test_repos_parse_with_branches_lines_and_commas(self):
        repos = gs.parse_repos(
            "shop=https://github.com/o/shop.git#main, erp=https://dev.azure.com/o/p/_git/erp\n"
            "# a comment\n"
        )
        self.assertEqual(
            repos,
            [
                gs.Repo("shop", "https://github.com/o/shop.git", "main"),
                gs.Repo("erp", "https://dev.azure.com/o/p/_git/erp", None),
            ],
        )

    def test_bad_configuration_is_refused(self):
        for spec in [
            "",
            "no-equals-sign",
            "../escape=https://github.com/o/r",
            "a=https://github.com/o/r,a=https://github.com/o/s",
            "a=git@github.com:o/r.git",
            "a=https://user:token@github.com/o/r",
        ]:
            with self.assertRaises(gs.ConfigError, msg=spec):
                gs.parse_repos(spec)

    def test_the_token_goes_in_a_header_variable_and_nowhere_else(self):
        env = gs.git_env(
            "s3cret", "x-access-token", {"PATH": "/bin", "EKOS_GIT_SYNC_TOKEN": "s3cret"}
        )
        self.assertNotIn("EKOS_GIT_SYNC_TOKEN", env)
        self.assertEqual(env["GIT_CONFIG_KEY_0"], "http.extraHeader")
        expected = base64.b64encode(b"x-access-token:s3cret").decode()
        self.assertEqual(env["GIT_CONFIG_VALUE_0"], f"Authorization: Basic {expected}")
        self.assertEqual(env["GIT_TERMINAL_PROMPT"], "0")
        self.assertNotIn("GIT_CONFIG_COUNT", gs.git_env(None, "x", {"PATH": "/bin"}))


if __name__ == "__main__":
    unittest.main()
