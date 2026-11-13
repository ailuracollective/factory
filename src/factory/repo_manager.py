"""Repository management for the AI Factory."""

from __future__ import annotations

import subprocess
from pathlib import Path
from typing import Any

from factory.config import get_repo, load_config, save_config


def _run(cmd: list[str], cwd: Path | None = None) -> subprocess.CompletedProcess[str]:
    return subprocess.run(cmd, cwd=cwd, capture_output=True, text=True, check=False)


def clone_repo(url_or_name: str, projects_base: Path) -> Path | None:
    """Clone a repo if not present. Returns the path."""
    name = url_or_name.rstrip("/").split("/")[-1].replace(".git", "")
    dest = projects_base / name
    if dest.exists():
        return dest
    url = url_or_name if url_or_name.startswith("http") else f"https://github.com/{url_or_name}.git"
    result = _run(["git", "clone", url, str(dest)])
    if result.returncode != 0:
        return None
    return dest


def register_t3(project_path: Path) -> bool:
    """Register a project in T3 Code."""
    import os

    t3_bin = os.environ.get("ISSUE_FACTORY_T3", "t3")
    result = _run([t3_bin, "project", "add", str(project_path)])
    return result.returncode == 0


def sync_labels(repo_slug: str, labels_file: Path | None = None) -> int:
    """Apply labels from standards repo. Returns count applied."""
    import urllib.request

    if labels_file is None or not labels_file.exists():
        # download from standards repo
        url = "https://raw.githubusercontent.com/ailuracollective/standards/main/.github/labels.yml"
        try:
            data = urllib.request.urlopen(url).read().decode()
        except Exception:
            return 0
        labels_file = Path("/tmp/labels.yml")
        labels_file.write_text(data)
    import re

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


def add_repo(name_or_url: str) -> dict[str, Any] | None:
    """Add a repo to the factory. Returns the entry or None on failure."""
    config = load_config()
    projects_base = Path(
        config.get("projects_base", str(Path.home() / "projects" / "ailuracollective"))
    )
    projects_base.mkdir(parents=True, exist_ok=True)

    # determine github slug
    if "/" in name_or_url and not name_or_url.startswith("http"):
        slug = name_or_url
    else:
        slug = (
            name_or_url.rstrip("/").split("github.com/")[-1].replace(".git", "")
            if "github.com" in name_or_url
            else name_or_url
        )
    name = slug.split("/")[-1]

    project_path = clone_repo(slug, projects_base)
    if project_path is None:
        return None

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
    # remove webhook
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
