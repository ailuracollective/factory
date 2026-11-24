use super::*;

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
    let key = format!(
        "{}#prompt#{}",
        repo_entry.github,
        crate::execution::new_execution_id()
    );
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
    json: bool,
) -> Result<()> {
    let actor = std::env::var("USER").unwrap_or_else(|_| "factory".to_string());
    let target = resolve_cancel_target(execution_id, repo, issue, pr)?;
    let rec = crate::execution::request_cancel(
        &target,
        &format!("factory:{actor}"),
        force,
        Some("factory cancel"),
    )?;
    crate::agent::cancel_running_execution(&target, force)
        .map_err(|e| anyhow::anyhow!("failed to deliver cancellation to running execution: {e}"))?;
    crate::execution::append_audit(crate::execution::ExecutionAuditEvent {
        ts: crate::state::now(),
        actor: format!("factory:{actor}"),
        action: if force {
            "cancel_force".to_string()
        } else {
            "cancel".to_string()
        },
        target: target.clone(),
        outcome: rec.status.as_str().to_string(),
        detail: Some(rec.message.clone()),
    })?;
    if json {
        use crate::interface::output::{OutputEnvelope, to_json};
        #[derive(serde::Serialize)]
        struct CancelResult {
            execution_id: String,
            status: String,
            requested: bool,
            confirmed: bool,
            message: String,
        }
        let data = CancelResult {
            execution_id: rec.execution_id.clone(),
            status: rec.status.as_str().to_string(),
            requested: rec.requested,
            confirmed: rec.confirmed,
            message: rec.message.clone(),
        };
        println!("{}", to_json(&OutputEnvelope::new(data)).unwrap());
    } else {
        println!("Execution: {}", rec.execution_id);
        println!("Status: {}", rec.status.as_str());
        println!("Requested: {}", rec.requested);
        println!("Confirmed: {}", rec.confirmed);
        println!("{}", rec.message);
        if rec.status == crate::execution::ExecutionStatus::Cancelling && !rec.confirmed {
            println!("Cancellation is not yet confirmed; the execution may still be stopping.");
        }
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
        if let Some(pr) = pr
            && !pr_matches(Some(&rec.repo), rec.pr_url.as_deref(), pr)
        {
            anyhow::bail!("execution {id} does not match pr #{pr}");
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
            .filter(|r| pr_matches(Some(repo), r.pr_url.as_deref(), pr))
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

fn pr_matches(repo_constraint: Option<&str>, pr_url: Option<&str>, pr: u64) -> bool {
    let Some(url) = pr_url else {
        return false;
    };
    let url = url.trim().trim_end_matches('/');
    let suffix = format!("/pull/{pr}");
    if !url.ends_with(&suffix) {
        return false;
    }
    let Some(rest) = url
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("http://github.com/"))
    else {
        return false;
    };
    let Some(repo_part) = rest.strip_suffix(&suffix) else {
        return false;
    };
    if repo_part.is_empty() {
        return false;
    }
    if let Some(repo) = repo_constraint
        && !repo_part.eq_ignore_ascii_case(repo)
    {
        return false;
    }
    true
}

pub fn connect(gateway: &str, runner: &str, bootstrap_secret: Option<&str>) -> Result<()> {
    let mut config = load_config()?;
    if config.gateway.callback_secret.trim().is_empty() {
        config.gateway.callback_secret = generate_secret();
    }
    let callback_url = if config.gateway.callback_url.trim().is_empty() {
        let base = config.webhook.funnel_url.trim_end_matches('/').to_string();
        if base.is_empty() {
            String::new()
        } else {
            format!("{}/runner/jobs", base)
        }
    } else {
        config.gateway.callback_url.clone()
    };
    if !callback_url.is_empty() {
        config.gateway.callback_url = callback_url.clone();
    }
    save_config(&config)?;

    let client = reqwest::blocking::Client::new();
    let url = format!("{}/runners/register", gateway.trim_end_matches('/'));
    let mut req = client.post(&url).json(&serde_json::json!({
        "runner_id": runner,
        "callback_url": callback_url,
        "callback_secret": config.gateway.callback_secret,
    }));
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

#[allow(clippy::too_many_arguments)]
pub fn execution_list(
    status: Option<&str>,
    repo: Option<&str>,
    issue: Option<u64>,
    source: Option<&str>,
    since: Option<u64>,
    until: Option<u64>,
    cursor: Option<&str>,
    limit: u64,
    json: bool,
) -> Result<()> {
    // validation
    if issue.is_some() && repo.is_none() {
        anyhow::bail!("--issue requires --repo");
    }
    if limit == 0 || limit > 200 {
        anyhow::bail!("--limit must be between 1 and 200");
    }
    if let Some(s) = status {
        let valid = [
            "queued",
            "running",
            "cancelling",
            "cancelled",
            "completed",
            "failed",
        ];
        if !valid.contains(&s) {
            anyhow::bail!("invalid status: {s}");
        }
    }
    if let Some(src) = source
        && src != "manual"
        && src != "github"
    {
        anyhow::bail!("invalid source: {src}");
    }
    if let Some((a, b)) = since.zip(until)
        && a > b
    {
        anyhow::bail!("--since must be <= --until");
    }
    if let Some(c) = cursor {
        let parts: Vec<&str> = c.splitn(2, ':').collect();
        if parts.len() != 2 || parts[0].parse::<u64>().is_err() {
            anyhow::bail!("invalid cursor: {c}");
        }
    }
    if source == Some("manual") && issue.is_some() {
        anyhow::bail!("--source manual cannot be combined with --issue");
    }
    let mut recs = crate::execution::list_executions_filtered(
        repo,
        status,
        issue.map(|v| v as i64),
        source,
        since,
        until,
        cursor,
        limit,
    )?;
    let next_cursor = if recs.len() as u64 > limit {
        let last = &recs[limit as usize - 1];
        Some(format!("{}:{}", last.started_at, last.execution_id))
    } else {
        None
    };
    recs.truncate(limit as usize);
    if json {
        use crate::interface::output::{OutputEnvelope, OutputMeta, to_json};
        #[derive(serde::Serialize)]
        struct ExecutionListItem {
            execution_id: String,
            status: String,
            repo: String,
            issue: i64,
            attempt: u32,
            runner: Option<String>,
            started_at: u64,
            ended_at: Option<u64>,
            updated_at: u64,
        }
        let data: Vec<ExecutionListItem> = recs
            .iter()
            .map(|r| ExecutionListItem {
                execution_id: r.execution_id.clone(),
                status: r.status.as_str().to_string(),
                repo: r.repo.clone(),
                issue: r.issue_number,
                attempt: r.attempt,
                runner: r.runner.clone().or(r.requester.clone()),
                started_at: r.started_at,
                ended_at: r.ended_at,
                updated_at: r.updated_at,
            })
            .collect();
        let count = data.len();
        let env = OutputEnvelope::new(data).with_meta(OutputMeta {
            count: Some(count),
            limit: Some(limit as usize),
            next_cursor: next_cursor.clone(),
        });
        println!("{}", to_json(&env).unwrap());
    } else {
        if recs.is_empty() {
            println!("No executions found.");
            return Ok(());
        }
        println!(
            "{:<40} {:<12} {:<28} {:<8} {:<8} RUNNER/ORIGIN",
            "EXECUTION", "STATUS", "REPO", "ISSUE", "ATTEMPT"
        );
        for r in &recs {
            println!(
                "{:<40} {:<12} {:<28} {:<8} {:<8} {}",
                r.execution_id,
                r.status.as_str(),
                r.repo,
                r.issue_number,
                r.attempt,
                r.runner.clone().or(r.requester.clone()).unwrap_or_default()
            );
        }
    }
    Ok(())
}

pub fn agent_run(
    repo: &str,
    issue: Option<u64>,
    prompt: Option<&str>,
    harness: Option<&str>,
    model: Option<&str>,
    json: bool,
) -> Result<()> {
    // exactly one source
    match (issue, prompt) {
        (Some(_), Some(_)) => {
            anyhow::bail!("--issue and --prompt are mutually exclusive");
        }
        (None, None) => {
            anyhow::bail!("either --issue or --prompt is required");
        }
        _ => {}
    }

    let config = load_config()?;
    let repo_entry = match get_repo(&config, repo) {
        Some(r) => r.clone(),
        None => {
            if json {
                println!(
                    "{}",
                    serde_json::json!({"error": format!("repo not found: {repo}")})
                );
            } else {
                println!("Repo not found: {}", repo);
            }
            return Err(anyhow::anyhow!("repo not found: {repo}"));
        }
    };
    let project = std::path::PathBuf::from(&repo_entry.path);
    if !project.exists() {
        if json {
            println!(
                "{}",
                serde_json::json!({"error": format!("project path not found: {}", project.display())})
            );
        } else {
            println!("Project path not found: {}", project.display());
        }
        return Err(anyhow::anyhow!("project path not found"));
    }

    // apply overrides via env (no config mutation)
    if let Some(h) = harness {
        unsafe {
            std::env::set_var("ISSUE_FACTORY_HARNESS", h);
        }
    }
    if let Some(m) = model {
        unsafe {
            std::env::set_var("ISSUE_FACTORY_MODEL", m);
        }
    }

    if let Some(issue_num) = issue {
        // fetch issue
        let (code, out) = run(&[
            "gh",
            "issue",
            "view",
            &issue_num.to_string(),
            "--repo",
            &repo_entry.github,
            "--json",
            "number,title,body,url,labels",
        ]);
        if code != 0 {
            if json {
                println!(
                    "{}",
                    serde_json::json!({"error": format!("failed to fetch issue: {out}")})
                );
            } else {
                println!("Failed to fetch issue: {}", out);
            }
            return Err(anyhow::anyhow!("failed to fetch issue"));
        }
        let issue_data: serde_json::Value = serde_json::from_str(&out)
            .map_err(|_| anyhow::anyhow!("failed to parse issue JSON"))?;

        let key = format!("{}#{}", repo_entry.github, issue_num);
        let prior = crate::state::issue_state(&key);
        let existing = prior.as_ref().and_then(|s| s.thread_id.clone());
        let attempt = prior
            .as_ref()
            .map(|s| s.attempts.saturating_add(1))
            .unwrap_or(1);
        let exec = crate::execution::create_execution(
            &key,
            &repo_entry.github,
            issue_num as i64,
            attempt,
            Some("cli"),
            Some("local"),
            Some("agent run"),
        )?;
        let exec_id = exec.execution_id.clone();
        let launch = crate::agent::launch_agent_with_execution(crate::agent::AgentLaunchRequest {
            issue: &issue_data,
            project: &project,
            config: &config,
            prompt: None,
            existing_thread: existing.as_deref(),
            attempt,
            execution_id: Some(&exec_id),
        });
        match launch {
            Ok(tid) => {
                let pr_url = crate::mentions::discover_pr_url(&repo_entry.github, issue_num);
                let _ =
                    crate::execution::complete_execution(&exec_id, Some(&tid), pr_url.as_deref());
                let _ = crate::state::update_issue_state(&key, |rec| {
                    rec.status = crate::state::IssueStatus::Completed;
                    rec.thread_id = Some(tid.clone());
                    rec.last_error = None;
                    if rec.pr_url.is_none() {
                        rec.pr_url = pr_url.clone();
                    }
                });
                if json {
                    println!(
                        "{}",
                        serde_json::json!({
                            "execution_id": exec_id,
                            "source": "issue",
                            "status": "completed",
                            "thread_id": tid,
                            "pr_url": pr_url,
                        })
                    );
                } else {
                    println!("Execution: {}", exec_id);
                    println!("Launched thread: {}", tid);
                }
                Ok(())
            }
            Err(e) => {
                if !crate::execution::is_cancel_requested(&exec_id) {
                    let _ = crate::execution::fail_execution(&exec_id, &e);
                }
                let _ = crate::state::update_issue_state(&key, |rec| {
                    rec.status = crate::state::IssueStatus::Failed;
                    rec.last_error = Some(e.clone());
                });
                if json {
                    println!("{}", serde_json::json!({"error": e}));
                } else {
                    println!("Failed to launch agent: {e}");
                }
                Err(anyhow::anyhow!("{}", e))
            }
        }
    } else {
        let p = prompt.unwrap_or("");
        let issue =
            serde_json::json!({"number": 0, "title": "custom prompt", "url": "", "labels": []});
        let key = format!(
            "{}#prompt#{}",
            repo_entry.github,
            crate::execution::new_execution_id()
        );
        let exec = crate::execution::create_execution(
            &key,
            &repo_entry.github,
            0,
            1,
            Some("cli"),
            Some("local"),
            Some("custom prompt"),
        )?;
        let exec_id = exec.execution_id.clone();
        let launch = crate::agent::launch_agent_with_execution(crate::agent::AgentLaunchRequest {
            issue: &issue,
            project: &project,
            config: &config,
            prompt: Some(p),
            existing_thread: None,
            attempt: 1,
            execution_id: Some(&exec_id),
        });
        match launch {
            Ok(tid) => {
                let _ = crate::execution::complete_execution(&exec_id, Some(&tid), None);
                if json {
                    println!(
                        "{}",
                        serde_json::json!({
                            "execution_id": exec_id,
                            "source": "prompt",
                            "status": "completed",
                            "thread_id": tid,
                            "pr_url": null,
                        })
                    );
                } else {
                    println!("Execution: {}", exec_id);
                    println!("Launched thread: {}", tid);
                }
                Ok(())
            }
            Err(e) => {
                if !crate::execution::is_cancel_requested(&exec_id) {
                    let _ = crate::execution::fail_execution(&exec_id, &e);
                }
                if json {
                    println!("{}", serde_json::json!({"error": e}));
                } else {
                    println!("Failed to launch agent: {e}");
                }
                Err(anyhow::anyhow!("{}", e))
            }
        }
    }
}

pub fn agent_check(
    repo: Option<&str>,
    harness_override: Option<&str>,
    model_override: Option<&str>,
    json: bool,
) -> Result<()> {
    let config = load_config()?;
    let defaults = &config.defaults;

    // resolve harness/model like agent run: env overrides then args
    let mut harness = match std::env::var("ISSUE_FACTORY_HARNESS") {
        Ok(v) => v,
        Err(_) => crate::t3::resolve_harness(&defaults.harness),
    };
    if let Some(h) = harness_override {
        harness = crate::t3::resolve_harness(h);
    }
    let mut model = std::env::var("ISSUE_FACTORY_MODEL").unwrap_or_else(|_| defaults.model.clone());
    if let Some(m) = model_override {
        model = m.to_string();
    }

    #[derive(serde::Serialize)]
    struct Check {
        id: &'static str,
        status: &'static str,
        message: String,
    }
    let mut checks: Vec<Check> = Vec::new();
    let mut required_failed = false;

    // project path / repo registration
    if let Some(repo_name) = repo {
        match get_repo(&config, repo_name) {
            Some(r) => {
                let project = std::path::PathBuf::from(&r.path);
                if project.exists() {
                    checks.push(Check {
                        id: "project_path",
                        status: "pass",
                        message: format!("project path exists: {}", project.display()),
                    });
                } else {
                    checks.push(Check {
                        id: "project_path",
                        status: "fail",
                        message: format!("project path not found: {}", project.display()),
                    });
                    required_failed = true;
                }
                checks.push(Check {
                    id: "repo_registration",
                    status: "pass",
                    message: format!("repo registered: {}", r.github),
                });
            }
            None => {
                checks.push(Check {
                    id: "repo_registration",
                    status: "fail",
                    message: format!("repo not found: {repo_name}"),
                });
                required_failed = true;
            }
        }
    } else {
        checks.push(Check {
            id: "repo_registration",
            status: "warn",
            message: "no --repo provided; skipping repo/project checks".to_string(),
        });
    }

    // harness availability
    let installed = crate::t3::harnesses();
    let h = crate::t3::harness(&harness);
    if h.is_some() {
        checks.push(Check {
            id: "harness_available",
            status: "pass",
            message: format!("harness available: {harness}"),
        });
    } else if installed.is_empty() {
        checks.push(Check {
            id: "harness_available",
            status: "unknown",
            message: format!("cannot determine harness availability (T3 cache missing): {harness}"),
        });
    } else {
        checks.push(Check {
            id: "harness_available",
            status: "fail",
            message: format!("harness not installed: {harness}"),
        });
        required_failed = true;
    }

    // model discovery
    match h {
        Some(hh) => {
            if hh.models.is_empty() || hh.models.contains(&model) {
                checks.push(Check {
                    id: "model_discovery",
                    status: "pass",
                    message: format!("model available: {model}"),
                });
            } else {
                checks.push(Check {
                    id: "model_discovery",
                    status: "fail",
                    message: format!("model not found in harness {harness}: {model}"),
                });
                required_failed = true;
            }
        }
        None => {
            checks.push(Check {
                id: "model_discovery",
                status: "unknown",
                message: format!("cannot verify model (harness unavailable): {model}"),
            });
        }
    }

    let overall = if required_failed { "fail" } else { "pass" };

    if json {
        println!(
            "{}",
            serde_json::json!({
                "harness": harness,
                "model": model,
                "overall": overall,
                "checks": checks,
            })
        );
    } else {
        for c in &checks {
            println!("[{}] {} - {}", c.status.to_uppercase(), c.id, c.message);
        }
        println!("Overall: {}", overall);
    }

    if required_failed {
        return Err(anyhow::anyhow!("agent check failed"));
    }
    Ok(())
}

pub fn execution_show(execution_id: &str, json: bool) -> Result<()> {
    let rec = match crate::execution::get_execution(execution_id)? {
        Some(r) => r,
        None => anyhow::bail!("execution not found: {execution_id}"),
    };
    if json {
        use crate::interface::output::{OutputEnvelope, to_json};
        #[derive(serde::Serialize)]
        struct ExecutionShow {
            execution_id: String,
            issue_key: String,
            source: String,
            status: String,
            repo: String,
            issue: i64,
            attempt: u32,
            requester: Option<String>,
            runner: Option<String>,
            thread_id: Option<String>,
            pr_url: Option<String>,
            reason: Option<String>,
            cancel_requested: bool,
            cancel_requested_by: Option<String>,
            cancel_requested_at: Option<u64>,
            cancel_reason: Option<String>,
            cancel_confirmed_at: Option<u64>,
            cancel_outcome: Option<String>,
            started_at: u64,
            ended_at: Option<u64>,
            updated_at: u64,
        }
        let source = if rec.issue_number == 0 {
            "manual".to_string()
        } else {
            "github".to_string()
        };
        let data = ExecutionShow {
            execution_id: rec.execution_id.clone(),
            issue_key: rec.issue_key.clone(),
            source,
            status: rec.status.as_str().to_string(),
            repo: rec.repo.clone(),
            issue: rec.issue_number,
            attempt: rec.attempt,
            requester: rec.requester.clone(),
            runner: rec.runner.clone(),
            thread_id: rec.thread_id.clone(),
            pr_url: rec.pr_url.clone(),
            reason: rec.reason.clone(),
            cancel_requested: rec.cancel_requested,
            cancel_requested_by: rec.cancel_requested_by.clone(),
            cancel_requested_at: rec.cancel_requested_at,
            cancel_reason: rec.cancel_reason.clone(),
            cancel_confirmed_at: rec.cancel_confirmed_at,
            cancel_outcome: rec.cancel_outcome.clone(),
            started_at: rec.started_at,
            ended_at: rec.ended_at,
            updated_at: rec.updated_at,
        };
        let env = OutputEnvelope::new(data);
        println!("{}", to_json(&env).unwrap());
    } else {
        println!("Execution: {}", rec.execution_id);
        println!(
            "Source: {}",
            if rec.issue_number == 0 {
                "manual"
            } else {
                "github"
            }
        );
        println!("State: {}", rec.status.as_str());
        println!("Repository: {}", rec.repo);
        println!("Issue: {}", rec.issue_number);
        println!("Attempt: {}", rec.attempt);
        println!("Started: {}", rec.started_at);
        println!("Updated: {}", rec.updated_at);
        if let Some(ended) = rec.ended_at {
            println!("Ended: {}", ended);
        }
        if let Some(v) = &rec.requester {
            println!("Requester: {}", v);
        }
        if let Some(v) = &rec.runner {
            println!("Runner: {}", v);
        }
        if let Some(v) = &rec.thread_id {
            println!("Thread: {}", v);
        }
        if let Some(v) = &rec.pr_url {
            println!("PR: {}", v);
        }
        if let Some(v) = &rec.reason {
            println!("Reason: {}", v);
        }
        if let Some(v) = &rec.cancel_reason {
            println!("Cancel reason: {}", v);
        }
        if let Some(v) = &rec.cancel_outcome {
            println!("Cancel outcome: {}", v);
        }
    }
    Ok(())
}
