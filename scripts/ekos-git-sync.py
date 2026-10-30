#!/usr/bin/env python3
"""ekos-git-sync — keep the repositories EKOS compiles up to date under /sources (RFC 0232).

In a cloud the console reads its sources from a volume it mounts read-only; nothing on that
machine checks repositories out by hand. This program runs beside the console (a sidecar
container, a second container in the same task or pod, or a systemd timer on a VM) with the
same volume mounted read-write, clones each configured repository once and then fetches and
resets it to its branch on an interval.

Configuration, all from the environment:

  EKOS_GIT_SYNC_REPOS     One repository per line or comma-separated: `name=url[#branch]`.
                          `name` is the directory under the destination; no `#branch` follows the
                          remote's default branch.
  EKOS_GIT_SYNC_DEST      Where the checkouts live (default /sources).
  EKOS_GIT_SYNC_INTERVAL  Seconds between rounds; 0 runs one round and exits (default 300).
  EKOS_GIT_SYNC_TOKEN     An access token for HTTPS remotes (GitHub, GitLab, Azure DevOps,
                          Bitbucket). Injected by the platform from its secret store.
  EKOS_GIT_SYNC_USERNAME  The user name sent with the token (default `x-access-token`; GitLab
                          takes `oauth2`, Azure DevOps any non-empty name).
  EKOS_GIT_SYNC_DEPTH     Clone depth; 0 (the default) is full history. The git observer, the
                          freshness check and `semantics` blame read history, so keep it full
                          unless a repository is too large.

The token is never written to disk and never part of a URL or an argument (where `ps` and
`.git/config` would show it). It reaches git as an `http.extraHeader` through `GIT_CONFIG_*`
variables of the git process only. Output names repositories, never URLs with credentials.

Exit code: 0 when every repository synced in the last round; 1 otherwise (with an interval the
program keeps going and logs the failure; one bad repository never stops the others).
"""

from __future__ import annotations

import base64
import dataclasses
import os
import re
import subprocess
import sys
import time
from pathlib import Path

_NAME = re.compile(r"[A-Za-z0-9][A-Za-z0-9._-]{0,99}")


@dataclasses.dataclass(frozen=True)
class Repo:
    name: str
    url: str
    branch: str | None


class ConfigError(ValueError):
    pass


def parse_repos(spec: str) -> list[Repo]:
    repos: list[Repo] = []
    seen: set[str] = set()
    for raw in re.split(r"[\n,]", spec):
        item = raw.strip()
        if not item or item.startswith("#"):
            continue
        if "=" not in item:
            raise ConfigError(f"{item!r}: expected name=url[#branch]")
        name, rest = (s.strip() for s in item.split("=", 1))
        if not _NAME.fullmatch(name) or name in (".", ".."):
            raise ConfigError(f"{name!r} is not a directory name")
        if name in seen:
            raise ConfigError(f"{name!r} is listed twice")
        url, _, branch = rest.partition("#")
        url = url.strip()
        if not (url.startswith("https://") or url.startswith("file://") or url.startswith("/")):
            raise ConfigError(f"{name}: only https:// remotes are supported")
        if re.match(r"https://[^/]*@", url):
            raise ConfigError(f"{name}: put the token in EKOS_GIT_SYNC_TOKEN, not in the URL")
        seen.add(name)
        repos.append(Repo(name, url, branch.strip() or None))
    if not repos:
        raise ConfigError("EKOS_GIT_SYNC_REPOS lists no repository")
    return repos


def git_env(token: str | None, username: str, base: dict[str, str] | None = None) -> dict[str, str]:
    """The environment of a git process: no prompts, and the token as a header if there is one."""
    env = dict(os.environ if base is None else base)
    env["GIT_TERMINAL_PROMPT"] = "0"
    env.pop("EKOS_GIT_SYNC_TOKEN", None)
    if token:
        basic = base64.b64encode(f"{username}:{token}".encode()).decode()
        env["GIT_CONFIG_COUNT"] = "1"
        env["GIT_CONFIG_KEY_0"] = "http.extraHeader"
        env["GIT_CONFIG_VALUE_0"] = f"Authorization: Basic {basic}"
    return env


