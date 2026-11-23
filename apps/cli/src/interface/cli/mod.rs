use crate::config::{get_repo, load_config, save_config};
use crate::repo_manager::{add_repo, list_repos, remove_repo, sync_labels};
use crate::webhook_manager::create_webhook;
use anyhow::Result;
use std::process::Command;
use tokio::runtime::Runtime;

pub(super) fn run(cmd: &[&str]) -> (i32, String) {
    match Command::new(cmd[0]).args(&cmd[1..]).output() {
        Ok(o) => {
            let out = String::from_utf8_lossy(&o.stdout);
            let err = String::from_utf8_lossy(&o.stderr);
            let msg = if out.trim().is_empty() {
                err.trim().to_string()
            } else {
                out.trim().to_string()
            };
            (o.status.code().unwrap_or(1), msg)
        }
        Err(_) => (127, "not found".to_string()),
    }
}

pub fn init(force: bool) -> Result<()> {
    use crate::config::{Config, config_dir, config_file, save_config};
    let secret = generate_secret();
    let dir = config_dir();
    std::fs::create_dir_all(&dir)?;
    let path = config_file();
    if path.exists() && !force {
        println!(
            "Config already exists at {}. Use --force to overwrite.",
            path.display()
        );
        return Ok(());
    }
    let mut cfg = Config::default();
    cfg.webhook.secret = secret.clone();
    save_config(&cfg)?;
    println!("Config created at {}", path.display());
    println!("Secret: {}", secret);
    println!("Next: run 'factory doctor' to verify dependencies.");
    Ok(())
}

fn generate_secret() -> String {
    // try openssl
    let (code, out) = run(&["openssl", "rand", "-hex", "32"]);
    if code == 0 && !out.is_empty() {
        return out;
    }
    // fallback: simple (should not happen; openssl is standard)
    "0000000000000000000000000000000000000000000000000000000000000000".to_string()
}

pub fn repo_add(name_or_path: &str) -> Result<()> {
    let entry = match add_repo(name_or_path) {
        Some(e) => e,
        None => {
            println!("Failed to add repo: {}", name_or_path);
            return Ok(());
        }
    };
    let config = load_config()?;
    let url = config.webhook.funnel_url.clone();
    let secret = config.webhook.secret.clone();
    if !url.is_empty()
        && !secret.is_empty()
        && let Some(hook_id) = create_webhook(&entry.github, &url, &secret)?
    {
        // update config with hook_id
        let mut cfg = load_config()?;
        if let Some(r) = cfg.repos.iter_mut().find(|r| r.name == entry.name) {
            r.hook_id = Some(hook_id);
        }
        save_config(&cfg)?;
        println!("Webhook created (id={})", hook_id);
    }
    println!("Repo added: {} -> {}", entry.name, entry.path);
    let n = sync_labels(&entry.github);
    println!("Labels synced: {}", n);
    Ok(())
}

pub fn repo_list() -> Result<()> {
    let repos = list_repos();
    if repos.is_empty() {
        println!("No repos configured. Use 'factory repo add <repo>'.");
        return Ok(());
    }
    for r in repos {
        println!("{:30} {:40} enabled={}", r.name, r.github, r.enabled);
    }
    Ok(())
}

pub fn repo_remove(name: &str) -> Result<()> {
    if remove_repo(name) {
        println!("Repo removed: {}", name);
    } else {
        println!("Repo not found: {}", name);
    }
    Ok(())
}

pub fn repo_sync(name: &str) -> Result<()> {
    let config = load_config()?;
    let repo = match get_repo(&config, name) {
        Some(r) => r.clone(),
        None => {
            println!("Repo not found: {}", name);
            return Ok(());
        }
    };
    let n = sync_labels(&repo.github);
    println!("Synced {} labels for {}", n, repo.github);
    Ok(())
}

pub(super) fn repo_check_results(
    repo: &crate::config::RepoEntry,
    webhook_expected: bool,
) -> Vec<(&'static str, String)> {
    let mut results = Vec::new();
    let path = std::path::Path::new(&repo.path);

    if path.is_dir() {
        results.push(("OK", format!("path: {}", repo.path)));
    } else {
        results.push(("FAIL", format!("path missing: {}", repo.path)));
    }

    if repo.github.is_empty() {
        results.push(("FAIL", "github slug missing".to_string()));
    } else {
        results.push(("OK", format!("github: {}", repo.github)));
        let (code, remote) = run(&["git", "-C", &repo.path, "remote", "get-url", "origin"]);
        if code == 0 && remote.contains(&repo.github) {
            results.push(("OK", "git remote matches github".to_string()));
        } else if code == 0 {
            results.push((
                "FAIL",
                format!("git remote does not match github: {remote}"),
            ));
        } else {
            results.push(("FAIL", format!("git remote unavailable: {remote}")));
        }
    }

    if webhook_expected {
        match repo.hook_id {
            Some(id) => results.push(("OK", format!("webhook hook_id: {id}"))),
            None => results.push((
                "FAIL",
                "webhook hook_id missing (funnel configured)".to_string(),
            )),
        }
    }

    if !repo.enabled {
        results.push(("WARN", "repo is disabled".to_string()));
    }

    results
}

