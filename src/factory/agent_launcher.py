"""Agent launching for the AI Factory."""

from __future__ import annotations

import json
import logging
import os
import re
import subprocess
from pathlib import Path
from typing import Any

from factory.config import load_config


def _slugify(text: str) -> str:
    """Convert text to a branch-safe slug."""
    slug = re.sub(r"[^a-zA-Z0-9]+", "-", text.strip().lower()).strip("-")
    return slug[:40] or "issue"


def build_prompt(issue: dict[str, Any]) -> str:
    """Build the agent prompt for a GitHub issue."""
    number = issue.get("number")
    title = str(issue.get("title", "")).strip()
    body = str(issue.get("body", "") or "").strip()
    url = str(issue.get("url", "")).strip()
    labels = issue.get("labels")
    label_names = ""
    if isinstance(labels, list):
        names: list[str] = []
        for label in labels:
            if isinstance(label, dict):
                name = label.get("name")
                if name is not None:
                    names.append(str(name))
            elif isinstance(label, str):
                names.append(label)
        label_names = ", ".join(names)
    return (
        f"Resolve GitHub issue #{number}: {title}\n\n"
        f"URL: {url}\n"
        f"Labels: {label_names}\n\n"
        "## Issue body\n"
        f"{body}\n\n"
        "## Instructions\n"
        "- Work in the provided worktree/branch.\n"
        "- Implement the requested change with tests.\n"
        "- Run the project test suite and linters before finishing.\n"
        f"- Open a pull request that references the issue (Closes #{number}).\n"
        "- Do not merge the pull request.\n"
    )


def _parse_thread_id(output: str) -> str | None:
    """Extract a thread id from t3 CLI output."""
    text = output.strip()
    if not text:
        return None
    try:
        data = json.loads(text)
    except json.JSONDecodeError:
        for raw_line in text.splitlines():
            stripped = raw_line.strip()
            if stripped:
                return stripped
        return None
    if isinstance(data, dict):
        for key in ("threadId", "thread_id", "id"):
            value = data.get(key)
            if isinstance(value, str) and value:
                return value
    if isinstance(data, str) and data:
        return data
    return None


def launch_agent(
    issue: dict[str, Any], project: Path, config: dict[str, Any] | None = None
) -> str | None:
    """Launch a T3 Code agent thread for the given issue."""
    if config is None:
        config = load_config()
    defaults = config.get("defaults", {})
    t3_bin = os.environ.get("ISSUE_FACTORY_T3", "t3")
    os.environ.get("ISSUE_FACTORY_GH", "gh")
    number = issue.get("number")
    if not isinstance(number, int):
        return None
    title = str(issue.get("title", "")).strip()
    branch = f"issue-{number}-{_slugify(title)}"
    thread_title = f"Fix issue #{number}: {title}"[:80]
    prompt = build_prompt(issue)
    base_branch = defaults.get("base_branch", "main")
    provider = defaults.get("provider", "opencode")
    model = defaults.get("model", "opencode-go/beam-free")
    # env overrides
    provider = os.environ.get("ISSUE_FACTORY_PROVIDER", provider)
    model = os.environ.get("ISSUE_FACTORY_MODEL", model)
    base_branch = os.environ.get("ISSUE_FACTORY_BASE_BRANCH", base_branch)
    args = [
        t3_bin,
        "thread",
        "start",
        str(project),
        prompt,
        "--title",
        thread_title,
        "--provider",
        provider,
        "--model",
        model,
        "--worktree",
        base_branch,
        "--branch",
        branch,
        "--json",
    ]
    logger = logging.getLogger(__name__)
    logger.info("Launching agent for issue #%s in %s", number, project)
    try:
        result = subprocess.run(
            args,
            cwd=project,
            check=False,
            capture_output=True,
            text=True,
            timeout=120,
        )
    except (subprocess.SubprocessError, OSError) as exc:
        logger.exception("Failed to launch t3 for issue #%s: %s", number, exc)
        return None
    if result.returncode != 0:
        logger.error(
            "t3 failed for issue #%s: %s",
            number,
            result.stderr.strip() or result.stdout.strip(),
        )
        return None
    thread_id = _parse_thread_id(result.stdout)
    if thread_id:
        logger.info("Launched thread %s for issue #%s", thread_id, number)
    else:
        logger.warning("t3 succeeded but no thread id parsed: %s", result.stdout.strip())
    return thread_id