def _git(args: list[str], env: dict[str, str], cwd: Path | None = None) -> str:
    out = subprocess.run(
        ["git", *args],
        cwd=cwd,
        env=env,
        capture_output=True,
        text=True,
        timeout=3600,
    )
    if out.returncode != 0:
        raise RuntimeError((out.stderr or out.stdout).strip().splitlines()[-1:] or ["git failed"])
    return out.stdout.strip()


def sync(repo: Repo, dest: Path, env: dict[str, str], depth: int = 0) -> str:
    """Clone `repo` into `dest/name` or bring it to its branch's tip. Returns the commit."""
    target = dest / repo.name
    shallow = ["--depth", str(depth)] if depth > 0 else []
    if not (target / ".git").exists():
        if target.exists() and any(target.iterdir()):
            raise RuntimeError(f"{target} exists and is not a git checkout; not touching it")
        branch = ["--branch", repo.branch] if repo.branch else []
        _git(["clone", "--quiet", *shallow, *branch, "--", repo.url, str(target)], env)
    else:
        # The remote's URL may have been changed in the configuration.
        _git(["remote", "set-url", "origin", repo.url], env, cwd=target)
        ref = repo.branch or _default_branch(target, env)
        _git(["fetch", "--quiet", "--prune", *shallow, "origin", ref], env, cwd=target)
        _git(["checkout", "--quiet", "-B", ref, "FETCH_HEAD"], env, cwd=target)
        _git(["reset", "--quiet", "--hard", "FETCH_HEAD"], env, cwd=target)
        # Files EKOS never wrote (it never writes here) but a manual edit could have left.
        _git(["clean", "-fdq"], env, cwd=target)
    return _git(["rev-parse", "HEAD"], env, cwd=target)


def _default_branch(target: Path, env: dict[str, str]) -> str:
    try:
        _git(["remote", "set-head", "origin", "--auto"], env, cwd=target)
        head = _git(["symbolic-ref", "--short", "refs/remotes/origin/HEAD"], env, cwd=target)
        return head.split("/", 1)[1]
    except RuntimeError:
        return _git(["rev-parse", "--abbrev-ref", "HEAD"], env, cwd=target)


def round_(repos: list[Repo], dest: Path, env: dict[str, str], depth: int) -> bool:
    ok = True
    for repo in repos:
        started = time.monotonic()
        try:
            commit = sync(repo, dest, env, depth)
            print(f"{repo.name}: {commit[:12]} ({time.monotonic() - started:.1f}s)", flush=True)
        except Exception as exc:  # one repository never stops the others
            ok = False
            print(f"{repo.name}: FAILED: {exc}", file=sys.stderr, flush=True)
    return ok


def main(environ: dict[str, str] | None = None) -> int:
    environ = dict(os.environ if environ is None else environ)
    try:
        repos = parse_repos(environ.get("EKOS_GIT_SYNC_REPOS", ""))
        interval = int(environ.get("EKOS_GIT_SYNC_INTERVAL", "300"))
        depth = int(environ.get("EKOS_GIT_SYNC_DEPTH", "0"))
    except (ConfigError, ValueError) as exc:
        print(f"ekos-git-sync: {exc}", file=sys.stderr)
        return 2
    dest = Path(environ.get("EKOS_GIT_SYNC_DEST", "/sources"))
    dest.mkdir(parents=True, exist_ok=True)
    env = git_env(
        environ.get("EKOS_GIT_SYNC_TOKEN") or None,
        environ.get("EKOS_GIT_SYNC_USERNAME", "x-access-token"),
        environ,
    )
    while True:
        ok = round_(repos, dest, env, depth)
        if interval <= 0:
            return 0 if ok else 1
        time.sleep(interval)


if __name__ == "__main__":
    sys.exit(main())
