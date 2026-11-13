"""GitHub issue listener that launches T3 Code agents (AI factory)."""

from __future__ import annotations

import argparse
import hashlib
import hmac
import json
import logging
import os
import re
import subprocess
import threading
from dataclasses import dataclass
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Any

logger = logging.getLogger(__name__)

DEFAULT_STATE_FILE = ".issue_factory_state.json"
DEFAULT_BRANCH = "main"


@dataclass
class IssueFactoryConfig:
    """Configuration for the issue factory listener."""

    repo: str = ""
    project: Path | None = None
    projects_base: Path = Path.home() / "projects" / "ailuracollective"
    label: str = "status/ready"
    state_file: Path = Path(DEFAULT_STATE_FILE)
    t3_binary: str = "t3"
    gh_binary: str = "gh"
    base_branch: str = DEFAULT_BRANCH
    webhook_secret: str = ""
    webhook_port: int = 8787
    provider: str = "opencode"
    model: str = "opencode-go/beam-free"

    @classmethod
    def from_env(cls) -> IssueFactoryConfig:
        """Build a configuration from environment variables."""
        project_raw = os.environ.get("ISSUE_FACTORY_PROJECT", "").strip()
        project = Path(project_raw).resolve() if project_raw else None
        repo = os.environ.get("ISSUE_FACTORY_REPO", "").strip()
        if not repo and project is not None:
            repo = _detect_repo(project, "gh")
        base_raw = os.environ.get("ISSUE_FACTORY_PROJECTS_BASE", "").strip()
        if base_raw:
            projects_base = Path(base_raw).resolve()
        else:
            projects_base = Path.home() / "projects" / "ailuracollective"
        label = os.environ.get("ISSUE_FACTORY_LABEL", "status/ready").strip()
        state_file = Path(os.environ.get("ISSUE_FACTORY_STATE", DEFAULT_STATE_FILE))
        t3_binary = os.environ.get("ISSUE_FACTORY_T3", "t3")
        gh_binary = os.environ.get("ISSUE_FACTORY_GH", "gh")
        base_branch = os.environ.get("ISSUE_FACTORY_BASE_BRANCH", DEFAULT_BRANCH)
        webhook_secret = os.environ.get("ISSUE_FACTORY_WEBHOOK_SECRET", "").strip()
        port_raw = os.environ.get("ISSUE_FACTORY_WEBHOOK_PORT", "8787")
        try:
            webhook_port = int(port_raw)
        except ValueError:
            webhook_port = 8787
        provider = os.environ.get("ISSUE_FACTORY_PROVIDER", "opencode").strip() or "opencode"
        model_raw = os.environ.get("ISSUE_FACTORY_MODEL", "opencode-go/beam-free")
        model = model_raw.strip() or "opencode-go/beam-free"
        return cls(
            repo=repo,
            project=project,
            projects_base=projects_base,
            label=label,
            state_file=state_file,
            t3_binary=t3_binary,
            gh_binary=gh_binary,
            base_branch=base_branch,
            webhook_secret=webhook_secret,
            webhook_port=webhook_port,
            provider=provider,
            model=model,
        )


def _detect_repo(project: Path, gh_binary: str) -> str:
    """Detect the GitHub repo (owner/name) from the project directory."""
    try:
        result = subprocess.run(  # noqa: S603
            [gh_binary, "repo", "view", "--json", "nameWithOwner", "-q", ".nameWithOwner"],
            cwd=project,
            check=True,
            capture_output=True,
            text=True,
            timeout=30,
        )
    except (subprocess.SubprocessError, OSError):
        logger.warning("Could not detect repo from %s", project)
        return ""
    return result.stdout.strip()


def load_state(path: Path) -> tuple[set[str], dict[str, str]]:
    """Load processed issue keys and thread ids from a state file."""
    if not path.exists():
        return set(), {}
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        logger.warning("Could not read state file %s; starting fresh", path)
        return set(), {}
    if not isinstance(data, dict):
        return set(), {}
    raw_ids = data.get("processed", [])
    processed: set[str] = set()
    if isinstance(raw_ids, list):
        for value in raw_ids:
            if isinstance(value, str):
                processed.add(value)
            elif isinstance(value, int):
                processed.add(str(value))
    raw_threads = data.get("threads", {})
    threads: dict[str, str] = {}
    if isinstance(raw_threads, dict):
        for key, value in raw_threads.items():
            threads[str(key)] = str(value)
    return processed, threads


def save_state(path: Path, processed: set[str], threads: dict[str, str]) -> None:
    """Persist processed issue numbers and thread ids to a state file."""
    payload: dict[str, Any] = {"processed": sorted(processed)}
    if threads:
        payload["threads"] = {str(key): value for key, value in threads.items()}
    path.write_text(json.dumps(payload, indent=2), encoding="utf-8")


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


def launch_agent(config: IssueFactoryConfig, issue: dict[str, Any], project: Path) -> str | None:
    """Launch a T3 Code agent thread for the given issue."""
    number = issue.get("number")
    if not isinstance(number, int):
        return None
    title = str(issue.get("title", "")).strip()
    branch = f"issue-{number}-{_slugify(title)}"
    thread_title = f"Fix issue #{number}: {title}"[:80]
    prompt = build_prompt(issue)
    args = [
        config.t3_binary,
        "thread",
        "start",
        str(project),
        prompt,
        "--title",
        thread_title,
        "--provider",
        config.provider,
        "--model",
        config.model,
        "--worktree",
        config.base_branch,
        "--branch",
        branch,
        "--json",
    ]
    logger.info("Launching agent for issue #%s", number)
    try:
        result = subprocess.run(  # noqa: S603
            args,
            cwd=config.project,
            check=False,
            capture_output=True,
            text=True,
            timeout=120,
        )
    except (subprocess.SubprocessError, OSError):
        logger.exception("Failed to launch t3 for issue #%s", number)
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