pub fn repo_check(name: Option<&str>) -> Result<()> {
    let config = load_config()?;
    let repos: Vec<_> = match name {
        Some(n) => match get_repo(&config, n) {
            Some(r) => vec![r.clone()],
            None => {
                println!("Repo not found: {n}");
                anyhow::bail!("repo not found: {n}");
            }
        },
        None => {
            if config.repos.is_empty() {
                println!("No repos configured. Use 'factory repo add <repo>'.");
                return Ok(());
            }
            config.repos.clone()
        }
    };

    let webhook_expected =
        !config.webhook.funnel_url.is_empty() && !config.webhook.secret.is_empty();
    let mut failed = 0usize;

    for repo in &repos {
        println!("{} ({})", repo.name, repo.github);
        let mut repo_ok = true;
        for (level, msg) in repo_check_results(repo, webhook_expected) {
            println!("  [{level}] {msg}");
            if level == "FAIL" {
                repo_ok = false;
            }
        }
        if !repo_ok {
            failed += 1;
        }
    }

    if failed > 0 {
        anyhow::bail!("{failed} repo(s) misconfigured");
    }
    Ok(())
}

pub fn start(foreground: bool) -> Result<()> {
    let (code, _) = run(&["systemctl", "--user", "start", "factory"]);
    if code == 0 {
        println!("Factory started.");
        return Ok(());
    }
    println!("systemd not available.");
    if foreground {
        // run listener in foreground
        let config = load_config()?;
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(crate::listener::serve(config))?;
        return Ok(());
    }
    Ok(())
}

pub fn stop() -> Result<()> {
    let (code, msg) = run(&["systemctl", "--user", "stop", "factory"]);
    if code == 0 {
        println!("Factory stopped.");
    } else {
        println!(
            "{}",
            if msg.is_empty() {
                "Failed to stop."
            } else {
                &msg
            }
        );
    }
    Ok(())
}

pub fn restart() -> Result<()> {
    let (code, msg) = run(&["systemctl", "--user", "restart", "factory"]);
    if code == 0 {
        println!("Factory restarted.");
    } else {
        println!(
            "{}",
            if msg.is_empty() {
                "Failed to restart."
            } else {
                &msg
            }
        );
    }
    Ok(())
}

pub fn listen(port: Option<u16>) -> Result<()> {
    let mut config = load_config()?;
    if let Some(p) = port {
        config.webhook.port = p;
    }
    let rt = Runtime::new().unwrap();
    rt.block_on(crate::listener::serve(config))
}

