use anyhow::Result;
use crate::config::{get_repo, load_config, save_config};
use crate::doctor::run_doctor;
use crate::repo_manager::{add_repo, list_repos, remove_repo, sync_labels};
use crate::webhook_manager::create_webhook;
use std::process::Command;
use tokio::runtime::Runtime;

fn run(cmd: &[&str]) -> (i32, String) {
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
    use crate::config::{config_dir, config_file, save_config, Config};
    let secret = generate_secret();
    let dir = config_dir();
    std::fs::create_dir_all(&dir)?;
    let path = config_file();
    if path.exists() && !force {
        println!("Config already exists at {}. Use --force to overwrite.", path.display());
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
    if !url.is_empty() && !secret.is_empty() {
        if let Some(hook_id) = create_webhook(&entry.github, &url, &secret)? {
            // update config with hook_id
            let mut cfg = load_config()?;
            if let Some(r) = cfg.repos.iter_mut().find(|r| r.name == entry.name) {
                r.hook_id = Some(hook_id);
            }
            save_config(&cfg)?;
            println!("Webhook created (id={})", hook_id);
        }
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
        println!(
            "{:30} {:40} enabled={}",
            r.name, r.github, r.enabled
        );
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
        println!("{}", if msg.is_empty() { "Failed to stop." } else { &msg });
    }
    Ok(())
}

pub fn restart() -> Result<()> {
    let (code, msg) = run(&["systemctl", "--user", "restart", "factory"]);
    if code == 0 {
        println!("Factory restarted.");
    } else {
        println!("{}", if msg.is_empty() { "Failed to restart." } else { &msg });
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
        println!("  - {} ({}) enabled={}", repo.name, repo.github, repo.enabled);
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
        if let Some(hook_id) = repo.hook_id {
            if !github.is_empty() && !url.is_empty() {
                let ok = crate::webhook_manager::update_webhook(&github, hook_id, &url, &new_secret).unwrap_or(false);
                println!("  {}: {}", github, if ok { "updated" } else { "FAILED" });
            }
        }
    }
    // update service file
    let service = dirs::home_dir()
        .unwrap()
        .join(".config/systemd/user/factory.service");
    if service.exists() {
        let text = std::fs::read_to_string(&service)?;
        let re = regex::Regex::new(r"Environment=ISSUE_FACTORY_WEBHOOK_SECRET=.*").unwrap();
        let new_text = re.replace(&text, format!("Environment=ISSUE_FACTORY_WEBHOOK_SECRET={}", new_secret));
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
    let mut args = vec!["journalctl", "--user", "-u", "factory", "--no-pager", "-n", n.as_str()];
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

pub fn test(repo: &str, issue: u64, prompt: Option<String>) -> Result<()> {
    let config = load_config()?;
    let repo_entry = match get_repo(&config, repo) {
        Some(r) => r.clone(),
        None => {
            println!("Repo not found: {}", repo);
            return Ok(());
        }
    };
    let project = std::path::PathBuf::from(&repo_entry.path);
    if !project.exists() {
        println!("Project path not found: {}", project.display());
        return Ok(());
    }
    let (code, out) = run(
        &[
            "gh",
            "issue",
            "view",
            &issue.to_string(),
            "--repo",
            &repo_entry.github,
            "--json",
            "number,title,body,url,labels",
        ],
    );
    if code != 0 {
        println!("Failed to fetch issue: {}", out);
        return Ok(());
    }
    let issue_data: serde_json::Value = match serde_json::from_str(&out) {
        Ok(v) => v,
        Err(_) => {
            println!("Failed to parse issue JSON");
            return Ok(());
        }
    };
    match crate::agent::launch_agent(&issue_data, &project, &config, prompt.as_deref()) {
        Some(tid) => println!("Launched thread: {}", tid),
        None => println!("Failed to launch agent."),
    }
    Ok(())
}

pub fn run_prompt(repo: &str, prompt: &str) -> Result<()> {
    let config = load_config()?;
    let repo_entry = match get_repo(&config, repo) {
        Some(r) => r.clone(),
        None => {
            println!("Repo not found: {}", repo);
            return Ok(());
        }
    };
    let project = std::path::PathBuf::from(&repo_entry.path);
    if !project.exists() {
        println!("Project path not found: {}", project.display());
        return Ok(());
    }
    // build a minimal issue-like object
    let issue = serde_json::json!({"number": 0, "title": "custom prompt", "url": "", "labels": []});
    match crate::agent::launch_agent(&issue, &project, &config, Some(prompt)) {
        Some(tid) => println!("Launched thread: {}", tid),
        None => println!("Failed to launch agent."),
    }
    Ok(())
}
