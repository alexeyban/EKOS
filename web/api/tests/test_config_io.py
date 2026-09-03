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
