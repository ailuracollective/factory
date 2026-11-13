"""CLI for the AI Factory."""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
from pathlib import Path

from factory import __version__
from factory.agent_launcher import launch_agent
from factory.config import add_repo as cfg_add_repo
from factory.config import get_repo, load_config, save_config
from factory.doctor import run_doctor
from factory.repo_manager import add_repo as repo_add
from factory.repo_manager import list_repos, sync_labels
from factory.repo_manager import remove_repo as repo_remove
from factory.webhook_manager import create_webhook


def _run(cmd: list[str]) -> subprocess.CompletedProcess[str]:
    return subprocess.run(cmd, capture_output=True, text=True, check=False)


def cmd_init(args: argparse.Namespace) -> int:
    """Initialize factory config."""
    from factory.config import CONFIG_DIR, CONFIG_FILE, default_config, save_config

    CONFIG_DIR.mkdir(parents=True, exist_ok=True)
    if CONFIG_FILE.exists() and not args.force:
        print(f"Config already exists at {CONFIG_FILE}. Use --force to overwrite.")
        return 1
    cfg = default_config()
    # generate secret
    import secrets

    cfg["webhook"]["secret"] = secrets.token_hex(32)
    save_config(cfg)
    print(f"Config created at {CONFIG_FILE}")
    print(f"Secret: {cfg['webhook']['secret']}")
    print("Next: run 'factory doctor' to verify dependencies.")
    return 0


def cmd_repo_add(args: argparse.Namespace) -> int:
    """Add a repo to the factory."""
    entry = repo_add(args.repo)
    if entry is None:
        print(f"Failed to add repo: {args.repo}")
        return 1
    # create webhook
    config = load_config()
    url = config.get("webhook", {}).get("funnel_url", "")
    secret = config.get("webhook", {}).get("secret", "")
    if url and secret:
        hook_id = create_webhook(entry["github"], url, secret)
        if hook_id:
            entry["hook_id"] = hook_id
            cfg_add_repo(config, entry)
            save_config(config)
            print(f"Webhook created (id={hook_id})")
    print(f"Repo added: {entry['name']} -> {entry['path']}")
    print(f"Labels synced: {sync_labels(entry['github'])}")
    return 0


def cmd_repo_list(args: argparse.Namespace) -> int:
    """List configured repos."""
    repos = list_repos()
    if not repos:
        print("No repos configured. Use 'factory repo add <repo>'.")
        return 0
    for r in repos:
        print(f"{r.get('name', '?'):30} {r.get('github', '?'):40} enabled={r.get('enabled', True)}")
    return 0


def cmd_repo_remove(args: argparse.Namespace) -> int:
    """Remove a repo from the factory."""
    ok = repo_remove(args.name)
    if not ok:
        print(f"Repo not found: {args.name}")
        return 1
    print(f"Repo removed: {args.name}")
    return 0


def cmd_repo_sync(args: argparse.Namespace) -> int:
    """Sync labels for a repo."""
    repo = get_repo(load_config(), args.name)
    if repo is None:
        print(f"Repo not found: {args.name}")
        return 1
    n = sync_labels(repo["github"])
    print(f"Synced {n} labels for {repo['github']}")
    return 0


def cmd_start(args: argparse.Namespace) -> int:
    """Start the factory daemon."""
    # delegate to systemd if available
    r = _run(["systemctl", "--user", "start", "factory"])
    if r.returncode == 0:
        print("Factory started.")
        return 0
    print("systemd not available; run 'factory-listen' manually or use --foreground")
    if getattr(args, "foreground", False):
        from factory.listener import main as listener_main

        listener_main()
        return 0
    return 1


def cmd_stop(args: argparse.Namespace) -> int:
    """Stop the factory daemon."""
    r = _run(["systemctl", "--user", "stop", "factory"])
    if r.returncode == 0:
        print("Factory stopped.")
        return 0
    print(r.stderr.strip() or "Failed to stop.")
    return 1


def cmd_restart(args: argparse.Namespace) -> int:
    """Restart the factory daemon."""
    r = _run(["systemctl", "--user", "restart", "factory"])
    if r.returncode == 0:
        print("Factory restarted.")
        return 0
    print(r.stderr.strip() or "Failed to restart.")
    return 1


def cmd_status(args: argparse.Namespace) -> int:
    """Show factory status."""
    r = _run(["systemctl", "--user", "is-active", "factory"])
    print(f"Daemon: {r.stdout.strip() or r.stderr.strip()}")
    cfg = load_config()
    print(f"Repos: {len(cfg.get('repos', []))}")
    for repo in cfg.get("repos", []):
        print(f"  - {repo.get('name')} ({repo.get('github')}) enabled={repo.get('enabled', True)}")
    url = cfg.get("webhook", {}).get("funnel_url", "")
    if url:
        print(f"Funnel: {url}")
    return 0