pub fn mentions(once: bool, interval: Option<u64>) -> Result<()> {
    use crate::mentions::{
        AgentDispatcher, Engine, EventSource, EventStore, GhAuthorizer, GhEventSource,
        SqliteEventStore, parse_cancel_invocation, parse_invocation,
    };

    let config = load_config()?;
    if !config.mentions.enabled {
        println!("Mentions are disabled. Set mentions.enabled=true in the config.");
        return Ok(());
    }
    if config.mentions.usernames.is_empty() {
        println!("No trigger usernames configured. Set mentions.usernames in the config.");
        return Ok(());
    }
    let store = SqliteEventStore;
    let mut source = GhEventSource;
    let mut bg: Vec<std::thread::JoinHandle<()>> = Vec::new();
    loop {
        bg.retain(|h| !h.is_finished());
        let config = std::sync::Arc::new(load_config()?);
        let interval = interval
            .unwrap_or(config.mentions.poll_interval_secs)
            .max(5);
        let lookback = config.mentions.initial_lookback_secs;
        let dispatcher = AgentDispatcher {
            config: config.clone(),
        };
        let authorizer = GhAuthorizer;

        for repo in config.repos.iter().filter(|r| r.enabled) {
            if repo.github.trim().is_empty() {
                continue;
            }
            let cursor = store.cursor(&repo.github);
            let since = cursor.clone().unwrap_or_else(|| {
                crate::mentions::format_timestamp(crate::state::now().saturating_sub(lookback))
            });
            let events = match source.fetch(&repo.github, Some(&since)) {
                Ok(events) => events,
                Err(e) => {
                    tracing::error!(repo = %repo.github, "mention poll failed: {e}");
                    continue;
                }
            };
            let mut max_ts = crate::mentions::parse_timestamp(&since).unwrap_or(0);
            for event in &events {
                if let Some(ts) = crate::mentions::parse_timestamp(&event.updated_at) {
                    max_ts = max_ts.max(ts);
                }
                let is_cancel = parse_cancel_invocation(&event.body, &config.mentions).is_some();
                let is_launch =
                    !is_cancel && parse_invocation(&event.body, &config.mentions).is_some();
                if is_launch && !event.author_is_bot {
                    let event = event.clone();
                    let cfg = config.clone();
                    let disp = dispatcher.clone();
                    let auth = authorizer;
                    let st = store;
                    bg.push(std::thread::spawn(move || {
                        let engine = Engine::new(&cfg, &st, &disp, &auth);
                        let _ = engine.handle(&event, crate::state::now());
                    }));
                } else {
                    let engine = Engine::new(&config, &store, &dispatcher, &authorizer);
                    let _ = engine.handle(event, crate::state::now());
                }
            }
            let cursor_ts = cursor
                .as_deref()
                .and_then(crate::mentions::parse_timestamp)
                .unwrap_or(0);
            if max_ts > 0 && max_ts > cursor_ts {
                let next = crate::mentions::format_timestamp(max_ts.saturating_sub(5));
                let _ = store.set_cursor(&repo.github, &next);
            }
        }

        if once {
            for h in bg {
                let _ = h.join();
            }
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_secs(interval));
    }
}

pub fn status() -> Result<()> {
    let (code, msg) = run(&["systemctl", "--user", "is-active", "factory"]);
    println!("Daemon: {}", if msg.is_empty() { "unknown" } else { &msg });
    let _ = code;
    let config = load_config()?;
    println!("Repos: {}", config.repos.len());
    for repo in &config.repos {
        println!(
            "  - {} ({}) enabled={}",
            repo.name, repo.github, repo.enabled
        );
    }
    let url = &config.webhook.funnel_url;
    if !url.is_empty() {
        println!("Funnel: {}", url);
    }
    Ok(())
}

pub fn secret_rotate() -> Result<()> {
    let new_secret = generate_secret();
    let mut config = load_config()?;
    config.webhook.secret = new_secret.clone();
    save_config(&config)?;
    let url = config.webhook.funnel_url.clone();
    for repo in &config.repos {
        let github = repo.github.clone();
        if let Some(hook_id) = repo.hook_id
            && !github.is_empty()
            && !url.is_empty()
        {
            let ok = crate::webhook_manager::update_webhook(&github, hook_id, &url, &new_secret)
                .unwrap_or(false);
            println!("  {}: {}", github, if ok { "updated" } else { "FAILED" });
        }
    }
    // update service file
    let service = dirs::home_dir()
        .unwrap()
        .join(".config/systemd/user/factory.service");
    if service.exists() {
        let text = std::fs::read_to_string(&service)?;
        let re = regex::Regex::new(r"Environment=ISSUE_FACTORY_WEBHOOK_SECRET=.*").unwrap();
        let new_text = re.replace(
            &text,
            format!("Environment=ISSUE_FACTORY_WEBHOOK_SECRET={}", new_secret),
        );
        std::fs::write(&service, new_text.to_string())?;
        run(&["systemctl", "--user", "daemon-reload"]);
        run(&["systemctl", "--user", "restart", "factory"]);
    }
    println!("New secret: {}", new_secret);
    println!("Save this secret somewhere safe.");
    Ok(())
}

pub fn logs(follow: bool, lines: u32) -> Result<()> {
    let n = lines.to_string();
    let mut args = vec![
        "journalctl",
        "--user",
        "-u",
        "factory",
        "--no-pager",
        "-n",
        n.as_str(),
    ];
    if follow {
        args = vec!["journalctl", "--user", "-u", "factory", "-f"];
    }
    let _ = std::process::Command::new("journalctl")
        .args(&args[1..])
        .status();
    Ok(())
}

pub fn doctor() -> Result<()> {
    crate::doctor::run_doctor()
}

mod ops;
mod runner_cmd;

pub(crate) use ops::encode_path_segment;
pub use ops::{cancel, connect, run_prompt, test};
pub use runner_cmd::runner;

#[cfg(test)]
mod tests;