def verify_signature(secret: str, body: bytes, signature_header: str) -> bool:
    """Verify a GitHub webhook HMAC-SHA256 signature."""
    if not secret:
        return True
    if not signature_header.startswith("sha256="):
        return False
    expected = "sha256=" + hmac.new(secret.encode("utf-8"), body, hashlib.sha256).hexdigest()
    return hmac.compare_digest(expected, signature_header)


def _handle_webhook_payload(  # noqa: C901, PLR0911
    config: IssueFactoryConfig,
    processed: set[str],
    threads: dict[str, str],
    payload: dict[str, Any],
) -> str:
    """Process a single webhook payload. Returns a short status string."""
    action = str(payload.get("action", ""))
    issue = payload.get("issue")
    if not isinstance(issue, dict):
        return "ignored: no issue"
    if action not in {"opened", "reopened", "labeled"}:
        return f"ignored: action={action}"
    number = issue.get("number")
    if not isinstance(number, int):
        return "ignored: bad number"
    repo = str((payload.get("repository") or {}).get("full_name", "")).strip()
    if not repo:
        repo = config.repo
    if config.repo and repo != config.repo:
        return "ignored: repo not configured"
    if config.label and not _issue_matches_label(config.label, issue, action, payload):
        return "ignored: label mismatch"
    key = f"{repo}#{number}"
    if key in processed:
        return f"ignored: already processed {key}"
    if config.project is not None:
        project = config.project
    else:
        project = config.projects_base / repo.split("/")[-1]
    if not project.exists():
        return f"ignored: no project for {repo}"
    thread_id = launch_agent(config, issue, project)
    processed.add(key)
    if thread_id:
        threads[key] = thread_id
    save_state(config.state_file, processed, threads)
    return f"launched {key}"


def _issue_matches_label(
    label: str,
    issue: dict[str, Any],
    action: str,
    payload: dict[str, Any],
) -> bool:
    """Check whether an issue/payload matches the configured label filter."""
    if action == "labeled":
        payload_label = payload.get("label")
        name = payload_label.get("name") if isinstance(payload_label, dict) else None
        return name == label
    labels = issue.get("labels")
    if not isinstance(labels, list):
        return False
    names = [
        str(item.get("name"))
        for item in labels
        if isinstance(item, dict) and item.get("name") is not None
    ]
    return label in names


class _WebhookHandler(BaseHTTPRequestHandler):
    """HTTP handler for GitHub issue webhooks."""

    config: IssueFactoryConfig
    processed: set[int]
    threads: dict[int, str]
    lock: Any

    def do_GET(self) -> None:
        if self.path.rstrip("/") == "/health":
            self._respond(200, {"status": "ok"})
            return
        self._respond(404, {"error": "not found"})

    def do_POST(self) -> None:
        length = int(self.headers.get("Content-Length", "0"))
        body = self.rfile.read(length)
        signature = self.headers.get("X-Hub-Signature-256", "")
        if not verify_signature(self.config.webhook_secret, body, signature):
            self._respond(401, {"error": "bad signature"})
            return
        event = self.headers.get("X-GitHub-Event", "")
        if event and event != "issues":
            self._respond(202, {"status": "ignored", "reason": f"event={event}"})
            return
        try:
            payload = json.loads(body.decode("utf-8"))
        except (json.JSONDecodeError, UnicodeDecodeError):
            self._respond(400, {"error": "invalid json"})
            return
        if not isinstance(payload, dict):
            self._respond(400, {"error": "invalid payload"})
            return

        def worker() -> None:
            with self.lock:
                status = _handle_webhook_payload(self.config, self.processed, self.threads, payload)
            logger.info("webhook result: %s", status)

        threading.Thread(target=worker, daemon=True).start()
        self._respond(202, {"status": "accepted"})

    def _respond(self, code: int, data: dict[str, Any]) -> None:
        raw = json.dumps(data).encode("utf-8")
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(raw)))
        self.end_headers()
        self.wfile.write(raw)


def serve_webhook(config: IssueFactoryConfig) -> None:
    """Run an HTTP server that receives GitHub issue webhooks."""
    processed, threads = load_state(config.state_file)
    handler = _WebhookHandler
    handler.config = config
    handler.processed = processed
    handler.threads = threads
    handler.lock = threading.Lock()
    server = ThreadingHTTPServer(("0.0.0.0", config.webhook_port), handler)  # noqa: S104
    logger.info(
        "Webhook listener on http://0.0.0.0:%s (secret=%s)",
        config.webhook_port,
        bool(config.webhook_secret),
    )
    try:
        server.serve_forever()
    finally:
        server.server_close()


def main() -> None:
    """Entry point for the issue factory listener."""
    parser = argparse.ArgumentParser(description="Listen for GitHub issues and launch T3 agents.")
    parser.add_argument("--port", type=int, default=None, help="Webhook port (default from env).")
    args = parser.parse_args()

    logging.basicConfig(
        level=logging.INFO,
        format="%(asctime)s - %(name)s - %(levelname)s - %(message)s",
    )
    config = IssueFactoryConfig.from_env()
    if args.port is not None:
        config.webhook_port = args.port
    logger.info(
        "Issue factory starting for repo=%s project=%s",
        config.repo or "<unset>",
        config.project,
    )
    if not config.webhook_secret:
        logger.warning(
            "ISSUE_FACTORY_WEBHOOK_SECRET not set; accepting unsigned payloads (dev only)"
        )
    serve_webhook(config)


if __name__ == "__main__":
    main()
