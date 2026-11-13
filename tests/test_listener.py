"""Tests for the issue factory listener."""

from pathlib import Path

from factory.listener import build_prompt, load_state, save_state


def test_load_state_missing(tmp_path: Path) -> None:
    """Missing state file yields empty collections."""
    processed, threads = load_state(tmp_path / "missing.json")
    assert processed == set()
    assert threads == {}


def test_save_and_load_state(tmp_path: Path) -> None:
    """State round-trips through disk."""
    path = tmp_path / "state.json"
    save_state(path, {"1", "2"}, {"1": "thread-1"})
    processed, threads = load_state(path)
    assert processed == {"1", "2"}
    assert threads == {"1": "thread-1"}


def test_load_state_corrupt(tmp_path: Path) -> None:
    """Corrupt state file is ignored gracefully."""
    path = tmp_path / "state.json"
    path.write_text("{not json", encoding="utf-8")
    processed, threads = load_state(path)
    assert processed == set()
    assert threads == {}


def test_build_prompt_contains_key_fields() -> None:
    """Prompt includes issue metadata and PR instruction."""
    issue = {
        "number": 7,
        "title": "Fix bug",
        "body": "Details here",
        "url": "https://example.com/issue/7",
        "labels": [{"name": "ai"}, "extra"],
    }
    prompt = build_prompt(issue)
    assert "issue #7" in prompt
    assert "Fix bug" in prompt
    assert "Details here" in prompt
    assert "Closes #7" in prompt
    assert "ai, extra" in prompt
