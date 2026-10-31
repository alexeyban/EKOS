"""Running in a cloud (RFC 0232): one writer over a shared home, and backups to object storage.

**The instance lease.** On a VM the home is a local disk and there is one console. On a
serverless container platform (Azure Container Apps, Cloud Run, ECS Fargate) or Kubernetes the
home is a network share, and a new revision starts while the old one still runs. Two consoles
writing one SQLite database and one ledger over SMB or NFS can corrupt both. The lease is a file
in the control directory: the newest console writes its id there, waits for the previous holder
to see it, and the previous holder shuts itself down at its next heartbeat. Last start wins;
there is never a moment when no console can serve, and never more than one heartbeat interval
when two run.

**Scheduled backups.** With `backup_url` set, the console runs `ekos home backup --to <url>` on
`backup_cron` (UTC). Credentials are the machine's cloud identity, passed to that one process
through `platform_env`; a tenant's processes never get them. Retention is the bucket's lifecycle
rule, set by the Terraform modules, not something the console deletes.
"""

from __future__ import annotations

import asyncio
import contextlib
import json
import logging
import os
import socket
import time
import uuid
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path

from apscheduler.triggers.cron import CronTrigger

from . import _proc, envs

log = logging.getLogger("ekos.console.cloud")

LEASE = "instance.lease"

# What the cloud identity of the machine or container is made of, for the backup process only:
# the providers' own variables, the managed-identity endpoints of Azure Container Apps / App
# Service, and the metadata host override some GCP runtimes set.
_PLATFORM_PREFIXES = ("AWS_", "AZURE_", "GOOGLE_", "OBJECT_STORE_")
_PLATFORM_NAMES = frozenset(
    {"IDENTITY_ENDPOINT", "IDENTITY_HEADER", "MSI_ENDPOINT", "MSI_SECRET", "GCE_METADATA_HOST"}
)


def platform_env(environ: dict[str, str] | None = None) -> dict[str, str]:
    """The environment of a platform job (a backup): the base every process gets, plus the cloud
    identity variables. Never a tenant's model keys, never the console's own settings."""
    environ = dict(os.environ) if environ is None else environ
    return {
        k: v
        for k, v in environ.items()
        if not k.upper().startswith(envs.CONSOLE_PREFIX)
        and (
            k in envs.BASE
            or k.startswith(envs._BASE_PREFIXES)
            or k in _PLATFORM_NAMES
            or k.startswith(_PLATFORM_PREFIXES)
        )
    }


# ── the lease ────────────────────────────────────────────────────────────────


@dataclass(frozen=True)
class Holder:
    id: str
    generation: int
    host: str
    heartbeat: float


def _read(path: Path) -> Holder | None:
    try:
        data = json.loads(path.read_text())
        return Holder(
            id=str(data["id"]),
            generation=int(data["generation"]),
            host=str(data.get("host", "")),
            heartbeat=float(data["heartbeat"]),
        )
    except (OSError, ValueError, KeyError, TypeError):
        return None


def _write(path: Path, holder: Holder) -> None:
    # Write then rename: a reader on another machine sees the old file or the new one, never half.
    tmp = path.with_name(f".{path.name}.{holder.id}.tmp")
    tmp.write_text(
        json.dumps(
            {
                "id": holder.id,
                "generation": holder.generation,
                "host": holder.host,
                "heartbeat": holder.heartbeat,
            }
        )
    )
    os.replace(tmp, path)


class InstanceLease:
    """The control directory's single-writer lease. `interval` is the heartbeat; a holder whose
    heartbeat is older than `3 * interval` is gone."""

    def __init__(
        self,
        control: Path,
        *,
        interval: float = 10.0,
        clock: Callable[[], float] = time.time,
        instance_id: str | None = None,
    ) -> None:
        self.path = control / LEASE
        self.interval = interval
        self.clock = clock
        self.id = instance_id or f"{socket.gethostname()}-{os.getpid()}-{uuid.uuid4().hex[:8]}"
        self.generation = 0

    def stale(self, holder: Holder) -> bool:
        return self.clock() - holder.heartbeat > 3 * self.interval

    def take(self) -> Holder | None:
        """Write this instance into the lease. Returns the previous holder if it is still alive,
        so the caller can wait for it to stand down."""
        self.path.parent.mkdir(parents=True, exist_ok=True)
        previous = _read(self.path)
        self.generation = (previous.generation if previous else 0) + 1
        _write(
            self.path,
            Holder(self.id, self.generation, socket.gethostname(), self.clock()),
        )
        if previous is None or previous.id == self.id or self.stale(previous):
            return None
        log.warning(
            "taking over from console %s on %s (generation %d → %d)",
            previous.id,
            previous.host,
            previous.generation,
            self.generation,
        )
        return previous

    async def take_over(self) -> None:
        """`take`, then give a live previous holder one heartbeat and a margin to see the new id
        and stop before this one starts writing."""
        if self.take() is not None:
            await asyncio.sleep(self.interval * 1.5)

    def beat(self) -> bool:
        """Renew the heartbeat. False when another instance holds the lease: this one must stop."""
        current = _read(self.path)
        if current is not None and current.id != self.id:
            return False
        _write(self.path, Holder(self.id, self.generation, socket.gethostname(), self.clock()))
        return True

    async def hold(self, on_lost: Callable[[], None]) -> None:
        while True:
            await asyncio.sleep(self.interval)
            try:
                kept = self.beat()
            except OSError as exc:
                # A share that is briefly unreachable is not a lost lease; the next beat retries.
                log.warning("could not renew the instance lease: %s", exc)
                continue
            if not kept:
                log.error("another console took the instance lease; shutting down")
                on_lost()
                return

    def release(self) -> None:
        current = _read(self.path)
        if current is not None and current.id == self.id:
            with contextlib.suppress(OSError):
                self.path.unlink()


