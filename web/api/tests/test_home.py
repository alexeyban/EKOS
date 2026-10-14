"""RFC 0179 §2/§3: recognising a project in the EKOS home, and how the console runs `ekos` for
one. No subprocess, no binary."""

from __future__ import annotations

import sys
from pathlib import Path

import pytest

from app import config_io, home


# Windows cannot run this test's assumption (see the reason); the job on Windows is informational.
def _not_on_windows(reason: str) -> pytest.MarkDecorator:
    return pytest.mark.skipif(sys.platform == "win32", reason=reason)


@pytest.fixture
def ekos_home(monkeypatch: pytest.MonkeyPatch, tmp_path: Path, reset_settings: None) -> Path:
    root = tmp_path / "home"
    (root / "data" / "default" / "shop" / "state").mkdir(parents=True)
    (root / "data" / "default" / "shop" / "ekos.toml").write_text("[observe]\npaths = []\n")
    monkeypatch.setenv("EKOS_HOME", str(root))
    return root


def test_a_directory_under_data_tenant_project_is_a_project(ekos_home: Path) -> None:
    project = home.project_at(str(ekos_home / "data" / "default" / "shop"))
    assert project is not None
    assert (project.tenant, project.name) == ("default", "shop")
    assert project.dir == ekos_home / "data" / "default" / "shop"
    assert project.state_dir == project.dir / "state"


@pytest.mark.parametrize(
    "relative",
    [
        "data/default",  # a tenant, not a project
        "data/default/shop/state",  # inside a project
        pytest.param(
            "data/Default/shop",  # not a valid tenant name
            marks=_not_on_windows("a case-insensitive file system resolves it to `default`"),
        ),
        "data/default/..",
        "trash/default/shop",
    ],
)
def test_other_paths_in_the_home_are_not_projects(ekos_home: Path, relative: str) -> None:
    assert home.project_at(str(ekos_home / relative)) is None


def test_a_workspace_outside_the_home_is_not_a_project(ekos_home: Path, tmp_path: Path) -> None:
    assert home.project_at(str(tmp_path)) is None


def test_without_a_home_nothing_is_a_project(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path, reset_settings: None
) -> None:
    monkeypatch.delenv("EKOS_HOME", raising=False)
    assert home.configured_home() is None
    assert home.project_at(str(tmp_path / "data" / "default" / "shop")) is None


def test_a_symlink_to_a_project_is_the_project(ekos_home: Path, tmp_path: Path) -> None:
    link = tmp_path / "link"
    link.symlink_to(ekos_home / "data" / "default" / "shop")
    project = home.project_at(str(link))
    assert project is not None and project.name == "shop"


def test_command_for_a_project_selects_it_and_never_relies_on_the_directory(
    ekos_home: Path,
) -> None:
    path = str(ekos_home / "data" / "default" / "shop")
    argv, cwd = home.command("/bin/ekos", path, ["status", "--json"])
    assert argv == [
        "/bin/ekos",
        "--home",
        str(ekos_home),
        "--tenant",
        "default",
        "--project",
        "shop",
        "status",
        "--json",
    ]
    assert cwd == path


def test_command_for_an_in_place_workspace_is_unchanged(ekos_home: Path, tmp_path: Path) -> None:
    argv, cwd = home.command("/bin/ekos", str(tmp_path), ["status", "--json"])
    assert argv == ["/bin/ekos", "status", "--json"]
    assert cwd == str(tmp_path)


def test_mcp_command_names_the_workspace_only_for_an_in_place_one(
    ekos_home: Path, tmp_path: Path
) -> None:
    tail = ["--tcp", "127.0.0.1:7400"]
    in_place = home.mcp_command("/bin/ekos", str(tmp_path), tail)
    assert in_place == ["/bin/ekos", "mcp", "serve", "--workspace", str(tmp_path), *tail]

    project = home.mcp_command("/bin/ekos", str(ekos_home / "data" / "default" / "shop"), tail)
    # `--workspace` together with `--project` is refused by the CLI.
    assert "--workspace" not in project
    assert project[-4:] == ["mcp", "serve", *tail]
    assert project[1:7] == ["--home", str(ekos_home), "--tenant", "default", "--project", "shop"]


def test_state_directory(ekos_home: Path, tmp_path: Path) -> None:
    project = ekos_home / "data" / "default" / "shop"
    assert home.state_dir(str(project)) == project / "state"
    assert home.state_dir(str(tmp_path)) == tmp_path.resolve() / ".ekos"


@pytest.mark.parametrize("name", ["shop", "a", "payments-api_2", "0abc"])
def test_valid_names(name: str) -> None:
    assert home.is_valid_name(name)


@pytest.mark.parametrize("name", ["", "Shop", "-x", "a/b", "..", "a b", "x" * 64, "é"])
def test_invalid_names(name: str) -> None:
    assert not home.is_valid_name(name)


def test_projects_of_the_default_tenant_are_listed(ekos_home: Path) -> None:
    (ekos_home / "data" / "default" / "no-config").mkdir()
    (ekos_home / "data" / "other" / "theirs").mkdir(parents=True)
    (ekos_home / "data" / "other" / "theirs" / "ekos.toml").write_text("")
    assert [p.name for p in home.projects_in(ekos_home)] == ["shop"]


# ── §3: console state under the home ─────────────────────────────────────────


def test_console_state_defaults_under_the_home(ekos_home: Path) -> None:
    from app.settings import get_settings

    s = get_settings()
    assert Path(s.console_db) == ekos_home / "control" / "console.db"
    assert Path(s.runs_dir) == ekos_home / "control" / "runs"


def test_console_state_defaults_are_unchanged_without_a_home(
    monkeypatch: pytest.MonkeyPatch, reset_settings: None
) -> None:
    from app.settings import get_settings

    monkeypatch.delenv("EKOS_HOME", raising=False)
    s = get_settings()
    assert Path(s.console_db) == Path(".ekos-web/console.db")
    assert Path(s.runs_dir) == Path(".ekos-web/runs")


def test_an_explicit_console_db_wins_over_the_home(
    ekos_home: Path, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    from app.settings import get_settings

    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_DB", str(tmp_path / "x.db"))
    assert get_settings().console_db == str(tmp_path / "x.db")


# ── §2: a project's config may not point outside the allowed root ────────────


@_not_on_windows("POSIX absolute paths in the config")
def test_directories_a_config_reads(tmp_path: Path) -> None:
    raw = (
        '[workspace]\nroot = "/sources/a"\n\n'
        '[[sources]]\nname = "x"\nkind = "git"\npath = "/sources/b"\n\n'
        '[[sources]]\nname = "y"\nkind = "dir"\npath = "docs"\n'
    )
    assert config_io.read_roots(raw) == ["/sources/a", "/sources/b", "/sources/a/docs"]


@_not_on_windows("POSIX absolute paths in the config")
def test_roots_outside_the_allowed_directory(tmp_path: Path) -> None:
    allowed = tmp_path / "sources"
    (allowed / "a").mkdir(parents=True)
    inside = f'[workspace]\nroot = "{allowed / "a"}"\n'
    assert config_io.roots_outside(inside, str(allowed)) == []

    outside = f'[workspace]\nroot = "{allowed / "a"}"\n\n[[sources]]\nname = "x"\npath = "/etc"\n'
    assert config_io.roots_outside(outside, str(allowed)) == ["/etc"]

    escaping = f'[workspace]\nroot = "{allowed}/a/../../elsewhere"\n'
    assert len(config_io.roots_outside(escaping, str(allowed))) == 1
