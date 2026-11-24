use super::*;

pub(super) async fn handle_payload(state: AppState, event: String, payload: Value) -> String {
    let action = payload
        .get("action")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let mention_eligible = matches!(
        (event.as_str(), action.as_str()),
        ("issue_comment", "created" | "edited")
            | ("issues", "opened" | "edited")
            | ("pull_request", "opened" | "edited")
            | ("pull_request_review_comment", "created" | "edited")
    );
    if mention_eligible
        && state.config.mentions.enabled
        && !state.config.mentions.usernames.is_empty()
        && let Some(mention_event) = crate::mentions::event_from_webhook(&event, &payload)
    {
        let config = state.config.clone();
        let outcome = tokio::task::spawn_blocking(move || {
            let store = crate::mentions::SqliteEventStore;
            let dispatcher = crate::mentions::AgentDispatcher {
                config: config.clone(),
            };
            let authorizer = crate::mentions::GhAuthorizer;
            let engine = crate::mentions::Engine::new(&config, &store, &dispatcher, &authorizer);
            engine.handle(&mention_event, crate::state::now())
        })
        .await
        .unwrap_or(crate::mentions::Outcome::Failed(
            "mention task panicked".to_string(),
        ));
        match outcome {
            crate::mentions::Outcome::Ignored(_) => {}
            crate::mentions::Outcome::Rejected(reason) => {
                tracing::info!(reason = %reason, "mention rejected; continuing webhook handling");
            }
            crate::mentions::Outcome::Launched(thread) => {
                return format!("mention launched: {thread}");
            }
            crate::mentions::Outcome::Failed(reason) => {
                return format!("mention failed: {reason}");
            }
        }
    }

    if event != "issues" || !["opened", "reopened", "labeled"].contains(&action.as_str()) {
        return format!("ignored: event={event} action={action}");
    }

    let issue = match payload.get("issue") {
        Some(i) if i.is_object() => i.clone(),
        _ => return "ignored: no issue".to_string(),
    };
    let number = match issue.get("number").and_then(|v| v.as_i64()) {
        Some(n) => n,
        None => return "ignored: bad number".to_string(),
    };

    let label = state.config.defaults.label.clone();
    if !label.is_empty() && !issue_matches_label(&label, &issue, &action, &payload) {
        return "ignored: label mismatch".to_string();
    }

    let repo = payload
        .get("repository")
        .and_then(|r| r.get("full_name"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let repo = if repo.is_empty() {
        state.config.repo.clone()
    } else {
        repo
    };
    // Allow any repository registered in config.repos (multi-repo support).
    // Fall back to the single legacy config.repo when no repos are registered.
    let allowed = if !state.config.repos.is_empty() {
        state.config.repos.iter().any(|r| r.github == repo)
    } else {
        state.config.repo.is_empty() || repo == state.config.repo
    };
    if !allowed {
        return "ignored: repo not configured".to_string();
    }

    let key = format!("{}#{}", repo, number);

    // Atomic reserve: decide whether this delivery may launch.
    {
        let mut issues = state.issues.lock().unwrap();
        if let Some(rec) = issues.get(&key) {
            match rec.status {
                IssueStatus::Completed => {
                    return format!("ignored: already completed {}", key);
                }
                IssueStatus::Running => {
                    return format!("ignored: already running {}", key);
                }
                IssueStatus::Failed if rec.attempts >= max_attempts(&state.config) => {
                    return format!("ignored: retries exhausted {}", key);
                }
                _ => {}
            }
        }
        let mut rec = issues.get(&key).cloned().unwrap_or_default();
        rec.status = IssueStatus::Running;
        rec.attempts = rec.attempts.saturating_add(1);
        rec.last_error = None;
        rec.updated_at = crate::state::now();
        issues.insert(key.clone(), rec);
        let snapshot = issues.clone();
        drop(issues);
        let _ = save_state(&snapshot);
    }

    tracing::info!(issue = %key, "reserved issue for launch");

    // resolve project path
    let projects_base = PathBuf::from(&state.config.projects_base);
    let mut project: Option<PathBuf> = None;
    for r in &state.config.repos {
        if r.github == repo || r.name == repo.rsplit('/').next().unwrap_or("") {
            if !r.path.is_empty() {
                project = Some(PathBuf::from(&r.path));
            }
            break;
        }
    }
    if project.is_none() {
        project = Some(projects_base.join(repo.rsplit('/').next().unwrap_or("")));
    }
    let project = project.unwrap();
    if !project.exists() {
        // transition to failed (retryable) state
        let mut issues = state.issues.lock().unwrap();
        if let Some(rec) = issues.get_mut(&key) {
            rec.status = IssueStatus::Failed;
            rec.last_error = Some("no project path".to_string());
            rec.updated_at = crate::state::now();
        }
        let snapshot = issues.clone();
        drop(issues);
        let _ = save_state(&snapshot);
        return format!("ignored: no project for {}", repo);
    }

    let (existing_thread, existing_pr, attempt) = {
        let issues = state.issues.lock().unwrap();
        let rec = issues.get(&key);
        (
            rec.and_then(|r| r.thread_id.clone()),
            rec.and_then(|r| r.pr_url.clone()),
            rec.map(|r| r.attempts).unwrap_or(1),
        )
    };
    let exec = match crate::execution::create_execution(
        &key,
        &repo,
        number,
        attempt,
        Some("webhook"),
        Some("local"),
        Some(&format!("webhook:{event}:{action}")),
    ) {
        Ok(exec) => exec,
        Err(e) => {
            let mut issues = state.issues.lock().unwrap();
            if let Some(rec) = issues.get_mut(&key) {
                rec.status = IssueStatus::Failed;
                rec.last_error = Some(format!("failed to register execution: {e}"));
                rec.updated_at = crate::state::now();
            }
            let snapshot = issues.clone();
            drop(issues);
            let _ = save_state(&snapshot);
            return format!("failed to register execution: {e}");
        }
    };
    let exec_id = Some(exec.execution_id.clone());

    let prefix = crate::infra::execution_labels::prefix_for_project(&project);
    if let Err(e) = crate::infra::execution_labels::project_issue_state(
        &repo,
        number,
        &prefix,
        Some(crate::infra::execution_labels::ExecutionState::Processing),
    ) {
        tracing::warn!(issue = %key, error = %e, "failed to project execution label");
    }

    let issue_for_launch = issue.clone();
    let project_for_launch = project.clone();
    let config_for_launch = state.config.clone();
    let existing_thread_for_launch = existing_thread.clone();
    let exec_for_launch = exec_id.clone();
    let launch_result = tokio::task::spawn_blocking(move || {
        crate::agent::launch_agent_with_execution(crate::agent::AgentLaunchRequest {
            issue: &issue_for_launch,
            project: &project_for_launch,
            config: &config_for_launch,
            prompt: None,
            existing_thread: existing_thread_for_launch.as_deref(),
            attempt,
            execution_id: exec_for_launch.as_deref(),
        })
    })
    .await;
    let pr_url = existing_pr.or_else(|| crate::mentions::discover_pr_url(&repo, number as u64));

    let (thread_id, failure_message) = match launch_result {
        Ok(Ok(id)) => (Some(id), None),
        Ok(Err(e)) => (None, Some(e)),
        Err(e) => (None, Some(format!("launch task failed: {e}"))),
    };

    {
        let mut issues = state.issues.lock().unwrap();
        let rec = issues.entry(key.clone()).or_default();
        if let Some(ref id) = thread_id {
            rec.status = IssueStatus::Completed;
            rec.thread_id = Some(id.clone());
            rec.last_error = None;
            tracing::info!(issue = %key, thread_id = %id, "launch succeeded");
        } else {
            rec.status = IssueStatus::Failed;
            rec.last_error = failure_message.clone();
            tracing::error!(issue = %key, "launch failed");
        }
        if let Some(ref url) = pr_url {
            rec.pr_url = Some(url.clone());
        }
        rec.updated_at = crate::state::now();
        let snapshot = issues.clone();
        drop(issues);
        let _ = save_state(&snapshot);
    }

    if let Some(err) = failure_message {
        let cancelled = exec_id
            .as_ref()
            .map(|id| crate::execution::is_cancel_requested(id))
            .unwrap_or(false);
        if let Some(ref id) = exec_id {
            if cancelled {
                let _ = crate::execution::mark_cancel_confirmed(id, "cancelled");
            } else {
                let _ = crate::execution::fail_execution(id, &err);
            }
        }
        let terminal = if cancelled {
            crate::infra::execution_labels::ExecutionState::Cancelled
        } else {
            crate::infra::execution_labels::ExecutionState::Failed
        };
        if let Err(e) = crate::infra::execution_labels::project_issue_state(
            &repo,
            number,
            &prefix,
            Some(terminal),
        ) {
            tracing::warn!(issue = %key, error = %e, "failed to project execution label");
        }
        let assignees = issue_assignee_logins(&issue);
        report_launch_failure(&repo, number, &err, &assignees);
        format!("failed {}", key)
    } else {
        if let Some(ref id) = exec_id {
            let _ =
                crate::execution::complete_execution(id, thread_id.as_deref(), pr_url.as_deref());
        }
        // Launch succeeded; the agent may still be running. Keep the
        // observational label as processing rather than claiming completion.
        if let Err(e) = crate::infra::execution_labels::project_issue_state(
            &repo,
            number,
            &prefix,
            Some(crate::infra::execution_labels::ExecutionState::Processing),
        ) {
            tracing::warn!(issue = %key, error = %e, "failed to project execution label");
        }
        format!("launched {}", key)
    }
}

fn issue_assignee_logins(issue: &Value) -> Vec<String> {
    issue
        .get("assignees")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|a| a.get("login").and_then(|l| l.as_str()))
                .filter(|l| !l.is_empty())
                .map(|l| l.to_string())
                .collect()
        })
        .unwrap_or_default()
}

