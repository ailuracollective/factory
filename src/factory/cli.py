"""CLI for the AI Factory."""

from __future__ import annotations

import json
import os
import subprocess
from pathlib import Path

import typer

from factory import __version__
from factory.agent_launcher import launch_agent
from factory.config import get_repo, load_config, save_config
from factory.doctor import run_doctor
from factory.repo_manager import add_repo as repo_add
from factory.repo_manager import list_repos, sync_labels


def _version_callback(value: bool) -> None:
    if value:
        typer.echo(f"factory {__version__}")
        raise typer.Exit()


app = typer.Typer(help="AI Factory CLI", add_completion=True)


@app.callback()
def main_callback(
    version: bool = typer.Option(
        False,
        "--version",
        callback=_version_callback,
        is_eager=True,
        help="Show version",
    ),
) -> None:
    pass
repo_app = typer.Typer(help="Manage repos")
secret_app = typer.Typer(help="Manage secret")
app.add_typer(repo_app, name="repo")
app.add_typer(secret_app, name="secret")


def _run(cmd: list[str]) -> subprocess.CompletedProcess[str]:
    return subprocess.run(cmd, capture_output=True, text=True, check=False)


@app.command()
def init(force: bool = typer.Option(False, "--force", help="Overwrite existing config")) -> None:
    """Initialize factory config."""
    from factory.config import CONFIG_DIR, CONFIG_FILE, default_config, save_config

    CONFIG_DIR.mkdir(parents=True, exist_ok=True)
    if CONFIG_FILE.exists() and not force:
        typer.echo(f"Config already exists at {CONFIG_FILE}. Use --force to overwrite.")
        raise typer.Exit(1)
    cfg = default_config()
    import secrets

    cfg["webhook"]["secret"] = secrets.token_hex(32)
    save_config(cfg)
    typer.echo(f"Config created at {CONFIG_FILE}")
    typer.echo(f"Secret: {cfg['webhook']['secret']}")
    typer.echo("Next: run 'factory doctor' to verify dependencies.")


@repo_app.command("add")
def repo_add_cmd(
    repo: str = typer.Argument(..., help="Repo name (owner/repo) or local path"),
) -> None:
    """Add a repo to the factory."""
    entry = repo_add(repo)
    if entry is None:
        typer.echo(f"Failed to add repo: {repo}")
        raise typer.Exit(1)
    config = load_config()
    url = config.get("webhook", {}).get("funnel_url", "")
    secret = config.get("webhook", {}).get("secret", "")
    if url and secret:
        from factory.webhook_manager import create_webhook as cw

        hook_id = cw(entry["github"], url, secret)
        if hook_id:
            entry["hook_id"] = hook_id
            from factory.config import add_repo as cfg_add

            cfg_add(config, entry)
            save_config(config)
            typer.echo(f"Webhook created (id={hook_id})")
    typer.echo(f"Repo added: {entry['name']} -> {entry['path']}")
    typer.echo(f"Labels synced: {sync_labels(entry['github'])}")


@repo_app.command("list")
def repo_list() -> None:
    """List configured repos."""
    repos = list_repos()
    if not repos:
        typer.echo("No repos configured. Use 'factory repo add <repo>'.")
        return
    for r in repos:
        name = r.get("name", "?")
        github = r.get("github", "?")
        enabled = r.get("enabled", True)
        typer.echo(f"{name:30} {github:40} enabled={enabled}")


@repo_app.command("remove")
def repo_remove_cmd(name: str = typer.Argument(..., help="Repo name")) -> None:
    """Remove a repo from the factory."""
    from factory.repo_manager import remove_repo as rr

    ok = rr(name)
    if not ok:
        typer.echo(f"Repo not found: {name}")
        raise typer.Exit(1)
    typer.echo(f"Repo removed: {name}")


@repo_app.command("sync")
def repo_sync(name: str = typer.Argument(..., help="Repo name")) -> None:
    """Sync labels for a repo."""
    repo = get_repo(load_config(), name)
    if repo is None:
        typer.echo(f"Repo not found: {name}")
        raise typer.Exit(1)
    n = sync_labels(repo["github"])
    typer.echo(f"Synced {n} labels for {repo['github']}")


@app.command()
def start(foreground: bool = typer.Option(False, "--foreground", help="Run in foreground")) -> None:
    """Start the factory daemon."""
    r = _run(["systemctl", "--user", "start", "factory"])
    if r.returncode == 0:
        typer.echo("Factory started.")
        return
    typer.echo("systemd not available.")
    if foreground:
        from factory.listener import main as listener_main

        listener_main()
        return
    raise typer.Exit(1)