def cmd_secret_rotate(args: argparse.Namespace) -> int:
    """Rotate the webhook secret."""
    import secrets

    config = load_config()
    new_secret = secrets.token_hex(32)
    config.setdefault("webhook", {})["secret"] = new_secret
    save_config(config)
    # update webhooks
    url = config["webhook"].get("funnel_url", "")
    for repo in config.get("repos", []):
        github = repo.get("github")
        hook_id = repo.get("hook_id")
        if github and hook_id and url:
            from factory.webhook_manager import update_webhook

            ok = update_webhook(github, hook_id, url, new_secret)
            print(f"  {github}: {'updated' if ok else 'FAILED'}")
    # update service file if exists
    service = Path.home() / ".config" / "systemd" / "user" / "factory.service"
    if service.exists():
        text = service.read_text()
        import re

        text = re.sub(
            r"Environment=ISSUE_FACTORY_WEBHOOK_SECRET=.*",
            f"Environment=ISSUE_FACTORY_WEBHOOK_SECRET={new_secret}",
            text,
        )
        service.write_text(text)
        _run(["systemctl", "--user", "daemon-reload"])
        _run(["systemctl", "--user", "restart", "factory"])
    print(f"New secret: {new_secret}")
    print("Save this secret somewhere safe.")
    return 0


def cmd_logs(args: argparse.Namespace) -> int:
    """Show daemon logs."""
    cmd = ["journalctl", "--user", "-u", "factory", "--no-pager"]
    if getattr(args, "follow", False):
        cmd = ["journalctl", "--user", "-u", "factory", "-f"]
    if getattr(args, "lines", None):
        cmd += ["-n", str(args.lines)]
    os.execvp("journalctl", cmd)
    return 0


def cmd_doctor(args: argparse.Namespace) -> int:
    """Run diagnostics."""
    results = run_doctor()
    for name, ok, detail in results:
        status = "OK" if ok else "FAIL"
        print(f"[{status}] {name}: {detail}")
    failed = [r for r in results if not r[1]]
    return 1 if failed else 0


def cmd_test(args: argparse.Namespace) -> int:
    """Manually launch an agent for an issue."""
    config = load_config()
    repo = get_repo(config, args.repo)
    if repo is None:
        print(f"Repo not found: {args.repo}")
        return 1
    project = Path(repo["path"])
    if not project.exists():
        print(f"Project path not found: {project}")
        return 1
    # fetch issue
    r = _run(
        [
            "gh",
            "issue",
            "view",
            str(args.issue),
            "--repo",
            repo["github"],
            "--json",
            "number,title,body,url,labels",
        ]
    )
    if r.returncode != 0:
        print(f"Failed to fetch issue: {r.stderr.strip()}")
        return 1
    try:
        issue = json.loads(r.stdout)
    except json.JSONDecodeError:
        print("Failed to parse issue JSON")
        return 1
    thread_id = launch_agent(issue, project, config)
    if thread_id:
        print(f"Launched thread: {thread_id}")
        return 0
    print("Failed to launch agent.")
    return 1


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="factory", description="AI Factory CLI")
    parser.add_argument("--version", action="version", version=f"factory {__version__}")
    sub = parser.add_subparsers(dest="command", required=True)

    # init
    p = sub.add_parser("init", help="Initialize factory config")
    p.add_argument("--force", action="store_true", help="Overwrite existing config")
    p.set_defaults(func=cmd_init)

    # repo
    repo_parser = sub.add_parser("repo", help="Manage repos")
    repo_sub = repo_parser.add_subparsers(dest="repo_command", required=True)

    p = repo_sub.add_parser("add", help="Add a repo")
    p.add_argument("repo", help="Repo name (owner/repo) or URL")
    p.set_defaults(func=cmd_repo_add)

    p = repo_sub.add_parser("list", help="List repos")
    p.set_defaults(func=cmd_repo_list)

    p = repo_sub.add_parser("remove", help="Remove a repo")
    p.add_argument("name", help="Repo name")
    p.set_defaults(func=cmd_repo_remove)

    p = repo_sub.add_parser("sync", help="Sync labels for a repo")
    p.add_argument("name", help="Repo name")
    p.set_defaults(func=cmd_repo_sync)

    # start/stop/restart
    for name, func, help_text in [
        ("start", cmd_start, "Start daemon"),
        ("stop", cmd_stop, "Stop daemon"),
        ("restart", cmd_restart, "Restart daemon"),
    ]:
        p = sub.add_parser(name, help=help_text)
        p.set_defaults(func=func)

    # status
    p = sub.add_parser("status", help="Show status")
    p.set_defaults(func=cmd_status)

    # secret
    p = sub.add_parser("secret", help="Manage secret")
    secret_sub = p.add_subparsers(dest="secret_command", required=True)
    sp = secret_sub.add_parser("rotate", help="Rotate webhook secret")
    sp.set_defaults(func=cmd_secret_rotate)

    # logs
    p = sub.add_parser("logs", help="Show daemon logs")
    p.add_argument("-f", "--follow", action="store_true", help="Follow log output")
    p.add_argument("-n", "--lines", type=int, default=50, help="Number of lines")
    p.set_defaults(func=cmd_logs)

    # doctor
    p = sub.add_parser("doctor", help="Run diagnostics")
    p.set_defaults(func=cmd_doctor)

    # test
    p = sub.add_parser("test", help="Manually launch agent for an issue")
    p.add_argument("repo", help="Repo name")
    p.add_argument("issue", type=int, help="Issue number")
    p.set_defaults(func=cmd_test)

    return parser


def main() -> None:
    """CLI entry point."""
    parser = build_parser()
    args = parser.parse_args()
    func = getattr(args, "func", None)
    if func is None:
        parser.print_help()
        sys.exit(1)
    sys.exit(func(args))


if __name__ == "__main__":
    main()