fn mention_prefix(assignees: &[String]) -> String {
    if assignees.is_empty() {
        String::new()
    } else {
        let mentions = assignees
            .iter()
            .map(|a| format!("@{}", a.trim_start_matches('@')))
            .collect::<Vec<_>>()
            .join(" ");
        format!("{mentions}\n\n")
    }
}

fn report_launch_failure(repo: &str, number: i64, error: &str, assignees: &[String]) {
    let detail = error.chars().take(1000).collect::<String>();
    let mentions = mention_prefix(assignees);
    let body = format!(
        "{}\n{}Factory failed to launch an agent for this issue.\n\n```\n{}\n```",
        crate::mentions::REPORT_MARKER,
        mentions,
        detail
    );
    let path = format!("repos/{repo}/issues/{number}/comments");
    let body_arg = format!("body={body}");
    let output = std::process::Command::new("gh")
        .args(["api", "-X", "POST", &path, "-f", &body_arg])
        .output();
    match output {
        Ok(o) if o.status.success() => {}
        Ok(o) => tracing::warn!(
            repo = %repo,
            "failed to post failure comment: {}",
            String::from_utf8_lossy(&o.stderr).trim()
        ),
        Err(e) => tracing::warn!(repo = %repo, "failed to post failure comment: {e}"),
    }
}

pub(super) fn issue_matches_label(
    label: &str,
    issue: &Value,
    action: &str,
    payload: &Value,
) -> bool {
    if action == "labeled" {
        let name = payload
            .get("label")
            .and_then(|l| l.get("name"))
            .and_then(|n| n.as_str());
        return name == Some(label);
    }
    let labels = issue.get("labels").and_then(|v| v.as_array());
    match labels {
        Some(arr) => arr
            .iter()
            .filter_map(|l| l.get("name").and_then(|n| n.as_str()))
            .any(|n| n == label),
        None => false,
    }
}
