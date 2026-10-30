"""RFC 0232: the instance lease over a shared home, and scheduled backups to object storage."""

from __future__ import annotations

import asyncio
import os
import stat
import sys
from pathlib import Path

import pytest

from app import cloud


class Clock:
    def __init__(self, now: float = 1000.0) -> None:
        self.now = now

    def __call__(self) -> float:
        return self.now


def test_the_first_console_takes_an_empty_lease_without_waiting(tmp_path: Path) -> None:
    a = cloud.InstanceLease(tmp_path, instance_id="a", clock=Clock())
    assert a.take() is None
    assert a.generation == 1
    assert a.beat() is True


def test_a_new_console_takes_over_and_the_old_one_learns_it_lost(tmp_path: Path) -> None:
    clock = Clock()
    old = cloud.InstanceLease(tmp_path, instance_id="old", clock=clock)
    old.take()
    clock.now += 5
    new = cloud.InstanceLease(tmp_path, instance_id="new", clock=clock)
    previous = new.take()
    # The old one is alive, so the new one is told to wait for it.
    assert previous is not None and previous.id == "old"
    assert new.generation == 2
    assert old.beat() is False  # it must stop
    assert new.beat() is True
    # And a lost holder never writes itself back.
    assert cloud._read(new.path).id == "new"


def test_a_stale_holder_is_not_waited_for(tmp_path: Path) -> None:
    clock = Clock()
    cloud.InstanceLease(tmp_path, instance_id="dead", clock=clock, interval=10).take()
    clock.now += 31
    assert cloud.InstanceLease(tmp_path, instance_id="b", clock=clock, interval=10).take() is None


def test_a_damaged_lease_file_counts_as_none(tmp_path: Path) -> None:
    (tmp_path / cloud.LEASE).write_text("{not json")
    lease = cloud.InstanceLease(tmp_path, instance_id="a", clock=Clock())
    assert lease.take() is None
    assert lease.generation == 1


async def test_hold_calls_on_lost_when_another_console_takes_the_lease(tmp_path: Path) -> None:
    lost: list[str] = []
    a = cloud.InstanceLease(tmp_path, instance_id="a", interval=0.01)
    a.take()
    task = asyncio.create_task(a.hold(lambda: lost.append("a")))
    await asyncio.sleep(0.05)
    assert lost == []
    cloud.InstanceLease(tmp_path, instance_id="b", interval=0.01).take()
    await asyncio.wait_for(task, timeout=2)
    assert lost == ["a"]


def test_release_removes_only_its_own_lease(tmp_path: Path) -> None:
    a = cloud.InstanceLease(tmp_path, instance_id="a", clock=Clock())
    a.take()
    b = cloud.InstanceLease(tmp_path, instance_id="b", clock=Clock())
    b.take()
    a.release()
    assert (tmp_path / cloud.LEASE).exists()
    b.release()
    assert not (tmp_path / cloud.LEASE).exists()


def test_the_backup_process_gets_cloud_identity_and_nothing_of_a_tenant_or_the_console() -> None:
    environ = {
        "PATH": "/usr/bin",
        "EKOS_HOME": "/var/lib/ekos",
        "AWS_REGION": "eu-central-1",
        "AWS_CONTAINER_CREDENTIALS_RELATIVE_URI": "/v2/credentials/x",
        "AZURE_CLIENT_ID": "00000000-0000-0000-0000-000000000000",
        "IDENTITY_ENDPOINT": "http://localhost:42356/msi/token",
        "IDENTITY_HEADER": "h",
        "GOOGLE_CLOUD_PROJECT": "p",
        "OPENAI_API_KEY": "sk-tenant",
        "EKOS_CONSOLE_SESSION_SECRET": "console",
        "EKOS_DB_DEFAULT_ERP_MAIN": "db-password",
    }
    env = cloud.platform_env(environ)
    for kept in [
        "PATH",
        "EKOS_HOME",
        "AWS_REGION",
        "AWS_CONTAINER_CREDENTIALS_RELATIVE_URI",
        "AZURE_CLIENT_ID",
        "IDENTITY_ENDPOINT",
        "IDENTITY_HEADER",
        "GOOGLE_CLOUD_PROJECT",
    ]:
        assert env[kept] == environ[kept], kept
    for dropped in ["OPENAI_API_KEY", "EKOS_CONSOLE_SESSION_SECRET", "EKOS_DB_DEFAULT_ERP_MAIN"]:
        assert dropped not in env, dropped


def test_backup_argv_and_trigger() -> None:
    assert cloud.backup_argv("ekos", "s3://b/ekos") == [
        "ekos",
        "home",
        "backup",
        "--to",
        "s3://b/ekos/",
    ]
    cloud.backup_trigger("17 2 * * *")
    with pytest.raises(cloud.BackupError):
        cloud.backup_trigger("every night")


@pytest.mark.skipif(sys.platform == "win32", reason="a #! fake binary")
async def test_a_backup_runs_the_command_and_records_the_result(tmp_path: Path) -> None:
    fake = tmp_path / "ekos"
    fake.write_text(
        '#!/bin/sh\necho "Backed up 1 project(s)"\necho "  $4ekos-backup-1.tar.zst"\nexit 0\n'
    )
    fake.chmod(fake.stat().st_mode | stat.S_IEXEC)
    job = cloud.BackupJob(str(fake), "gs://bucket/ekos/")
    assert await job.run() == 0
    assert job.last is not None and job.last[1] == 0
    assert job.last[2] == "gs://bucket/ekos/ekos-backup-1.tar.zst"

    failing = tmp_path / "failing"
    failing.write_text("#!/bin/sh\necho 'no credentials' >&2\nexit 1\n")
    failing.chmod(failing.stat().st_mode | stat.S_IEXEC)
    job = cloud.BackupJob(str(failing), "gs://bucket/ekos/")
    assert await job.run() == 1
    assert job.last[2] == "no credentials"


def test_the_cloud_settings_have_rows_in_the_console_file() -> None:
    from app import console_file

    for name in ["instance_lease", "instance_lease_seconds", "backup_url", "backup_cron"]:
        assert name in console_file.FIELDS
        assert not console_file.FIELDS[name].secret


def test_the_lease_survives_concurrent_writers_without_a_torn_file(tmp_path: Path) -> None:
    # Two writers replacing the file in turn: every read sees a whole record.
    a = cloud.InstanceLease(tmp_path, instance_id="a", clock=Clock())
    b = cloud.InstanceLease(tmp_path, instance_id="b", clock=Clock())
    for _ in range(50):
        a.take()
        assert cloud._read(a.path) is not None
        b.take()
        assert cloud._read(b.path) is not None
    leftovers = [p for p in os.listdir(tmp_path) if p.endswith(".tmp")]
    assert leftovers == []