@app.command()
def stop() -> None:
    """Stop the factory daemon."""
    r = _run(["systemctl", "--user", "stop", "factory"])
    if r.returncode == 0:
        typer.echo("Factory stopped.")
        return
    typer.echo(r.stderr.strip() or "Failed to stop.")
    raise typer.Exit(1)


@app.command()
def restart() -> None:
    """Restart the factory daemon."""
    r = _run(["systemctl", "--user", "restart", "factory"])
    if r.returncode == 0:
        typer.echo("Factory restarted.")
        return
    typer.echo(r.stderr.strip() or "Failed to restart.")
    raise typer.Exit(1)


@app.command()
def status() -> None:
    """Show factory status."""
    r = _run(["systemctl", "--user", "is-active", "factory"])
    typer.echo(f"Daemon: {r.stdout.strip() or r.stderr.strip()}")
    cfg = load_config()
    typer.echo(f"Repos: {len(cfg.get('repos', []))}")
    for repo in cfg.get("repos", []):
        name = repo.get("name")
        github = repo.get("github")
        enabled = repo.get("enabled", True)
        typer.echo(f"  - {name} ({github}) enabled={enabled}")
    url = cfg.get("webhook", {}).get("funnel_url", "")
    if url:
        typer.echo(f"Funnel: {url}")


@secret_app.command("rotate")
def secret_rotate() -> None:
    """Rotate the webhook secret."""
    import re
    import secrets

    config = load_config()
    new_secret = secrets.token_hex(32)
    config.setdefault("webhook", {})["secret"] = new_secret
    save_config(config)
    url = config["webhook"].get("funnel_url", "")
    for repo in config.get("repos", []):
        github = repo.get("github")
        hook_id = repo.get("hook_id")
        if github and hook_id and url:
            from factory.webhook_manager import update_webhook

            ok = update_webhook(github, hook_id, url, new_secret)
            typer.echo(f"  {github}: {'updated' if ok else 'FAILED'}")
    service = Path.home() / ".config" / "systemd" / "user" / "factory.service"
    if service.exists():
        text = service.read_text()
        pattern = r"Environment=ISSUE_FACTORY_WEBHOOK_SECRET=.*"
        replacement = f"Environment=ISSUE_FACTORY_WEBHOOK_SECRET={new_secret}"
        text = re.sub(pattern, replacement, text)
        service.write_text(text)
        _run(["systemctl", "--user", "daemon-reload"])
        _run(["systemctl", "--user", "restart", "factory"])
    typer.echo(f"New secret: {new_secret}")
    typer.echo("Save this secret somewhere safe.")


@app.command()
def logs(
    follow: bool = typer.Option(False, "-f", "--follow", help="Follow log output"),
    lines: int = typer.Option(50, "-n", "--lines", help="Number of lines"),
) -> None:
    """Show daemon logs."""
    cmd = ["journalctl", "--user", "-u", "factory", "--no-pager", "-n", str(lines)]
    if follow:
        cmd = ["journalctl", "--user", "-u", "factory", "-f"]
    os.execvp("journalctl", cmd)


@app.command()
def doctor() -> None:
    """Run diagnostics."""
    results = run_doctor()
    for name, ok, detail in results:
        status = "OK" if ok else "FAIL"
        typer.echo(f"[{status}] {name}: {detail}")
    failed = [r for r in results if not r[1]]
    if failed:
        raise typer.Exit(1)


@app.command()
def test(
    repo: str = typer.Argument(..., help="Repo name"),
    issue: int = typer.Argument(..., help="Issue number"),
) -> None:
    """Manually launch an agent for an issue."""
    config = load_config()
    repo_entry = get_repo(config, repo)
    if repo_entry is None:
        typer.echo(f"Repo not found: {repo}")
        raise typer.Exit(1)
    project = Path(repo_entry["path"])
    if not project.exists():
        typer.echo(f"Project path not found: {project}")
        raise typer.Exit(1)
    cmd = [
        "gh",
        "issue",
        "view",
        str(issue),
        "--repo",
        repo_entry["github"],
        "--json",
        "number,title,body,url,labels",
    ]
    r = _run(cmd)
    if r.returncode != 0:
        typer.echo(f"Failed to fetch issue: {r.stderr.strip()}")
        raise typer.Exit(1)
    try:
        issue_data = json.loads(r.stdout)
    except json.JSONDecodeError:
        typer.echo("Failed to parse issue JSON")
        raise typer.Exit(1) from None
    thread_id = launch_agent(issue_data, project, config)
    if thread_id:
        typer.echo(f"Launched thread: {thread_id}")
        return
    typer.echo("Failed to launch agent.")
    raise typer.Exit(1)


def main() -> None:
    """CLI entry point."""
    app()


if __name__ == "__main__":
    main()