# ── scheduled backups ────────────────────────────────────────────────────────


class BackupError(ValueError):
    """A backup schedule the console cannot run."""


def backup_trigger(cron: str) -> CronTrigger:
    try:
        return CronTrigger.from_crontab(cron, timezone="UTC")
    except Exception as exc:
        raise BackupError(f"invalid backup_cron {cron!r}: {exc}") from exc


def backup_argv(ekos_bin: str, url: str) -> list[str]:
    if not url.endswith("/"):
        url += "/"
    return [ekos_bin, "home", "backup", "--to", url]


class RestoreError(RuntimeError):
    """The first-start restore failed for a reason other than there being no backup."""


RESTORE_TIMEOUT = 2 * 3600

# What `ekos home restore --from <location>/` says when the location holds no backup.
_NO_BACKUP = "no backup under"


def home_is_empty(home: Path, console_db: Path) -> bool:
    """No project in the home and no console database: a fresh deployment (or a new share)."""
    if console_db.exists():
        return False
    return not any((home / "data").glob("*/*/ekos.toml"))


def restore_argv(ekos_bin: str, url: str) -> list[str]:
    if not url.endswith("/"):
        url += "/"
    return [ekos_bin, "home", "restore", "--from", url]


async def restore_if_empty(
    ekos_bin: str,
    url: str,
    home: Path,
    console_db: Path,
) -> str:
    """Restore the newest backup under `url` into an empty home, before the console opens its
    database. Returns `"not-empty"`, `"no-backup"` or `"restored"`.

    Any other failure raises: a console that cannot read its backup location must not start over
    an empty home, because its first scheduled backup would then be the newest one there."""
    if not home_is_empty(home, console_db):
        return "not-empty"
    argv = restore_argv(ekos_bin, url)
    log.info("the home is empty; restoring the newest backup under %s", argv[-1])
    proc = await _proc.spawn(argv, env=platform_env())
    try:
        out, err = await asyncio.wait_for(proc.communicate(), timeout=RESTORE_TIMEOUT)
    except TimeoutError:
        await _proc.terminate(proc)
        raise RestoreError(
            f"restoring from {argv[-1]} timed out after {RESTORE_TIMEOUT}s"
        ) from None
    text = _proc.strip_ansi((out + err).decode(errors="replace")).strip()
    if proc.returncode == 0:
        # A warning so it reaches the platform's log (the console logs warnings and up): a
        # whole home replaced at start must be visible.
        log.warning("the home was empty; restored the newest backup under %s", argv[-1])
        return "restored"
    if _NO_BACKUP in text:
        log.info("no backup under %s yet; starting with an empty home", argv[-1])
        return "no-backup"
    raise RestoreError(
        f"could not restore the empty home from {argv[-1]} (exit {proc.returncode}): "
        f"{text[-2000:]}. The console does not start over an empty home it cannot restore; "
        "fix the location or its credentials, or set backup_restore_on_empty = false"
    )


class BackupJob:
    """`ekos home backup --to <url>`, one at a time."""

    def __init__(self, ekos_bin: str, url: str, *, timeout: float = 6 * 3600) -> None:
        self.argv = backup_argv(ekos_bin, url)
        self.timeout = timeout
        self._lock = asyncio.Lock()
        self.last: tuple[float, int, str] | None = None  # (finished at, exit code, last line)

    async def run(self) -> int:
        if self._lock.locked():
            log.warning("a backup is still running; this one is skipped")
            return -1
        async with self._lock:
            proc = await _proc.spawn(self.argv, env=platform_env())
            try:
                out, err = await asyncio.wait_for(proc.communicate(), timeout=self.timeout)
            except TimeoutError:
                await _proc.terminate(proc)
                self.last = (time.time(), 124, "timed out")
                log.error("backup timed out after %ss", self.timeout)
                return 124
            text = _proc.strip_ansi((out + err).decode(errors="replace")).strip()
            tail = text.splitlines()[-1].strip() if text else ""
            code = proc.returncode if proc.returncode is not None else -1
            self.last = (time.time(), code, tail)
            if code == 0:
                log.info("backup written: %s", tail)
            else:
                log.error("backup failed (exit %s): %s", code, text[-2000:])
            return code
