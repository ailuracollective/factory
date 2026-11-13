"""Repository management for the AI Factory."""

from __future__ import annotations

import os
import re
import subprocess
import urllib.request
from pathlib import Path
from typing import Any

from factory.config import get_repo, load_config, save_config


def _run(cmd: list[str], cwd: Path | None = None) -> subprocess.CompletedProcess[str]:
    return subprocess.run(cmd, cwd=cwd, capture_output=True, text=True, check=False)


def resolve_project_path(name_or_path: str, projects_base: Path) -> Path | None:
    """Resolve a project path from a name or path. Does not clone."""
    candidate = Path(name_or_path).expanduser()
    if candidate.exists():
        return candidate.resolve()
    name = name_or_path.rstrip("/").split("/")[-1].replace(".git", "")
    dest = projects_base / name
    if dest.exists():
        return dest.resolve()
    return None


def register_t3(project_path: Path) -> bool:
    """Register a project in T3 Code."""
    t3_bin = os.environ.get("ISSUE_FACTORY_T3", "t3")
    result = _run([t3_bin, "project", "add", str(project_path)])
    return result.returncode == 0


def sync_labels(repo_slug: str, labels_file: Path | None = None) -> int:
    """Apply labels from standards repo. Returns count applied."""
    if labels_file is None or not labels_file.exists():
        url = "https://raw.githubusercontent.com/ailuracollective/standards/main/.github/labels.yml"
        try:
            data = urllib.request.urlopen(url).read().decode()
        except Exception:
            return 0
        labels_file = Path("/tmp/labels.yml")
        labels_file.write_text(data)
    text = labels_file.read_text()
    pattern = re.compile(
        r'-\s*name:\s*"([^"]+)"\s*\n\s*color:\s*"([^"]+)"\s*\n\s*description:\s*"([^"]*)"',
        re.MULTILINE,
    )
    count = 0
    for name, color, desc in pattern.findall(text):
        _run(
            [
                "gh",
                "label",
                "create",
                name,
                "--color",
                color,
                "--description",
                desc,
                "--repo",
                repo_slug,
            ]
        )
        _run(
            [
                "gh",
                "label",
                "edit",
                name,
                "--color",
                color,
                "--description",
                desc,
                "--repo",
                repo_slug,
            ]
        )
        count += 1
    return count


def _detect_slug(project_path: Path) -> str:
    """Detect the GitHub slug from a project's git remote."""
    result = _run(["git", "-C", str(project_path), "remote", "get-url", "origin"])
    remote = result.stdout.strip()
    if "github.com" in remote:
        return remote.split("github.com")[-1].strip(":/").replace(".git", "")
    return project_path.name


def add_repo(name_or_path: str) -> dict[str, Any] | None:
    """Add a repo to the factory (no cloning). Returns the entry or None on failure."""
    config = load_config()
    default_base = str(Path.home() / "projects" / "ailuracollective")
    projects_base = Path(config.get("projects_base", default_base))

    project_path = resolve_project_path(name_or_path, projects_base)
    if project_path is None:
        return None

    slug = _detect_slug(project_path)
    name = slug.split("/")[-1]

    register_t3(project_path)
    sync_labels(slug)

    entry = {
        "name": name,
        "path": str(project_path),
        "github": slug,
        "hook_id": None,
        "enabled": True,
    }
    from factory.config import add_repo as cfg_add

    cfg_add(config, entry)
    save_config(config)
    return entry


def remove_repo(name: str) -> bool:
    """Remove a repo from config (does not delete files)."""
    config = load_config()
    repo = get_repo(config, name)
    if repo is None:
        return False
    hook_id = repo.get("hook_id")
    github = repo.get("github")
    if hook_id and github:
        _run(["gh", "api", f"repos/{github}/hooks/{hook_id}", "-X", "DELETE"])
    from factory.config import remove_repo as cfg_remove

    cfg_remove(config, name)
    save_config(config)
    return True


def list_repos() -> list[dict[str, Any]]:
    """List configured repos."""
    return load_config().get("repos", [])
