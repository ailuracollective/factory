use serde_json::Value;
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex, OnceLock};

use crate::config::Config;
use crate::execution;

mod process;
mod run;

pub use process::{cancel_running_execution, is_execution_running};
use process::{configure_process_group, register_process, unregister_process};
#[cfg(test)]
use run::{MAX_CAPTURED_OUTPUT, read_bounded_tail};
use run::{output_detail, run_supervised_command};

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

pub fn build_prompt(issue: &Value, project: &Path) -> String {
    let number = issue.get("number").and_then(|v| v.as_i64()).unwrap_or(0);
    let url = issue
        .get("url")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    let instructions = std::fs::read_to_string(project.join(".factory/instructions/issue.md"))
        .unwrap_or_else(|_| {
            include_str!("../../../../../.factory/instructions/issue.md").to_string()
        });
    format!("Resolve GitHub issue #{number}: {url}\n\n{instructions}")
}

#[derive(Debug, Clone)]
pub struct AgentLaunchRequest<'a> {
    pub issue: &'a Value,
    pub project: &'a Path,
    pub config: &'a Config,
    pub prompt: Option<&'a str>,
    pub existing_thread: Option<&'a str>,
    pub attempt: u32,
    pub execution_id: Option<&'a str>,
}

pub fn launch_agent(
    issue: &Value,
    project: &Path,
    config: &Config,
    prompt: Option<&str>,
    existing_thread: Option<&str>,
    attempt: u32,
) -> Result<String, String> {
    launch_agent_with_execution(AgentLaunchRequest {
        issue,
        project,
        config,
        prompt,
        existing_thread,
        attempt,
        execution_id: None,
    })
}

pub fn launch_agent_with_execution(req: AgentLaunchRequest<'_>) -> Result<String, String> {
    let AgentLaunchRequest {
        issue,
        project,
        config,
        prompt,
        existing_thread,
        attempt,
        execution_id,
    } = req;

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
        _ => build_prompt(issue, project),
    };
    let t3_bin = std::env::var("ISSUE_FACTORY_T3").unwrap_or_else(|_| "t3".to_string());

    if let Some(exec_id) = execution_id {
        if execution::is_cancel_requested(exec_id) {
            let _ = execution::mark_cancel_confirmed(exec_id, "cancelled_before_launch");
            return Err("execution cancelled before launch".to_string());
        }
        let _ = execution::transition_execution(
            exec_id,
            Some(execution::ExecutionStatus::Queued),
            execution::ExecutionStatus::Running,
            None,
            None,
        );
    }

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
        match run_supervised_command(cmd, execution_id, "t3 thread send") {
            Ok(out) if out.status.success() => return Ok(thread_id.to_string()),
            Ok(out) => {
                let detail = output_detail(&out.stdout, &out.stderr);
                tracing::warn!(
                    "t3 thread send failed for issue #{}, falling back to start: {}",
                    number,
                    detail
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
    let harness = match std::env::var("ISSUE_FACTORY_HARNESS") {
        Ok(v) => v,
        Err(_) => crate::t3::resolve_harness(&defaults.harness),
    };
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
        // T3 names the harness flag `--provider`.
        .arg("--provider")
        .arg(&harness)
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

    let output = run_supervised_command(cmd, execution_id, "t3 thread start")?;
    if !output.status.success() {
        let detail = output_detail(&output.stdout, &output.stderr);
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
        let p = build_prompt(&issue, Path::new("."));
        assert!(p.contains("issue #7"));
        assert!(p.contains("https://example.com/issue/7"));
        assert!(p.contains("Run the checks relevant to the change"));
        assert!(p.contains("relevant code"));
        assert!(p.contains("Do not merge"));
    }

    #[test]
    fn parse_thread_id_json() {
        let out = r#"{"threadId":"thread-1"}"#;
        assert_eq!(parse_thread_id(out), Some("thread-1".to_string()));
    }

    #[test]
    fn captured_output_keeps_only_the_tail() {
        let mut captured = Vec::new();
        let input = vec![b'a'; MAX_CAPTURED_OUTPUT + 7];
        read_bounded_tail(&mut input.as_slice(), &mut captured);
        assert_eq!(captured.len(), MAX_CAPTURED_OUTPUT);
        assert!(captured.iter().all(|b| *b == b'a'));
    }

    #[test]
    fn launch_passes_harness_as_t3_provider_flag() {
        let _lock = crate::state::tests::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!(
            "factory_agent_args_{}",
            uuid::Uuid::now_v7().simple()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("t3");
        let args_file = dir.join("args");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\necho '{{\"threadId\":\"t1\"}}'\n",
                args_file.display()
            ),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("ISSUE_FACTORY_T3", script.to_str().unwrap()) };

        let config = crate::config::Config::default();
        let issue = serde_json::json!({"number": 7, "title": "t", "url": "u"});
        let res = launch_agent_with_execution(AgentLaunchRequest {
            issue: &issue,
            project: &dir,
            config: &config,
            prompt: Some("p"),
            existing_thread: None,
            attempt: 1,
            execution_id: None,
        });
        assert_eq!(res.unwrap(), "t1");

        let args = std::fs::read_to_string(&args_file).unwrap();
        let args: Vec<&str> = args.lines().collect();
        let flag = args.iter().position(|a| *a == "--provider").unwrap();
        assert_eq!(args[flag + 1], "opencode");
        assert!(!args.contains(&"--harness"));
        assert_eq!(
            args[args.iter().position(|a| *a == "--worktree").unwrap() + 1],
            "main"
        );

        unsafe { std::env::remove_var("ISSUE_FACTORY_T3") };
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn supervised_launch_can_be_cancelled() {
        let _lock = crate::state::tests::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = std::env::temp_dir().join(format!(
            "factory_agent_cancel_{}.db",
            uuid::Uuid::now_v7().simple()
        ));
        let _ = std::fs::remove_file(&tmp);
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("ISSUE_FACTORY_DB_FILE", tmp.to_str().unwrap()) };

        let script = std::env::temp_dir().join(format!("factory_sleep_{}.sh", std::process::id()));
        std::fs::write(&script, "#!/bin/sh\nsleep 30\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("ISSUE_FACTORY_T3", script.to_str().unwrap()) };

        let config = crate::config::Config::default();
        let issue = serde_json::json!({"number": 1, "title": "t", "url": "u"});
        let project = std::env::temp_dir();
        let exec = crate::execution::create_execution(
            "org/repo#1",
            "org/repo",
            1,
            1,
            Some("cli"),
            Some("local"),
            None,
        )
        .unwrap();
        let exec_id = exec.execution_id.clone();

        let handle = std::thread::spawn(move || {
            launch_agent_with_execution(AgentLaunchRequest {
                issue: &issue,
                project: &project,
                config: &config,
                prompt: Some("p"),
                existing_thread: None,
                attempt: 1,
                execution_id: Some(&exec_id),
            })
        });
        std::thread::sleep(std::time::Duration::from_millis(300));
        let _ = crate::execution::request_cancel(&exec.execution_id, "test", false, Some("test"));
        let res = handle.join().unwrap();
        assert!(res.is_err());
        let rec = crate::execution::get_execution(&exec.execution_id)
            .unwrap()
            .unwrap();
        assert_eq!(rec.status, crate::execution::ExecutionStatus::Cancelled);

        let _ = std::fs::remove_file(&script);
        let _ = std::fs::remove_file(&tmp);
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::remove_var("ISSUE_FACTORY_T3") };
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::remove_var("ISSUE_FACTORY_DB_FILE") };
    }
}
