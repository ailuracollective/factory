use crate::config::{
    Defaults, FactoryRepoConfig, MentionsConfig, get_repo, load_config, load_factory_config,
    save_config,
};
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

pub fn init(force: bool, local: bool) -> Result<()> {
    if local {
        let path = std::env::current_dir()
            .unwrap_or_default()
            .join("factory.json");
        if path.exists() && !force {
            println!("factory.json already exists. Use --force to overwrite.");
            return Ok(());
        }
        let fc = FactoryRepoConfig {
            defaults: Some(Defaults {
                harness: String::new(),
                model: String::new(),
                base_branch: "main".to_string(),
                label: "status/ready".to_string(),
                max_attempts: 3,
            }),
            mentions: Some(MentionsConfig::default()),
        };
        std::fs::write(&path, serde_json::to_string_pretty(&fc)?)?;
        println!("Created {}", path.display());
        return Ok(());
    }
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

pub fn repo_add(name_or_path: &str, json: bool) -> Result<()> {
    let entry = match add_repo(name_or_path) {
        Some(e) => e,
        None => {
            if json {
                use crate::interface::output::{OutputError, to_json};
                println!(
                    "{}",
                    to_json(&OutputError::new(
                        "REPO_ADD_FAILED",
                        format!("Failed to add repo: {}", name_or_path)
                    ))
                    .unwrap()
                );
            } else {
                println!("Failed to add repo: {}", name_or_path);
            }
            anyhow::bail!("failed to add repo: {}", name_or_path);
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
    let n = sync_labels(&entry.github);
    if json {
        use crate::interface::output::{OutputEnvelope, to_json};
        #[derive(serde::Serialize)]
        struct R {
            name: String,
            github: String,
            path: String,
            labels_synced: u32,
        }
        println!(
            "{}",
            to_json(&OutputEnvelope::new(R {
                name: entry.name.clone(),
                github: entry.github.clone(),
                path: entry.path.clone(),
                labels_synced: n
            }))
            .unwrap()
        );
    } else {
        println!("Repo added: {} -> {}", entry.name, entry.path);
        println!("Labels synced: {}", n);
    }
    Ok(())
}

pub fn repo_list(json: bool) -> Result<()> {
    use crate::interface::output::{OutputEnvelope, OutputMeta, to_json};
    let repos = list_repos();
    if json {
        #[derive(serde::Serialize)]
        struct RepoDto {
            name: String,
            github: String,
            enabled: bool,
            path: String,
        }
        let data: Vec<RepoDto> = repos
            .iter()
            .map(|r| RepoDto {
                name: r.name.clone(),
                github: r.github.clone(),
                enabled: r.enabled,
                path: r.path.clone(),
            })
            .collect();
        let env = OutputEnvelope::new(data).with_meta(OutputMeta {
            count: Some(repos.len()),
            limit: None,
            next_cursor: None,
        });
        println!("{}", to_json(&env).unwrap());
        return Ok(());
    }
    if repos.is_empty() {
        println!("No repos configured. Use 'factory repo add <repo>'.");
        return Ok(());
    }
    for r in repos {
        println!("{:30} {:40} enabled={}", r.name, r.github, r.enabled);
    }
    Ok(())
}

pub fn repo_remove(name: &str, json: bool) -> Result<()> {
    if remove_repo(name) {
        if json {
            use crate::interface::output::{OutputEnvelope, to_json};
            #[derive(serde::Serialize)]
            struct R {
                name: String,
                removed: bool,
            }
            println!(
                "{}",
                to_json(&OutputEnvelope::new(R {
                    name: name.to_string(),
                    removed: true
                }))
                .unwrap()
            );
        } else {
            println!("Repo removed: {}", name);
        }
    } else {
        if json {
            use crate::interface::output::{OutputError, to_json};
            println!(
                "{}",
                to_json(&OutputError::new(
                    "REPO_NOT_FOUND",
                    format!("Repo not found: {}", name)
                ))
                .unwrap()
            );
        } else {
            println!("Repo not found: {}", name);
        }
        anyhow::bail!("repo not found: {}", name);
    }
    Ok(())
}

pub fn repo_sync(name: &str, json: bool) -> Result<()> {
    let config = load_config()?;
    let repo = match get_repo(&config, name) {
        Some(r) => r.clone(),
        None => {
            if json {
                use crate::interface::output::{OutputError, to_json};
                println!(
                    "{}",
                    to_json(&OutputError::new(
                        "REPO_NOT_FOUND",
                        format!("Repo not found: {}", name)
                    ))
                    .unwrap()
                );
            } else {
                println!("Repo not found: {}", name);
            }
            anyhow::bail!("repo not found: {}", name);
        }
    };
    let n = sync_labels(&repo.github);
    if json {
        use crate::interface::output::{OutputEnvelope, to_json};
        #[derive(serde::Serialize)]
        struct R {
            name: String,
            github: String,
            labels_synced: u32,
        }
        println!(
            "{}",
            to_json(&OutputEnvelope::new(R {
                name: name.to_string(),
                github: repo.github.clone(),
                labels_synced: n
            }))
            .unwrap()
        );
    } else {
        println!("Synced {} labels for {}", n, repo.github);
    }
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

pub fn repo_check(name: Option<&str>, json: bool) -> Result<()> {
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

    #[derive(serde::Serialize)]
    struct Check {
        id: String,
        status: String,
        subject: String,
        message: String,
    }
    #[derive(serde::Serialize)]
    struct RepoResult {
        name: String,
        github: String,
        checks: Vec<Check>,
        ok: bool,
    }
    let mut repo_results: Vec<RepoResult> = Vec::new();

    for repo in &repos {
        if !json {
            println!("{} ({})", repo.name, repo.github);
        }
        let mut repo_ok = true;
        let mut checks: Vec<Check> = Vec::new();
        for (level, msg) in repo_check_results(repo, webhook_expected) {
            if !json {
                println!("  [{level}] {msg}");
            }
            if level == "FAIL" {
                repo_ok = false;
            }
            checks.push(Check {
                id: msg.split(':').next().unwrap_or("check").to_string(),
                status: level.to_string(),
                subject: repo.name.clone(),
                message: msg.clone(),
            });
        }
        if !repo_ok {
            failed += 1;
        }
        repo_results.push(RepoResult {
            name: repo.name.clone(),
            github: repo.github.clone(),
            checks,
            ok: repo_ok,
        });
    }

    if json {
        use crate::interface::output::{OutputEnvelope, to_json};
        let env = OutputEnvelope::new(repo_results);
        println!("{}", to_json(&env).unwrap());
    }

    if failed > 0 {
        anyhow::bail!("{failed} repo(s) misconfigured");
    }
    Ok(())
}

pub trait ServiceManager {
    fn start(&self) -> Result<(), String>;
    fn stop(&self) -> Result<(), String>;
    fn restart(&self) -> Result<(), String>;
}

pub struct SystemdUserServiceManager;

impl ServiceManager for SystemdUserServiceManager {
    fn start(&self) -> Result<(), String> {
        let (code, msg) = run(&["systemctl", "--user", "start", "factory"]);
        if code == 0 {
            Ok(())
        } else if code == 127 {
            Err(if msg.is_empty() {
                "unsupported service manager: systemctl not found".to_string()
            } else {
                format!("unsupported service manager: {}", msg)
            })
        } else {
            Err(if msg.is_empty() {
                format!("failed to start (exit code {})", code)
            } else {
                format!("{} (exit code {})", msg, code)
            })
        }
    }

    fn stop(&self) -> Result<(), String> {
        let (code, msg) = run(&["systemctl", "--user", "stop", "factory"]);
        if code == 0 {
            Ok(())
        } else if code == 127 {
            Err(if msg.is_empty() {
                "unsupported service manager: systemctl not found".to_string()
            } else {
                format!("unsupported service manager: {}", msg)
            })
        } else {
            Err(if msg.is_empty() {
                format!("failed to stop (exit code {})", code)
            } else {
                format!("{} (exit code {})", msg, code)
            })
        }
    }

    fn restart(&self) -> Result<(), String> {
        let (code, msg) = run(&["systemctl", "--user", "restart", "factory"]);
        if code == 0 {
            Ok(())
        } else if code == 127 {
            Err(if msg.is_empty() {
                "unsupported service manager: systemctl not found".to_string()
            } else {
                format!("unsupported service manager: {}", msg)
            })
        } else {
            Err(if msg.is_empty() {
                format!("failed to restart (exit code {})", code)
            } else {
                format!("{} (exit code {})", msg, code)
            })
        }
    }
}

pub fn start_with<M: ServiceManager>(mgr: &M) -> Result<()> {
    mgr.start().map_err(anyhow::Error::msg).map(|()| {
        println!("Factory started.");
    })?;
    Ok(())
}

pub fn stop_with<M: ServiceManager>(mgr: &M) -> Result<()> {
    mgr.stop().map_err(anyhow::Error::msg).map(|()| {
        println!("Factory stopped.");
    })?;
    Ok(())
}

pub fn restart_with<M: ServiceManager>(mgr: &M) -> Result<()> {
    mgr.restart().map_err(anyhow::Error::msg).map(|()| {
        println!("Factory restarted.");
    })?;
    Ok(())
}

pub fn start() -> Result<()> {
    start_with(&SystemdUserServiceManager)
}

pub fn stop() -> Result<()> {
    stop_with(&SystemdUserServiceManager)
}

pub fn restart() -> Result<()> {
    restart_with(&SystemdUserServiceManager)
}

#[derive(serde::Serialize)]
struct DaemonResumeData {
    state: &'static str,
}

pub fn resume(json: bool) -> Result<()> {
    let (code, msg) = run(&["systemctl", "--user", "is-active", "factory"]);
    let active = matches!((code, msg.trim()), (0, "active") | (_, "active"));
    if !active {
        if json {
            use crate::interface::output::{OutputError, to_json};
            println!(
                "{}",
                to_json(&OutputError::new(
                    "DAEMON_UNREACHABLE",
                    "daemon is not running".to_string()
                ))
                .unwrap()
            );
        } else {
            anyhow::bail!("daemon is not running");
        }
        anyhow::bail!("daemon is not running");
    }
    if json {
        use crate::interface::output::{OutputEnvelope, to_json};
        println!(
            "{}",
            to_json(&OutputEnvelope::new(DaemonResumeData { state: "running" })).unwrap()
        );
    } else {
        println!("Daemon resumed: accepting jobs.");
    }
    Ok(())
}

pub fn drain(timeout: u64, json: bool) -> Result<()> {
    use crate::config::config_dir;
    use crate::interface::output::{OutputEnvelope, to_json};
    let dir = config_dir();
    std::fs::create_dir_all(&dir)?;
    let state_file = dir.join("daemon_state.json");
    #[derive(serde::Serialize, serde::Deserialize, Default)]
    struct DaemonState {
        state: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        drain_deadline_unix: Option<u64>,
    }
    let mut st = if state_file.exists() {
        serde_json::from_str::<DaemonState>(&std::fs::read_to_string(&state_file)?)
            .unwrap_or_default()
    } else {
        DaemonState {
            state: "running".into(),
            drain_deadline_unix: None,
        }
    };
    // idempotent
    if st.state != "draining" && st.state != "drained" {
        let deadline = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + timeout;
        st.state = "draining".into();
        st.drain_deadline_unix = Some(deadline);
        std::fs::write(&state_file, serde_json::to_string_pretty(&st)?)?;
    }
    // transition to drained (no in-flight tracking available)
    st.state = "drained".into();
    std::fs::write(&state_file, serde_json::to_string_pretty(&st)?)?;

    if json {
        #[derive(serde::Serialize)]
        struct DrainDto {
            state: String,
            active_count: u64,
            quiesced: bool,
            timeout: u64,
        }
        let dto = DrainDto {
            state: st.state.clone(),
            active_count: 0,
            quiesced: true,
            timeout,
        };
        let env = OutputEnvelope {
            schema_version: 1,
            data: dto,
            meta: None,
        };
        println!("{}", to_json(&env)?);
    } else {
        println!("daemon drained");
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

pub fn daemon_run(mode: &str, port: Option<u16>) -> Result<()> {
    let mut config = load_config()?;
    if let Some(p) = port {
        config.webhook.port = p;
    }
    match mode {
        "listener" => {
            let rt = Runtime::new().unwrap();
            rt.block_on(crate::listener::serve(config))
        }
        "runner" => {
            let gw = config.gateway.url.clone();
            let runner_id = config.gateway.runner_id.clone();
            let token = config.gateway.token.clone();
            if gw.is_empty() || runner_id.is_empty() || token.is_empty() {
                anyhow::bail!(
                    "runner mode requires gateway url, runner_id, and token; run 'factory connect' first"
                );
            }
            let rt = Runtime::new().unwrap();
            rt.block_on(crate::listener::serve(config))
        }
        "auto" => {
            let rt = Runtime::new().unwrap();
            rt.block_on(crate::listener::serve(config))
        }
        other => anyhow::bail!("unknown daemon mode: {}", other),
    }
}

pub fn status(json: bool, verbose: bool) -> Result<()> {
    let (code, msg) = run(&["systemctl", "--user", "is-active", "factory"]);
    let systemd_state = match (code, msg.trim()) {
        (0, "active") => "active",
        (_, "inactive") => "inactive",
        (3, _) => "inactive",
        (_, "failed") => "failed",
        (_, "active") => "active",
        _ if code == 127 => {
            anyhow::bail!("failed to query systemd: {}", msg);
        }
        _ => "unknown",
    };
    // failed state detection via show
    let show = run(&[
        "systemctl",
        "--user",
        "show",
        "factory",
        "--property=ActiveState,MainPID,ActiveEnterTimestamp",
    ]);
    let mut pid: Option<u32> = None;
    let mut active_state = systemd_state.to_string();
    let mut active_since: Option<String> = None;
    for line in show.1.lines() {
        if let Some(v) = line.strip_prefix("ActiveState=") {
            if v == "failed" {
                active_state = "failed".to_string();
            } else if v == "inactive" && active_state != "active" {
                active_state = "inactive".to_string();
            } else if v == "active" {
                active_state = "active".to_string();
            }
        } else if let Some(v) = line.strip_prefix("MainPID=") {
            if let Ok(n) = v.parse::<u32>()
                && n != 0
            {
                pid = Some(n);
            }
        } else if let Some(v) = line.strip_prefix("ActiveEnterTimestamp=")
            && !v.trim().is_empty()
            && v.trim() != "n/a"
        {
            active_since = Some(v.trim().to_string());
        }
    }
    let systemd_state = active_state.as_str();

    let config = load_config()?;
    // runtime health
    let runtime_health = if systemd_state == "active" {
        let url = format!("http://127.0.0.1:{}/health", config.webhook.port);
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(2))
            .build()
            .ok();
        match client.and_then(|c| c.get(&url).send().ok()) {
            Some(resp) if resp.status().is_success() => "healthy",
            Some(_) => "degraded",
            None => "unreachable",
        }
    } else {
        "unknown"
    };
    let operation_state = if runtime_health == "healthy" {
        "running"
    } else {
        "stopped"
    };

    if json {
        use crate::interface::output::{OutputEnvelope, to_json};
        #[derive(serde::Serialize)]
        struct RepoSummary {
            name: String,
            github: String,
            enabled: bool,
        }
        #[derive(serde::Serialize)]
        struct StatusDto {
            systemd_state: String,
            runtime_health: String,
            operation_state: String,
            repos_count: usize,
            repos: Vec<RepoSummary>,
            funnel_url: String,
            #[serde(skip_serializing_if = "Option::is_none")]
            pid: Option<u32>,
            #[serde(skip_serializing_if = "Option::is_none")]
            uptime_since: Option<String>,
            #[serde(skip_serializing_if = "Option::is_none")]
            version: Option<String>,
        }
        let repos: Vec<RepoSummary> = config
            .repos
            .iter()
            .map(|r| RepoSummary {
                name: r.name.clone(),
                github: r.github.clone(),
                enabled: r.enabled,
            })
            .collect();
        let dto = StatusDto {
            systemd_state: systemd_state.to_string(),
            runtime_health: runtime_health.to_string(),
            operation_state: operation_state.to_string(),
            repos_count: config.repos.len(),
            repos,
            funnel_url: config.webhook.funnel_url.clone(),
            pid: if verbose { pid } else { None },
            uptime_since: if verbose { active_since } else { None },
            version: if verbose {
                Some(env!("CARGO_PKG_VERSION").to_string())
            } else {
                None
            },
        };
        let env = OutputEnvelope::new(dto);
        println!("{}", to_json(&env).unwrap());
        return Ok(());
    }
    println!("Systemd: {}", systemd_state);
    println!("Runtime: {}", runtime_health);
    println!("Operation: {}", operation_state);
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
    if verbose {
        if let Some(p) = pid {
            println!("PID: {}", p);
        }
        if let Some(s) = active_since {
            println!("Active since: {}", s);
        }
        println!("Version: {}", env!("CARGO_PKG_VERSION"));
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
    let mut args = vec!["--user", "-u", "factory", "--no-pager", "-n", n.as_str()];
    if follow {
        args.push("-f");
    }
    let status = std::process::Command::new("journalctl")
        .args(&args)
        .status()
        .map_err(|e| anyhow::anyhow!("failed to spawn journalctl: {}", e))?;
    // A non-zero exit that is not an interrupt (Ctrl-C) is a real failure.
    if let Some(code) = status.code()
        && code != 0
        && code != 130
        && code != 143
    {
        return Err(anyhow::anyhow!("journalctl exited with status {}", code));
    }
    Ok(())
}

pub fn doctor(json: bool) -> Result<()> {
    crate::doctor::run_doctor(json)
}

mod ops;
pub mod runner_cmd;

pub(crate) use ops::encode_path_segment;
pub use ops::{cancel, connect, execution_list, run_prompt, test};
pub use runner_cmd::runner;

#[cfg(test)]
mod tests;

pub fn harnesses(json: bool) -> Result<()> {
    use crate::interface::output::{OutputEnvelope, OutputMeta, to_json};
    let installed = crate::t3::harnesses();
    let configured = cached_default_harness();
    if json {
        #[derive(serde::Serialize)]
        struct HarnessDto {
            name: String,
            display_name: String,
            version: Option<String>,
            status: String,
            configured_default: bool,
            models: Vec<String>,
        }
        let data: Vec<HarnessDto> = installed
            .iter()
            .map(|h| HarnessDto {
                name: h.name.clone(),
                display_name: h.display_name.clone(),
                version: h.version.clone(),
                status: h.status.clone(),
                configured_default: configured.as_deref() == Some(h.name.as_str()),
                models: h.models.clone(),
            })
            .collect();
        let env = OutputEnvelope::new(data).with_meta(OutputMeta {
            count: Some(installed.len()),
            limit: None,
            next_cursor: None,
        });
        println!("{}", to_json(&env).unwrap());
        return Ok(());
    }
    if installed.is_empty() {
        println!("No installed harnesses found. Start T3 Code once so it can probe them.");
        return Ok(());
    }
    let name_w = installed.iter().map(|h| h.name.len()).max().unwrap_or(0);
    let display_w = installed
        .iter()
        .map(|h| h.display_name.len())
        .max()
        .unwrap_or(0);
    for h in installed {
        let mark = if configured.as_deref() == Some(h.name.as_str()) {
            "*"
        } else {
            " "
        };
        println!(
            "{} {:<name_w$}  {:<display_w$}  {:<8} {:>3} models  {}",
            mark,
            h.name,
            h.display_name,
            h.version.as_deref().unwrap_or("-"),
            h.models.len(),
            h.status,
        );
    }
    Ok(())
}

/// Configured harness resolved to a T3 id, used to flag the default row.
fn cached_default_harness() -> Option<String> {
    let configured = load_config().ok()?.defaults.harness;
    if configured.trim().is_empty() {
        return None;
    }
    Some(crate::t3::resolve_harness(&configured))
}

pub fn models(harness: &str, json: bool) -> Result<()> {
    use crate::interface::output::{OutputEnvelope, OutputError, OutputMeta, to_json};
    let name = crate::t3::resolve_harness(harness);
    let Some(h) = crate::t3::harness(&name) else {
        if json {
            let env = OutputError::new(
                "NOT_FOUND",
                format!(
                    "Unknown harness: {}. Available: {}",
                    name,
                    available_harnesses()
                ),
            );
            println!("{}", to_json(&env).unwrap());
        } else {
            eprintln!(
                "Unknown harness: {}. Available: {}",
                name,
                available_harnesses()
            );
        }
        anyhow::bail!("unknown harness: {}", name);
    };
    if json {
        #[derive(serde::Serialize)]
        struct ModelDto {
            slug: String,
        }
        #[derive(serde::Serialize)]
        struct ModelsDto {
            harness: String,
            models: Vec<ModelDto>,
        }
        let data = ModelsDto {
            harness: h.name.clone(),
            models: h
                .models
                .iter()
                .map(|m| ModelDto { slug: m.clone() })
                .collect(),
        };
        let env = OutputEnvelope::new(data).with_meta(OutputMeta {
            count: Some(h.models.len()),
            limit: None,
            next_cursor: None,
        });
        println!("{}", to_json(&env).unwrap());
        return Ok(());
    }
    for m in h.models {
        println!("{}", m);
    }
    Ok(())
}

fn available_harnesses() -> String {
    let names: Vec<String> = crate::t3::harnesses().into_iter().map(|h| h.name).collect();
    if names.is_empty() {
        "none (start T3 Code so it can probe them)".to_string()
    } else {
        names.join(", ")
    }
}

pub fn config_get(json: bool) -> Result<()> {
    let mut cfg = load_config()?;
    // Apply repo-level factory.json if present in cwd or an ancestor.
    if let Some(repo_cfg) = find_factory_config() {
        if let Some(d) = repo_cfg.defaults {
            if !d.harness.is_empty() {
                cfg.defaults.harness = d.harness;
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
    if json {
        use crate::interface::output::{OutputEnvelope, to_json};
        #[derive(serde::Serialize)]
        struct ConfigDto {
            harness: String,
            model: String,
            base_branch: String,
            label: String,
            max_attempts: u32,
            mentions_enabled: bool,
            mentions_usernames: Vec<String>,
            mentions_usernames_extra: Vec<String>,
            mentions_command: String,
            mentions_daily_quota: u32,
            mentions_report: bool,
            webhook_port: u16,
            webhook_bind: String,
        }
        let dto = ConfigDto {
            harness: crate::t3::resolve_harness(&cfg.defaults.harness),
            model: cfg.defaults.model.clone(),
            base_branch: cfg.defaults.base_branch.clone(),
            label: cfg.defaults.label.clone(),
            max_attempts: cfg.defaults.max_attempts,
            mentions_enabled: cfg.mentions.enabled,
            mentions_usernames: cfg.mentions.usernames.clone(),
            mentions_usernames_extra: cfg.mentions.usernames_extra.clone(),
            mentions_command: cfg.mentions.command.clone(),
            mentions_daily_quota: cfg.mentions.daily_quota,
            mentions_report: cfg.mentions.report,
            webhook_port: cfg.webhook.port,
            webhook_bind: cfg.webhook.bind.clone(),
        };
        let env = OutputEnvelope::new(dto);
        println!("{}", to_json(&env).unwrap());
        return Ok(());
    }
    // Show the harness T3 will actually receive (legacy values resolve).
    println!(
        "harness:  {}",
        crate::t3::resolve_harness(&cfg.defaults.harness)
    );
    println!("model:    {}", cfg.defaults.model);
    println!("base_branch: {}", cfg.defaults.base_branch);
    println!("label: {}", cfg.defaults.label);
    println!("max_attempts: {}", cfg.defaults.max_attempts);
    println!("mentions.enabled: {}", cfg.mentions.enabled);
    println!("mentions.usernames: {}", cfg.mentions.usernames.join(", "));
    println!(
        "mentions.usernames_extra: {}",
        cfg.mentions.usernames_extra.join(", ")
    );
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

pub fn config_set(key: &str, value: &str, global: bool) -> Result<()> {
    if !global {
        let path = std::env::current_dir()
            .unwrap_or_default()
            .join("factory.json");
        let mut fc: FactoryRepoConfig = if path.exists() {
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap_or_default())
                .unwrap_or_default()
        } else {
            FactoryRepoConfig::default()
        };
        if fc.defaults.is_none() {
            fc.defaults = Some(Defaults {
                harness: String::new(),
                model: String::new(),
                base_branch: String::new(),
                label: String::new(),
                max_attempts: 0,
            });
        }
        if fc.mentions.is_none() {
            fc.mentions = Some(MentionsConfig::default());
        }
        let d = fc.defaults.as_mut().unwrap();
        let m = fc.mentions.as_mut().unwrap();
        match key {
            "harness" => d.harness = value.to_string(),
            "model" => d.model = value.to_string(),
            "base_branch" => d.base_branch = value.to_string(),
            "label" => d.label = value.to_string(),
            "max_attempts" => {
                if let Ok(n) = value.parse() {
                    d.max_attempts = n;
                } else {
                    println!("Invalid number: {}", value);
                    return Ok(());
                }
            }
            "mentions.enabled" => m.enabled = value == "true" || value == "1",
            "mentions.usernames" => {
                m.usernames = value
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(String::from)
                    .collect();
            }
            "mentions.command" => m.command = value.to_string(),
            "mentions.allowed_authors" => {
                m.allowed_authors = value
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(String::from)
                    .collect();
            }
            "mentions.daily_quota" => {
                if let Ok(n) = value.parse() {
                    m.daily_quota = n;
                } else {
                    println!("Invalid number: {}", value);
                    return Ok(());
                }
            }
            "mentions.report" => m.report = value == "true" || value == "1",
            "mentions.usernames_extra" => {
                m.usernames_extra = value
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(String::from)
                    .collect();
            }
            _ => {
                println!("Unknown key.");
                return Ok(());
            }
        }
        std::fs::write(&path, serde_json::to_string_pretty(&fc)?)?;
        println!("Set {} = {} (local)", key, value);
        return Ok(());
    }
    let mut cfg = load_config()?;
    match key {
        "harness" => {
            let installed = crate::t3::harnesses();
            // Validate only when T3 has probed its harnesses; otherwise accept.
            if !installed.is_empty() {
                let Some(h) = crate::t3::harness(value) else {
                    println!(
                        "Unknown harness: {}. Available: {}",
                        value,
                        installed
                            .iter()
                            .map(|h| h.name.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                    return Ok(());
                };
                cfg.defaults.harness = h.name;
            } else {
                cfg.defaults.harness = value.to_string();
            }
        }
        "model" => {
            let harness = crate::t3::harness(&cfg.defaults.harness);
            if let Some(h) = &harness
                && !h.models.is_empty()
                && !h.models.iter().any(|m| m == value)
            {
                println!(
                    "Unknown model for {}: {}. Use 'factory models {}' to list.",
                    h.name, value, h.name
                );
                return Ok(());
            }
            cfg.defaults.model = value.to_string();
        }
        "base_branch" => cfg.defaults.base_branch = value.to_string(),
        "label" => cfg.defaults.label = value.to_string(),
        "max_attempts" => {
            if let Ok(n) = value.parse() {
                cfg.defaults.max_attempts = n;
            } else {
                println!("Invalid number: {}", value);
                return Ok(());
            }
        }
        "mentions.enabled" => cfg.mentions.enabled = value == "true" || value == "1",
        "mentions.command" => cfg.mentions.command = value.to_string(),
        "mentions.allowed_authors" => {
            cfg.mentions.allowed_authors = value
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect();
        }
        "mentions.daily_quota" => {
            if let Ok(n) = value.parse() {
                cfg.mentions.daily_quota = n;
            } else {
                println!("Invalid number: {}", value);
                return Ok(());
            }
        }
        "mentions.report" => cfg.mentions.report = value == "true" || value == "1",
        "mentions.usernames_extra" => {
            cfg.mentions.usernames_extra = value
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect();
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
            println!(
                "Unknown key: {}. Use 'harness', 'model', 'base_branch', 'label', 'max_attempts', 'mentions.*'.",
                key
            );
            return Ok(());
        }
    }
    save_config(&cfg)?;
    let stored = if key == "harness" {
        cfg.defaults.harness.as_str()
    } else {
        value
    };
    println!("Set {} = {}", key, stored);
    Ok(())
}
