"""`config_io` — tomlkit round-trip, observe diff, `.bak` write (RFC 0130 §2)."""

from __future__ import annotations

from pathlib import Path

import pytest

from app import config_io

SAMPLE = """\
# top comment
[observe]
paths = ["crates", "docs"]        # keep it tight
ignore-patterns = ["target", ".git"]
"""


def test_parse_rejects_malformed_toml() -> None:
    with pytest.raises(config_io.ConfigError):
        config_io.parse("[observe\npaths = ")


def test_read_config_returns_raw_and_observe(tmp_path: Path) -> None:
    (tmp_path / "ekos.toml").write_text(SAMPLE)
    raw, observe = config_io.read_config(str(tmp_path))
    assert "# top comment" in raw
    assert observe.paths == ["crates", "docs"]
    assert observe.ignore_patterns == ["target", ".git"]


def test_config_path_rejects_a_symlinked_ekos_toml_escaping_the_workspace(tmp_path: Path) -> None:
    """SonarCloud pythonsecurity:S2083 hardening: `ekos.toml` resolving outside the workspace
    root (e.g. a symlink) is rejected rather than silently followed."""
    outside = tmp_path / "outside.toml"
    outside.write_text(SAMPLE)
    workspace = tmp_path / "workspace"
    workspace.mkdir()
    (workspace / "ekos.toml").symlink_to(outside)

    # The message deliberately names no paths — it is returned verbatim to API callers
    # (CWE-209); the real paths go to the log instead. See the leak test further down.
    with pytest.raises(config_io.ConfigError, match="resolves outside the workspace root"):
        config_io.config_path(str(workspace))


def test_diff_observe_detects_narrowing() -> None:
    after = SAMPLE.replace('["crates", "docs"]', '["crates"]').replace(
        '"target", ".git"', '"target"'
    )
    delta = config_io.diff_observe(SAMPLE, after)
    assert delta.removed_paths == ["docs"]
    assert delta.removed_patterns == [".git"]
    assert delta.narrows
    assert config_io.append_only_warning(delta) is not None


def test_diff_observe_widening_has_no_warning() -> None:
    after = SAMPLE.replace('["crates", "docs"]', '["crates", "docs", "tests"]')
    delta = config_io.diff_observe(SAMPLE, after)
    assert delta.added_paths == ["tests"]
    assert not delta.narrows
    assert config_io.append_only_warning(delta) is None


def test_write_config_keeps_a_bak_and_preserves_comments(tmp_path: Path) -> None:
    cfg = tmp_path / "ekos.toml"
    cfg.write_text(SAMPLE)
    new = SAMPLE.replace('["crates", "docs"]', '["crates"]')
    config_io.write_config(str(tmp_path), new)
    assert cfg.read_text() == new
    bak = tmp_path / "ekos.toml.bak"
    assert bak.is_file()
    assert bak.read_text() == SAMPLE
    assert "# top comment" in new  # tomlkit-authored text still carries the comment


def test_write_config_refuses_malformed_toml_before_touching_the_file(tmp_path: Path) -> None:
    cfg = tmp_path / "ekos.toml"
    cfg.write_text(SAMPLE)
    with pytest.raises(config_io.ConfigError):
        config_io.write_config(str(tmp_path), "[observe\n")
    assert cfg.read_text() == SAMPLE
    assert not (tmp_path / "ekos.toml.bak").exists()


# ── RFC 0130 hardening: security fixes (devlog_203) ──────────────────────────


def test_error_messages_never_leak_absolute_server_paths(tmp_path):
    """`ConfigError` is surfaced verbatim as an HTTPException detail, so anything interpolated
    into it is returned to the API caller (CWE-209)."""
    with pytest.raises(config_io.ConfigError) as missing:
        config_io.read_config(str(tmp_path))
    assert str(tmp_path) not in str(missing.value)
    assert "ekos.toml" in str(missing.value)

    workspace = tmp_path / "ws"
    workspace.mkdir()
    outside = tmp_path / "outside.toml"
    outside.write_text("[observe]\n")
    (workspace / "ekos.toml").symlink_to(outside)
    with pytest.raises(config_io.ConfigError) as escape:
        config_io.config_path(str(workspace))
    assert str(tmp_path) not in str(escape.value)
    assert str(outside) not in str(escape.value)


def test_atomic_write_replaces_a_symlink_instead_of_following_it(tmp_path):
    """The TOCTOU window `config_path` cannot close on its own.

    `config_path` resolves, so a symlink that already exists at check time is followed and the
    *target* is what gets validated and written — by design, since the target must still be
    inside the workspace. The window is the other ordering: the check passes on a plain path,
    and a symlink appears at that name before the write lands. `write_text` would follow it;
    `os.replace` swaps the directory entry, so the planted link is destroyed rather than obeyed.
    """
    victim = tmp_path / "victim"
    victim.write_text("ORIGINAL")
    planted = tmp_path / "ekos.toml"
    planted.symlink_to(victim)

    config_io._atomic_write(planted, "REPLACED")

    assert victim.read_text() == "ORIGINAL", "the symlink target must not be written through"
    assert not planted.is_symlink(), "the planted symlink itself must be replaced"
    assert planted.read_text() == "REPLACED"


def test_write_config_keeps_the_resolved_target_inside_the_workspace(tmp_path):
    """Documents the deliberate half: an in-workspace symlink present at check time IS followed,
    because `config_path` resolved it and confirmed the target is still directly inside the
    workspace root. Nothing escapes; the write simply lands on the resolved file."""
    workspace = tmp_path / "ws"
    workspace.mkdir()
    target = workspace / "other.toml"
    target.write_text("[observe]\npaths = ['ORIGINAL']\n")
    (workspace / "ekos.toml").symlink_to(target)

    config_io.write_config(str(workspace), "[observe]\npaths = ['NEW']\n")

    assert "NEW" in target.read_text()
    assert not (tmp_path / "victim.toml").exists()


def test_written_config_and_backup_are_owner_only(tmp_path):
    config_io.write_config(str(tmp_path), "[observe]\npaths = ['.']\n")
    config_io.write_config(str(tmp_path), "[observe]\npaths = ['src']\n")
    assert (tmp_path / "ekos.toml").stat().st_mode & 0o777 == 0o600
    assert (tmp_path / "ekos.toml.bak").stat().st_mode & 0o777 == 0o600


def test_oversized_config_is_rejected_before_parsing(tmp_path):
    huge = "# " + "x" * config_io.MAX_CONFIG_BYTES + "\n"
    with pytest.raises(config_io.ConfigError, match="larger than"):
        config_io.parse(huge)

    (tmp_path / "ekos.toml").write_text(huge)
    with pytest.raises(config_io.ConfigError, match="larger than"):
        config_io.read_config(str(tmp_path))


def test_a_failed_write_leaves_no_temp_file_behind(tmp_path, monkeypatch):
    (tmp_path / "ekos.toml").write_text("[observe]\n")

    def boom(*a, **k):
        raise OSError("disk full")

    monkeypatch.setattr(config_io.os, "replace", boom)
    with pytest.raises(OSError):
        config_io.write_config(str(tmp_path), "[observe]\npaths = ['.']\n")

    leftovers = list(tmp_path.glob(".ekos-toml-*"))
    assert leftovers == [], f"temp files leaked: {leftovers}"
