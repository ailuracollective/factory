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
    let url = issue
        .get("url")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    format!(
        "Resolve GitHub issue #{number}: {url}\n\nRead the issue via `gh issue view {number}` for full details.\n\n## Instructions\n- Work in the provided worktree/branch.\n- Implement the requested change with tests.\n- Before finishing, run the project's full test suite, linters and build; only proceed if every check passes. If a check fails, fix the underlying cause and re-run until green — do not skip, disable, or weaken tests to make them pass.\n- Open a pull request that references the issue (Closes #{number}), including the passing check results in the PR description.\n- Do not merge the pull request.\n",
        number = number,
        url = url
    )
}

pub fn launch_agent(
    issue: &Value,
    project: &Path,
    config: &Config,
    prompt: Option<&str>,
    existing_thread: Option<&str>,
    attempt: u32,
) -> Result<String, String> {
    let number = issue
        .get("number")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| "missing issue number".to_string())?;
    let title = issue
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    let prompt = match prompt {
        Some(p) if !p.trim().is_empty() => p.to_string(),
        _ => build_prompt(issue),
    };
    let t3_bin = std::env::var("ISSUE_FACTORY_T3").unwrap_or_else(|_| "t3".to_string());

    if let Some(thread_id) = existing_thread.filter(|t| !t.trim().is_empty()) {
        tracing::info!(
            "Sending mention to existing thread {} for issue #{}",
            thread_id,
            number
        );
        let mut cmd = Command::new(&t3_bin);
        cmd.arg("thread")
            .arg("send")
            .arg(thread_id)
            .arg(&prompt)
            .arg("--wait")
            .arg("--json")
            .current_dir(project);
        match cmd.output() {
            Ok(o) if o.status.success() => return Ok(thread_id.to_string()),
            Ok(o) => {
                let stderr = String::from_utf8_lossy(&o.stderr);
                let stdout = String::from_utf8_lossy(&o.stdout);
                tracing::warn!(
                    "t3 thread send failed for issue #{}, falling back to start: {}",
                    number,
                    if stderr.trim().is_empty() {
                        stdout.trim()
                    } else {
                        stderr.trim()
                    }
                );
            }
            Err(e) => tracing::warn!(
                "t3 thread send error for issue #{}, falling back to start: {}",
                number,
                e
            ),
        }
    }

    let branch = if attempt > 1 {
        format!("issue-{}-{}-r{}", number, slugify(title), attempt)
    } else {
        format!("issue-{}-{}", number, slugify(title))
    };
    let thread_title = format!("Fix issue #{}: {}", number, title);
    let thread_title = if thread_title.len() > 80 {
        thread_title[..80].to_string()
    } else {
        thread_title
    };

    let defaults = &config.defaults;
    let provider =
        std::env::var("ISSUE_FACTORY_PROVIDER").unwrap_or_else(|_| defaults.provider.clone());
    let model = std::env::var("ISSUE_FACTORY_MODEL").unwrap_or_else(|_| defaults.model.clone());
    let base_branch =
        std::env::var("ISSUE_FACTORY_BASE_BRANCH").unwrap_or_else(|_| defaults.base_branch.clone());

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
        .arg("--wait")
        .arg("--json")
        .current_dir(project);

    tracing::info!(
        "Launching agent for issue #{} in {}",
        number,
        project.display()
    );

    let output = match cmd.output() {
        Ok(o) => o,
        Err(e) => {
            let msg = format!("Failed to launch t3: {e}");
            tracing::error!("{}", msg);
            return Err(msg);
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let detail = if stderr.trim().is_empty() {
            stdout.trim()
        } else {
            stderr.trim()
        };
        let msg = format!("t3 failed for issue #{number}: {detail}");
        tracing::error!("{}", msg);
        return Err(msg);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let thread_id = parse_thread_id(&stdout);
    if let Some(ref id) = thread_id {
        tracing::info!("Launched thread {} for issue #{}", id, number);
        Ok(id.clone())
    } else {
        let msg = format!("t3 succeeded but no thread id parsed: {}", stdout.trim());
        tracing::warn!("{}", msg);
        Err(msg)
    }
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
    text.lines()
        .next()
        .map(|l| l.trim().to_string())
        .filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn slugify_basic() {
        assert_eq!(slugify("Fix Bug!"), "fix-bug");
        assert_eq!(slugify("   "), "issue");
    }

    #[test]
    fn build_prompt_contains_fields() {
        let issue = json!({
            "number": 7,
            "title": "Fix bug",
            "url": "https://example.com/issue/7"
        });
        let p = build_prompt(&issue);
        assert!(p.contains("issue #7"));
        assert!(p.contains("https://example.com/issue/7"));
        assert!(p.contains("Closes #7"));
    }

    #[test]
    fn parse_thread_id_json() {
        let out = r#"{"threadId":"thread-1"}"#;
        assert_eq!(parse_thread_id(out), Some("thread-1".to_string()));
    }
}
