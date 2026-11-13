"""State management for the AI Factory."""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any

STATE_DIR = Path.home() / ".config" / "factory"
STATE_FILE = STATE_DIR / "state.json"


def load_state(path: Path | None = None) -> tuple[set[str], dict[str, str]]:
    """Load processed issue keys and thread ids from a state file."""
    if path is None:
        path = STATE_FILE
    if not path.exists():
        return set(), {}
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return set(), {}
    if not isinstance(data, dict):
        return set(), {}
    raw_ids = data.get("processed", [])
    processed: set[str] = set()
    if isinstance(raw_ids, list):
        for value in raw_ids:
            processed.add(str(value))
    raw_threads = data.get("threads", {})
    threads: dict[str, str] = {}
    if isinstance(raw_threads, dict):
        for key, value in raw_threads.items():
            threads[str(key)] = str(value)
    return processed, threads


def save_state(processed: set[str], threads: dict[str, str], path: Path | None = None) -> None:
    """Persist processed issue keys and thread ids to a state file."""
    if path is None:
        path = STATE_FILE
    path.parent.mkdir(parents=True, exist_ok=True)
    payload: dict[str, Any] = {"processed": sorted(processed)}
    if threads:
        payload["threads"] = dict(threads.items())
    path.write_text(json.dumps(payload, indent=2), encoding="utf-8")
