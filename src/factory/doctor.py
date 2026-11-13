"""Diagnostics for the AI Factory."""

from __future__ import annotations

import os
import subprocess

from factory.config import load_config


def _run(cmd: list[str]) -> tuple[int, str]:
    try:
        r = subprocess.run(cmd, capture_output=True, text=True, check=False, timeout=15)
        return r.returncode, (r.stdout.strip() or r.stderr.strip())
    except FileNotFoundError:
        return 127, "not found"
    except Exception as exc:
        return 1, str(exc)


def run_doctor() -> list[tuple[str, bool, str]]:
    """Run diagnostic checks. Returns list of (name, ok, detail)."""
    results: list[tuple[str, bool, str]] = []

    # gh
    code, out = _run(["gh", "auth", "status"])
    results.append(("gh auth", code == 0, out.splitlines()[0] if out else "ok"))

    # t3
    code, out = _run(["t3", "service", "status"])
    results.append(("t3 service", code == 0, out.splitlines()[0] if out else "ok"))

    # tailscale
    code, out = _run(["tailscale", "status"])
    results.append(("tailscale", code == 0, out.splitlines()[0] if out else "ok"))

    # funnel
    code, out = _run(["tailscale", "funnel", "status"])
    results.append(("tailscale funnel", code == 0, out.splitlines()[0] if out else "ok"))

    # python
    results.append(("python", True, os.sys.version.split()[0]))

    # config
    try:
        cfg = load_config()
        repos = cfg.get("repos", [])
        results.append(("config", True, f"{len(repos)} repos configured"))
    except Exception as exc:
        results.append(("config", False, str(exc)))

    # funnel url reachable
    url = load_config().get("webhook", {}).get("funnel_url", "")
    if url:
        code, out = _run(["curl", "-s", "-o", "/dev/null", "-w", "%{http_code}", url + "/health"])
        results.append(("funnel reachable", out.strip() == "200", out.strip()))

    return results
