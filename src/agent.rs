use serde_json::Value;
use std::path::Path;
use std::process::Command;

use crate::config::Config;

fn slugify(text: &str) -> String {
    let slug: String = text
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        "issue".to_string()
    } else {
        slug.chars().take(40).collect()
    }
}

pub fn build_prompt(issue: &Value) -> String {
    let number = issue.get("number").and_then(|v| v.as_i64()).unwrap_or(0);
    let url = issue.get("url").and_then(|v| v.as_str()).unwrap_or("").trim();
    format!(
        "Resolve GitHub issue #{number}: {url}\n\nRead the issue via `gh issue view {number}` for full details.\n\n## Instructions\n- Work in the provided worktree/branch.\n- Implement the requested change with tests.\n- Run the project test suite and linters before finishing.\n- Open a pull request that references the issue (Closes #{number}).\n- Do not merge the pull request.\n",
        number = number,
        url = url
    )
}

pub fn launch_agent(issue: &Value, project: &Path, config: &Config, prompt: Option<&str>) -> Option<String> {
    let number = issue.get("number").and_then(|v| v.as_i64())?;
    let title = issue.get("title").and_then(|v| v.as_str()).unwrap_or("").trim();
    let branch = format!("issue-{}-{}", number, slugify(title));
    let thread_title = format!("Fix issue #{}: {}", number, title);
    let thread_title = if thread_title.len() > 80 {
        thread_title[..80].to_string()
    } else {
        thread_title
    };
    let prompt = match prompt {
        Some(p) if !p.trim().is_empty() => p.to_string(),
        _ => build_prompt(issue),
    };

    let defaults = &config.defaults;
    let provider = std::env::var("ISSUE_FACTORY_PROVIDER").unwrap_or_else(|_| defaults.provider.clone());
    let model = std::env::var("ISSUE_FACTORY_MODEL").unwrap_or_else(|_| defaults.model.clone());
    let base_branch = std::env::var("ISSUE_FACTORY_BASE_BRANCH").unwrap_or_else(|_| defaults.base_branch.clone());
    let t3_bin = std::env::var("ISSUE_FACTORY_T3").unwrap_or_else(|_| "t3".to_string());

    let mut cmd = Command::new(&t3_bin);
    cmd.arg("thread")
        .arg("start")
        .arg(project)
        .arg(&prompt)
        .arg("--title")
        .arg(&thread_title)
        .arg("--provider")
        .arg(&provider)
        .arg("--model")
        .arg(&model)
        .arg("--worktree")
        .arg(&base_branch)
        .arg("--branch")
        .arg(&branch)
        .arg("--json")
        .current_dir(project);

    tracing::info!("Launching agent for issue #{} in {}", number, project.display());

    let output = match cmd.output() {
        Ok(o) => o,
        Err(e) => {
            tracing::error!("Failed to launch t3: {}", e);
            return None;
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        tracing::error!(
            "t3 failed for issue #{}: {}",
            number,
            if stderr.trim().is_empty() {
                stdout.trim()
            } else {
                stderr.trim()
            }
        );
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let thread_id = parse_thread_id(&stdout);
    if let Some(ref id) = thread_id {
        tracing::info!("Launched thread {} for issue #{}", id, number);
    } else {
        tracing::warn!("t3 succeeded but no thread id parsed: {}", stdout.trim());
    }
    thread_id
}

fn parse_thread_id(output: &str) -> Option<String> {
    let text = output.trim();
    if text.is_empty() {
        return None;
    }
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(text) {
        if let Some(obj) = val.as_object() {
            for key in ["threadId", "thread_id", "id"] {
                if let Some(v) = obj.get(key).and_then(|v| v.as_str()) {
                    return Some(v.to_string());
                }
            }
        }
        if let Some(s) = val.as_str() {
            return Some(s.to_string());
        }
    }
    text.lines().next().map(|l| l.trim().to_string()).filter(|s| !s.is_empty())
}
