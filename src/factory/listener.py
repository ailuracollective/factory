"""Webhook listener for the AI Factory."""

from __future__ import annotations

import argparse
import hashlib
import hmac
import json
import logging
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Any

from factory.agent_launcher import launch_agent
from factory.config import load_config
from factory.state import load_state, save_state

logger = logging.getLogger(__name__)

DEFAULT_STATE_FILE = Path.home() / ".config" / "factory" / "state.json"


def _slugify(text: str) -> str:
    """Convert text to a branch-safe slug."""
    import re

    slug = re.sub(r"[^a-zA-Z0-9]+", "-", text.strip().lower()).strip("-")
    return slug[:40] or "issue"


def verify_signature(secret: str, body: bytes, signature_header: str) -> bool:
    """Verify a GitHub webhook HMAC-SHA256 signature."""
    if not secret:
        return True
    if not signature_header.startswith("sha256="):
        return False
    expected = "sha256=" + hmac.new(secret.encode("utf-8"), body, hashlib.sha256).hexdigest()
    return hmac.compare_digest(expected, signature_header)


def _issue_matches_label(
    label: str, issue: dict[str, Any], action: str, payload: dict[str, Any]
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


def _handle_webhook_payload(
    config: dict[str, Any],
    processed: set[str],
    threads: dict[str, str],
    payload: dict[str, Any],
) -> str:  # noqa: C901, PLR0911
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
    defaults = config.get("defaults", {})
    label = defaults.get("label", "status/ready")
    if label and not _issue_matches_label(label, issue, action, payload):
        return "ignored: label mismatch"
    repo = str((payload.get("repository") or {}).get("full_name", "")).strip()
    if not repo:
        return "ignored: no repo"
    repos = config.get("repos", [])
    # single-repo filter if configured
    configured_repo = config.get("repo", "")
    if configured_repo and repo != configured_repo:
        return "ignored: repo not configured"
    key = f"{repo}#{number}"
    if key in processed:
        return f"ignored: already processed {key}"
    projects_base = Path(
        config.get("projects_base", str(Path.home() / "projects" / "ailuracollective"))
    )
    # find project path
    project = None
    for r in repos:
        if r.get("github") == repo or r.get("name") == repo.split("/")[-1]:
            if r.get("path"):
                project = Path(r["path"])
            break
    if project is None:
        project = projects_base / repo.split("/")[-1]
    if not project.exists():
        return f"ignored: no project for {repo}"
    thread_id = launch_agent(issue, project, config)
    processed.add(key)
    if thread_id:
        threads[key] = thread_id
    save_state(processed, threads)
    return f"launched {key}"


class _WebhookHandler(BaseHTTPRequestHandler):
    """HTTP handler for GitHub issue webhooks."""

    config: dict[str, Any]
    processed: set[str]
    threads: dict[str, str]
    lock: threading.Lock

    def do_GET(self) -> None:  # noqa: N802
        if self.path.rstrip("/") == "/health":
            self._respond(200, {"status": "ok"})
            return
        self._respond(404, {"error": "not found"})

    def do_POST(self) -> None:  # noqa: N802
        length = int(self.headers.get("Content-Length", "0"))
        body = self.rfile.read(length)
        webhook = self.config.get("webhook", {})
        secret = webhook.get("secret", "")
        signature = self.headers.get("X-Hub-Signature-256", "")
        if not verify_signature(secret, body, signature):
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

    def log_message(self, fmt: str, *args: Any) -> None:
        logger.info("webhook: %s", fmt % args if args else fmt)


def serve_webhook(config: dict[str, Any]) -> None:
    """Run an HTTP server that receives GitHub issue webhooks."""
    processed, threads = load_state()
    handler = _WebhookHandler
    handler.config = config
    handler.processed = processed
    handler.threads = threads
    handler.lock = threading.Lock()
    port = config.get("webhook", {}).get("port", 8787)
    server = ThreadingHTTPServer(("0.0.0.0", port), handler)  # noqa: S104
    logger.info(
        "Webhook listener on http://0.0.0.0:%s (secret=%s)",
        port,
        bool(config.get("webhook", {}).get("secret")),
    )
    try:
        server.serve_forever()
    finally:
        server.server_close()


def main() -> None:
    """Entry point for the AI Factory listener."""
    parser = argparse.ArgumentParser(description="AI Factory webhook listener.")
    parser.add_argument(
        "--port", type=int, default=None, help="Webhook port (default from config)."
    )
    args = parser.parse_args()

    logging.basicConfig(
        level=logging.INFO,
        format="%(asctime)s - %(name)s - %(levelname)s - %(message)s",
    )
    config = load_config()
    if args.port is not None:
        config.setdefault("webhook", {})["port"] = args.port
    secret = config.get("webhook", {}).get("secret", "")
    if not secret:
        logger.warning(
            "ISSUE_FACTORY_WEBHOOK_SECRET not set; accepting unsigned payloads (dev only)"
        )
    serve_webhook(config)
