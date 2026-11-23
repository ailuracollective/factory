use crate::config::{get_repo, load_config, save_config};
use crate::repo_manager::{add_repo, list_repos, remove_repo, sync_labels};
use crate::webhook_manager::create_webhook;
use anyhow::Result;
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

fn repo_check_results(
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
    let (code, out) = run(&[
        "gh",
        "issue",
        "view",
        &issue.to_string(),
        "--repo",
        &repo_entry.github,
        "--json",
        "number,title,body,url,labels",
    ]);
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
    let key = format!("{}#{}", repo_entry.github, issue);
    let prior = crate::state::issue_state(&key);
    let existing = prior.as_ref().and_then(|s| s.thread_id.clone());
    let attempt = prior
        .as_ref()
        .map(|s| s.attempts.saturating_add(1))
        .unwrap_or(1);
    let exec = Some(crate::execution::create_execution(
        &key,
        &repo_entry.github,
        issue as i64,
        attempt,
        Some("cli"),
        Some("local"),
        Some("manual test"),
    )?);
    let exec_id = exec.as_ref().map(|e| e.execution_id.as_str());
    let launch = crate::agent::launch_agent_with_execution(crate::agent::AgentLaunchRequest {
        issue: &issue_data,
        project: &project,
        config: &config,
        prompt: prompt.as_deref(),
        existing_thread: existing.as_deref(),
        attempt,
        execution_id: exec_id,
    });
    match launch {
        Ok(tid) => {
            let pr_url = crate::mentions::discover_pr_url(&repo_entry.github, issue);
            if let Some(id) = exec_id {
                let _ = crate::execution::complete_execution(id, Some(&tid), pr_url.as_deref());
            }
            let _ = crate::state::update_issue_state(&key, |rec| {
                rec.status = crate::state::IssueStatus::Completed;
                rec.thread_id = Some(tid.clone());
                rec.last_error = None;
                if rec.pr_url.is_none() {
                    rec.pr_url = pr_url;
                }
            });
            if let Some(e) = exec {
                println!("Execution: {}", e.execution_id);
            }
            println!("Launched thread: {}", tid);
        }
        Err(e) => {
            if let Some(id) = exec_id {
                let cancelled = crate::execution::is_cancel_requested(id);
                if !cancelled {
                    let _ = crate::execution::fail_execution(id, &e);
                }
            }
            let _ = crate::state::update_issue_state(&key, |rec| {
                rec.status = crate::state::IssueStatus::Failed;
                rec.last_error = Some(e.clone());
            });
            println!("Failed to launch agent: {e}");
        }
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
    let key = format!("{}#0", repo_entry.github);
    let exec = Some(crate::execution::create_execution(
        &key,
        &repo_entry.github,
        0,
        1,
        Some("cli"),
        Some("local"),
        Some("custom prompt"),
    )?);
    let exec_id = exec.as_ref().map(|e| e.execution_id.as_str());
    let launch = crate::agent::launch_agent_with_execution(crate::agent::AgentLaunchRequest {
        issue: &issue,
        project: &project,
        config: &config,
        prompt: Some(prompt),
        existing_thread: None,
        attempt: 1,
        execution_id: exec_id,
    });
    match launch {
        Ok(tid) => {
            if let Some(id) = exec_id {
                let _ = crate::execution::complete_execution(id, Some(&tid), None);
            }
            if let Some(e) = exec {
                println!("Execution: {}", e.execution_id);
            }
            println!("Launched thread: {}", tid);
        }
        Err(e) => {
            if let Some(id) = exec_id
                && !crate::execution::is_cancel_requested(id)
            {
                let _ = crate::execution::fail_execution(id, &e);
            }
            println!("Failed to launch agent: {e}");
        }
    }
    Ok(())
}

pub fn cancel(
    execution_id: Option<&str>,
    repo: Option<&str>,
    issue: Option<u64>,
    pr: Option<u64>,
    force: bool,
) -> Result<()> {
    let actor = std::env::var("USER").unwrap_or_else(|_| "cli".to_string());
    let target = resolve_cancel_target(execution_id, repo, issue, pr)?;
    let rec = crate::execution::request_cancel(
        &target,
        &format!("cli:{actor}"),
        force,
        Some("cli cancel"),
    )?;
    let _ = crate::agent::cancel_running_execution(&target, force);
    let _ = crate::execution::append_audit(crate::execution::ExecutionAuditEvent {
        ts: crate::state::now(),
        actor: format!("cli:{actor}"),
        action: if force {
            "cancel_force".to_string()
        } else {
            "cancel".to_string()
        },
        target: target.clone(),
        outcome: rec.status.as_str().to_string(),
        detail: Some(rec.message.clone()),
    });
    println!("Execution: {}", rec.execution_id);
    println!("Status: {}", rec.status.as_str());
    println!("Requested: {}", rec.requested);
    println!("Confirmed: {}", rec.confirmed);
    println!("{}", rec.message);
    if rec.status == crate::execution::ExecutionStatus::Cancelling && !rec.confirmed {
        println!("Cancellation is not yet confirmed; the execution may still be stopping.");
    }
    Ok(())
}

fn resolve_cancel_target(
    execution_id: Option<&str>,
    repo: Option<&str>,
    issue: Option<u64>,
    pr: Option<u64>,
) -> Result<String> {
    if let Some(id) = execution_id.map(str::trim).filter(|s| !s.is_empty()) {
        let rec = crate::execution::get_execution(id)?
            .ok_or_else(|| anyhow::anyhow!("execution not found: {id}"))?;
        if let Some(repo) = repo
            && !rec.repo.eq_ignore_ascii_case(repo)
        {
            anyhow::bail!("execution {id} does not belong to repo {repo}");
        }
        if let Some(issue) = issue
            && rec.issue_number != issue as i64
        {
            anyhow::bail!("execution {id} does not match issue #{issue}");
        }
        if let Some(pr) = pr {
            let pr_match = rec
                .pr_url
                .as_deref()
                .map(|u| u.contains(&format!("/pull/{pr}")))
                .unwrap_or(false)
                || rec.issue_number == pr as i64;
            if !pr_match {
                anyhow::bail!("execution {id} does not match pr #{pr}");
            }
        }
        return Ok(rec.execution_id);
    }

    let repo =
        repo.ok_or_else(|| anyhow::anyhow!("provide <execution-id> or --repo with --issue/--pr"))?;
    let candidates = if let Some(issue) = issue {
        crate::execution::list_cancellable_for_issue(repo, issue as i64)?
    } else if let Some(pr) = pr {
        crate::execution::list_cancellable_for_repo(repo)?
            .into_iter()
            .filter(|r| {
                r.pr_url
                    .as_deref()
                    .map(|u| u.contains(&format!("/pull/{pr}")))
                    .unwrap_or(false)
                    || r.issue_number == pr as i64
            })
            .collect()
    } else {
        anyhow::bail!("provide --issue or --pr with --repo");
    };

    match candidates.len() {
        0 => anyhow::bail!("no cancellable execution found for the given context"),
        1 => Ok(candidates[0].execution_id.clone()),
        _ => {
            let ids = candidates
                .iter()
                .map(|r| {
                    format!(
                        "{} (issue #{}, {}, attempt {})",
                        r.execution_id,
                        r.issue_number,
                        r.status.as_str(),
                        r.attempt
                    )
                })
                .collect::<Vec<_>>()
                .join("\n  - ");
            anyhow::bail!("ambiguous cancel context; specify execution id:\n  - {ids}");
        }
    }
}

pub fn connect(gateway: &str, runner: &str, bootstrap_secret: Option<&str>) -> Result<()> {
    let client = reqwest::blocking::Client::new();
    let url = format!("{}/runners/register", gateway.trim_end_matches('/'));
    let mut req = client
        .post(&url)
        .json(&serde_json::json!({"runner_id": runner}));
    if let Some(s) = bootstrap_secret {
        req = req.header("x-bootstrap-secret", s);
    }
    let resp = req.send()?;
    if !resp.status().is_success() {
        println!("Registration failed: {}", resp.status());
        return Ok(());
    }
    let body: serde_json::Value = resp.json()?;
    let token = body
        .get("token")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if token.is_empty() {
        println!("No token returned.");
        return Ok(());
    }
    let mut config = load_config()?;
    config.gateway.url = gateway.trim_end_matches('/').to_string();
    config.gateway.runner_id = runner.to_string();
    config.gateway.token = token.clone();
    save_config(&config)?;
    println!("Runner registered: {} -> {}", runner, gateway);
    println!("Token saved to config.");
    Ok(())
}

pub(crate) fn encode_path_segment(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

pub fn runner(once: bool) -> Result<()> {
    use std::sync::atomic::{AtomicBool, Ordering};
    loop {
        let config = load_config()?;
        let gw = config.gateway.url.clone();
        let runner_id = config.gateway.runner_id.clone();
        let token = config.gateway.token.clone();
        if gw.is_empty() || runner_id.is_empty() || token.is_empty() {
            println!("Not connected. Run 'factory connect' first.");
            return Ok(());
        }
        let client = reqwest::blocking::Client::new();
        let claim_url = format!("{}/jobs/claim", gw);
        let resp = client
            .post(&claim_url)
            .json(&serde_json::json!({"runner_id": runner_id, "token": token}))
            .send();
        match resp {
            Ok(r) if r.status().as_u16() == 204 => {
                if once {
                    println!("No job available.");
                    return Ok(());
                }
                std::thread::sleep(std::time::Duration::from_secs(5));
                continue;
            }
            Ok(r) if r.status().is_success() => {
                let job: serde_json::Value = match r.json() {
                    Ok(v) => v,
                    Err(_) => {
                        println!("Invalid job response.");
                        if once {
                            return Ok(());
                        }
                        std::thread::sleep(std::time::Duration::from_secs(5));
                        continue;
                    }
                };
                let job_id = job
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let repo = job
                    .get("repo")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let number = job
                    .get("issue_number")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
                let title = job
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let url = job
                    .get("url")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                let repo_entry = config
                    .repos
                    .iter()
                    .find(|r| r.github == repo || r.name == repo)
                    .cloned();
                let project = repo_entry.map(|r| std::path::PathBuf::from(r.path));
                let project = match project {
                    Some(p) if p.exists() => p,
                    _ => {
                        println!("No project path for {}", repo);
                        let _ = client
                            .post(format!("{}/jobs/{}/fail", gw, encode_path_segment(&job_id)))
                            .json(&serde_json::json!({"runner_id": runner_id, "token": token, "error": "no project path"}))
                            .send();
                        if once {
                            return Ok(());
                        }
                        continue;
                    }
                };

                let issue =
                    serde_json::json!({"number": number, "title": title, "url": url, "labels": []});
                let key = format!("{}#{}", repo, number);
                let prior = crate::state::issue_state(&key);
                let existing = prior.as_ref().and_then(|s| s.thread_id.clone());
                let attempt = prior
                    .as_ref()
                    .map(|s| s.attempts.saturating_add(1))
                    .unwrap_or(1);
                let exec = Some(crate::execution::create_execution(
                    &key,
                    &repo,
                    number,
                    attempt,
                    Some(&format!("runner:{runner_id}")),
                    Some(&runner_id),
                    Some(&format!("gateway-job:{job_id}")),
                )?);
                let exec_id = exec.as_ref().map(|e| e.execution_id.clone());

                let stop = std::sync::Arc::new(AtomicBool::new(false));
                let stop_hb = stop.clone();
                let gw_hb = gw.clone();
                let runner_hb = runner_id.clone();
                let token_hb = token.clone();
                let job_hb = job_id.clone();
                let exec_hb = exec_id.clone();
                let hb = std::thread::spawn(move || {
                    let client = reqwest::blocking::Client::new();
                    while !stop_hb.load(Ordering::Relaxed) {
                        let resp = client
                            .post(format!("{}/jobs/{}/heartbeat", gw_hb, encode_path_segment(&job_hb)))
                            .json(&serde_json::json!({"runner_id": runner_hb, "token": token_hb, "status": "running"}))
                            .send();
                        if let Ok(r) = resp
                            && let Ok(body) = r.json::<serde_json::Value>()
                        {
                            let job_status = body
                                .get("job_status")
                                .and_then(|v| v.as_str())
                                .unwrap_or("");
                            if (job_status == "cancelled" || job_status == "cancelling")
                                && let Some(ref id) = exec_hb
                            {
                                let _ = crate::execution::request_cancel(
                                    id,
                                    "gateway",
                                    false,
                                    Some("gateway job cancelled"),
                                );
                            }
                        }
                        for _ in 0..15 {
                            if stop_hb.load(Ordering::Relaxed) {
                                break;
                            }
                            std::thread::sleep(std::time::Duration::from_secs(1));
                        }
                    }
                });

                let launch =
                    crate::agent::launch_agent_with_execution(crate::agent::AgentLaunchRequest {
                        issue: &issue,
                        project: &project,
                        config: &config,
                        prompt: None,
                        existing_thread: existing.as_deref(),
                        attempt,
                        execution_id: exec_id.as_deref(),
                    });

                stop.store(true, Ordering::Relaxed);
                let _ = hb.join();

                match launch {
                    Ok(tid) => {
                        let cancelled = exec_id
                            .as_deref()
                            .map(crate::execution::is_cancel_requested)
                            .unwrap_or(false);
                        if cancelled {
                            if let Some(ref id) = exec_id {
                                let _ =
                                    crate::execution::mark_cancel_confirmed(id, "runner_cancelled");
                            }
                            let _ = client
                                .post(format!("{}/jobs/{}/fail", gw, encode_path_segment(&job_id)))
                                .json(&serde_json::json!({"runner_id": runner_id, "token": token, "error": "cancelled"}))
                                .send();
                            println!("Cancelled job {}", job_id);
                        } else {
                            let pr_url = crate::mentions::discover_pr_url(&repo, number as u64);
                            if let Some(ref id) = exec_id {
                                let _ = crate::execution::complete_execution(
                                    id,
                                    Some(&tid),
                                    pr_url.as_deref(),
                                );
                            }
                            let _ = crate::state::update_issue_state(&key, |rec| {
                                rec.status = crate::state::IssueStatus::Completed;
                                rec.thread_id = Some(tid.clone());
                                rec.last_error = None;
                                if rec.pr_url.is_none() {
                                    rec.pr_url = pr_url;
                                }
                            });
                            let _ = client
                                .post(format!("{}/jobs/{}/complete", gw, encode_path_segment(&job_id)))
                                .json(&serde_json::json!({"runner_id": runner_id, "token": token, "thread_id": tid}))
                                .send();
                            println!("Completed job {} -> {}", job_id, tid);
                        }
                    }
                    Err(e) => {
                        let cancelled = exec_id
                            .as_deref()
                            .map(crate::execution::is_cancel_requested)
                            .unwrap_or_else(|| e.to_lowercase().contains("cancel"));
                        if cancelled {
                            if let Some(ref id) = exec_id {
                                let _ =
                                    crate::execution::mark_cancel_confirmed(id, "runner_cancelled");
                            }
                            let _ = client
                                .post(format!("{}/jobs/{}/fail", gw, encode_path_segment(&job_id)))
                                .json(&serde_json::json!({"runner_id": runner_id, "token": token, "error": "cancelled"}))
                                .send();
                            println!("Cancelled job {}", job_id);
                        } else {
                            if let Some(ref id) = exec_id {
                                let _ = crate::execution::fail_execution(id, &e);
                            }
                            let _ = client
                                .post(format!("{}/jobs/{}/fail", gw, encode_path_segment(&job_id)))
                                .json(&serde_json::json!({"runner_id": runner_id, "token": token, "error": e}))
                                .send();
                            println!("Failed job {}: {}", job_id, e);
                        }
                    }
                }

                if once {
                    return Ok(());
                }
            }
            Ok(r) => {
                println!("Claim failed: {}", r.status());
                if once {
                    return Ok(());
                }
                std::thread::sleep(std::time::Duration::from_secs(5));
            }
            Err(e) => {
                println!("Network error: {}", e);
                if once {
                    return Ok(());
                }
                std::thread::sleep(std::time::Duration::from_secs(5));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::repo_check_results;
    use crate::config::RepoEntry;
    use std::process::Command;

    fn git(dir: &std::path::Path, args: &[&str]) {
        let status = Command::new("git")
            .args(args)
            .current_dir(dir)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    }

    #[test]
    fn repo_check_reports_ok_for_valid_repo() {
        let dir = std::env::temp_dir().join(format!("factory-repo-check-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        git(&dir, &["init"]);
        git(
            &dir,
            &["remote", "add", "origin", "git@github.com:org/repo.git"],
        );

        let repo = RepoEntry {
            name: "repo".to_string(),
            path: dir.to_string_lossy().to_string(),
            github: "org/repo".to_string(),
            hook_id: Some(1),
            enabled: true,
        };
        let results = repo_check_results(&repo, true);
        assert!(results.iter().all(|(l, _)| *l != "FAIL"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn repo_check_reports_missing_path_and_hook() {
        let repo = RepoEntry {
            name: "repo".to_string(),
            path: "/tmp/factory-repo-check-missing-path".to_string(),
            github: "org/repo".to_string(),
            hook_id: None,
            enabled: true,
        };
        let results = repo_check_results(&repo, true);
        assert!(
            results
                .iter()
                .any(|(l, m)| *l == "FAIL" && m.contains("path missing"))
        );
        assert!(
            results
                .iter()
                .any(|(l, m)| *l == "FAIL" && m.contains("hook_id missing"))
        );
    }
}
