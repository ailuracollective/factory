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
    let _ = crate::agent::cancel_running_execution(&target, force);
    let _ = crate::execution::append_audit(crate::execution::ExecutionAuditEvent {
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
    });
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

pub fn execution_list(
    status: Option<&str>,
    repo: Option<&str>,
    issue: Option<u64>,
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
    let recs =
        crate::execution::list_executions_filtered(repo, status, issue.map(|v| v as i64), limit)?;
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
            next_cursor: None,
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
