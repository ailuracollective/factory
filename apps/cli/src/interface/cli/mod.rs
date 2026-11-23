use crate::config::{FactoryRepoConfig, get_repo, load_config, load_factory_config, save_config};
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

pub fn start() -> Result<()> {
    let (code, _) = run(&["systemctl", "--user", "start", "factory"]);
    if code == 0 {
        println!("Factory started.");
        return Ok(());
    }
    println!(
        "systemd not available. Run 'factory listen' to start the listener in the foreground."
    );
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
pub mod runner_cmd;

pub(crate) use ops::encode_path_segment;
pub use ops::{cancel, connect, run_prompt, test};
pub use runner_cmd::runner;

#[cfg(test)]
mod tests;

pub fn providers() -> Result<()> {
    let (code, out) = run(&["opencode", "models"]);
    if code != 0 {
        println!("Failed to list models: {}", out);
        return Ok(());
    }
    let mut providers: Vec<String> = out
        .lines()
        .filter_map(|l| l.split('/').next())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    providers.sort();
    providers.dedup();
    for p in providers {
        println!("{}", p);
    }
    Ok(())
}

pub fn models(provider: Option<&str>) -> Result<()> {
    let (code, out) = run(&["opencode", "models"]);
    if code != 0 {
        println!("Failed to list models: {}", out);
        return Ok(());
    }
    for line in out.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(p) = provider
            && !line.starts_with(&format!("{}/", p))
        {
            continue;
        }
        println!("{}", line);
    }
    Ok(())
}

pub fn config_get() -> Result<()> {
    let mut cfg = load_config()?;
    // Apply repo-level factory.json if present in cwd or an ancestor.
    if let Some(repo_cfg) = find_factory_config() {
        if let Some(d) = repo_cfg.defaults {
            if !d.provider.is_empty() {
                cfg.defaults.provider = d.provider;
            }
            if !d.model.is_empty() {
                cfg.defaults.model = d.model;
            }
            if !d.base_branch.is_empty() {
                cfg.defaults.base_branch = d.base_branch;
            }
            if !d.label.is_empty() {
                cfg.defaults.label = d.label;
            }
            if d.max_attempts > 0 {
                cfg.defaults.max_attempts = d.max_attempts;
            }
        }
        if let Some(m) = repo_cfg.mentions {
            if m.enabled {
                cfg.mentions.enabled = true;
            }
            if !m.usernames.is_empty() {
                cfg.mentions.usernames = m.usernames;
            }
            if !m.command.is_empty() {
                cfg.mentions.command = m.command;
            }
            if !m.allowed_authors.is_empty() {
                cfg.mentions.allowed_authors = m.allowed_authors;
            }
            if m.daily_quota > 0 {
                cfg.mentions.daily_quota = m.daily_quota;
            }
            cfg.mentions.report = m.report;
            for u in m.usernames_extra {
                if !cfg.mentions.usernames.contains(&u) {
                    cfg.mentions.usernames.push(u);
                }
            }
        }
    }
    println!("provider: {}", cfg.defaults.provider);
    println!("model:    {}", cfg.defaults.model);
    println!("base_branch: {}", cfg.defaults.base_branch);
    println!("label: {}", cfg.defaults.label);
    println!("max_attempts: {}", cfg.defaults.max_attempts);
    println!("mentions.enabled: {}", cfg.mentions.enabled);
    println!("mentions.usernames: {}", cfg.mentions.usernames.join(", "));
    println!("mentions.usernames_extra: {}", cfg.mentions.usernames_extra.join(", "));
    println!("mentions.command: {}", cfg.mentions.command);
    println!("mentions.daily_quota: {}", cfg.mentions.daily_quota);
    println!("mentions.report: {}", cfg.mentions.report);
    println!("webhook.port: {}", cfg.webhook.port);
    println!("webhook.bind: {}", cfg.webhook.bind);
    Ok(())
}

fn find_factory_config() -> Option<FactoryRepoConfig> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        if let Some(cfg) = load_factory_config(&dir) {
            return Some(cfg);
        }
        if !dir.pop() {
            break;
        }
    }
    None
}

pub fn config_set(key: &str, value: &str) -> Result<()> {
    // validate against opencode models
    let (code, out) = run(&["opencode", "models"]);
    let models: Vec<&str> = if code == 0 {
        out.lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .collect()
    } else {
        Vec::new()
    };
    let mut cfg = load_config()?;
    match key {
        "provider" => {
            let valid = models.iter().any(|m| m.split('/').next() == Some(value));
            if !valid {
                println!(
                    "Unknown provider: {}. Available: {}",
                    value,
                    models
                        .iter()
                        .map(|m| m.split('/').next().unwrap_or(""))
                        .collect::<std::collections::BTreeSet<_>>()
                        .into_iter()
                        .collect::<Vec<_>>()
                        .join(", ")
                );
                return Ok(());
            }
            cfg.defaults.provider = value.to_string();
        }
        "model" => {
            if !models.contains(&value) {
                println!("Unknown model: {}. Use 'factory models' to list.", value);
                return Ok(());
            }
            cfg.defaults.model = value.to_string();
        }
        "mentions.usernames" => {
            cfg.mentions.usernames = value
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect();
        }
        _ => {
            println!("Unknown key: {}. Use 'provider', 'model' or 'mentions.usernames'.", key);
            return Ok(());
        }
    }
    save_config(&cfg)?;
    println!("Set {} = {}", key, value);
    Ok(())
}
