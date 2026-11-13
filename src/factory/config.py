"""Configuration management for the AI Factory."""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any

CONFIG_DIR = Path.home() / ".config" / "factory"
CONFIG_FILE = CONFIG_DIR / "config.json"
DEFAULT_PROJECTS_BASE = Path.home() / "projects" / "ailuracollective"


def default_config() -> dict[str, Any]:
    """Return a default configuration dictionary."""
    return {
        "version": 1,
        "projects_base": str(DEFAULT_PROJECTS_BASE),
        "webhook": {
            "port": 8787,
            "secret": "",
            "funnel_url": "",
        },
        "defaults": {
            "provider": "opencode",
            "model": "opencode-go/beam-free",
            "base_branch": "main",
            "label": "status/ready",
        },
        "repos": [],
    }


def load_config() -> dict[str, Any]:
    """Load configuration from disk, creating defaults if missing."""
    if not CONFIG_FILE.exists():
        cfg = default_config()
        save_config(cfg)
        return cfg
    try:
        data = json.loads(CONFIG_FILE.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return default_config()
    if not isinstance(data, dict):
        return default_config()
    # merge missing keys
    base = default_config()
    for key, value in base.items():
        if key not in data:
            data[key] = value
    if "webhook" not in data or not isinstance(data["webhook"], dict):
        data["webhook"] = base["webhook"]
    if "defaults" not in data or not isinstance(data["defaults"], dict):
        data["defaults"] = base["defaults"]
    if "repos" not in data or not isinstance(data["repos"], list):
        data["repos"] = []
    return data


def save_config(config: dict[str, Any]) -> None:
    """Persist configuration to disk."""
    CONFIG_DIR.mkdir(parents=True, exist_ok=True)
    CONFIG_FILE.write_text(json.dumps(config, indent=2), encoding="utf-8")


def get_repo(config: dict[str, Any], name: str) -> dict[str, Any] | None:
    """Find a repo entry by name or github slug."""
    for repo in config.get("repos", []):
        if repo.get("name") == name or repo.get("github") == name:
            return repo
    return None


def add_repo(config: dict[str, Any], entry: dict[str, Any]) -> None:
    """Add a repo entry if not already present."""
    existing = get_repo(config, entry.get("name", ""))
    if existing is None:
        config.setdefault("repos", []).append(entry)
    else:
        existing.update(entry)


def remove_repo(config: dict[str, Any], name: str) -> bool:
    """Remove a repo entry by name. Returns True if removed."""
    repos = config.get("repos", [])
    before = len(repos)
    config["repos"] = [r for r in repos if r.get("name") != name and r.get("github") != name]
    return len(config["repos"]) < before
