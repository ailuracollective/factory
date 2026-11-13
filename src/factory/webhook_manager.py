"""Webhook management for the AI Factory."""

from __future__ import annotations

import json
import subprocess

from factory.config import get_repo, load_config, save_config


def _run(cmd: list[str]) -> subprocess.CompletedProcess[str]:
    return subprocess.run(cmd, capture_output=True, text=True, check=False)


def create_webhook(repo_slug: str, url: str, secret: str) -> int | None:
    """Create a webhook on a repo. Returns the hook id or None."""
    result = _run(
        [
            "gh",
            "api",
            f"repos/{repo_slug}/hooks",
            "-X",
            "POST",
            "-f",
            "name=web",
            "-F",
            "active=true",
            "-f",
            "events[]=issues",
            "-f",
            f"config[url]={url}",
            "-f",
            "config[content_type]=json",
            "-f",
            f"config[secret]={secret}",
            "-f",
            "config[insecure_ssl]=0",
        ]
    )
    if result.returncode != 0:
        return None
    try:
        data = json.loads(result.stdout)
    except json.JSONDecodeError:
        return None
    return data.get("id")


def update_webhook(repo_slug: str, hook_id: int, url: str, secret: str) -> bool:
    """Update a webhook's url and secret."""
    result = _run(
        [
            "gh",
            "api",
            f"repos/{repo_slug}/hooks/{hook_id}",
            "-X",
            "PATCH",
            "-f",
            f"config[url]={url}",
            "-f",
            "config[content_type]=json",
            "-f",
            f"config[secret]={secret}",
            "-f",
            "config[insecure_ssl]=0",
        ]
    )
    return result.returncode == 0


def delete_webhook(repo_slug: str, hook_id: int) -> bool:
    """Delete a webhook."""
    result = _run(["gh", "api", f"repos/{repo_slug}/hooks/{hook_id}", "-X", "DELETE"])
    return result.returncode == 0


def sync_webhook(name: str) -> bool:
    """Ensure a repo has a webhook pointing to the configured funnel URL."""
    config = load_config()
    repo = get_repo(config, name)
    if repo is None:
        return False
    github = repo.get("github")
    url = config.get("webhook", {}).get("funnel_url", "")
    secret = config.get("webhook", {}).get("secret", "")
    if not github or not url or not secret:
        return False
    hook_id = repo.get("hook_id")
    if hook_id:
        ok = update_webhook(github, hook_id, url, secret)
    else:
        new_id = create_webhook(github, url, secret)
        if new_id is None:
            return False
        repo["hook_id"] = new_id
        save_config(config)
        ok = True
    return ok
