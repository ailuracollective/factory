use serde_json::Value;
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use crate::config::Config;
use crate::execution;

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

#[derive(Debug, Clone)]
struct RunningProcess {
    pgid: u32,
    child: Arc<Mutex<Option<std::process::Child>>>,
}

fn registry() -> &'static Mutex<HashMap<String, RunningProcess>> {
    static REG: OnceLock<Mutex<HashMap<String, RunningProcess>>> = OnceLock::new();
    REG.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn is_execution_running(execution_id: &str) -> bool {
    registry().lock().unwrap().contains_key(execution_id)
}

pub fn cancel_running_execution(execution_id: &str, _force: bool) -> Result<bool, String> {
    let proc = {
        let reg = registry().lock().unwrap();
        reg.get(execution_id).cloned()
    };
    let Some(proc) = proc else {
        return Ok(false);
    };
    signal_owned_process(&proc.child, proc.pgid, Signal::Term)?;
    Ok(true)
}

#[derive(Clone, Copy)]
enum Signal {
    Term,
    Kill,
}

#[cfg(unix)]
fn signal_process_group(pgid: u32, sig: Signal) -> Result<(), String> {
    let sig_arg = match sig {
        Signal::Term => "-TERM",
        Signal::Kill => "-KILL",
    };
    let target = format!("-{}", pgid);
    let status = Command::new("kill")
        .args([sig_arg, &target])
        .status()
        .map_err(|e| format!("failed to signal process group {pgid}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        // Process may have already exited; treat as non-fatal.
        Ok(())
    }
}

#[cfg(windows)]
fn signal_process_group(pgid: u32, sig: Signal) -> Result<(), String> {
    let pid = pgid.to_string();
    let mut cmd = Command::new("taskkill");
    cmd.args(["/PID", &pid, "/T"]);
    if matches!(sig, Signal::Kill) {
        cmd.arg("/F");
    }
    let status = cmd
        .status()
        .map_err(|e| format!("failed to stop process tree {pgid}: {e}"))?;
    if !status.success() {
        return Err(format!("taskkill failed for process tree {pgid}"));
    }
    Ok(())
}

#[cfg(not(any(unix, windows)))]
fn signal_process_group(_pgid: u32, _sig: Signal) -> Result<(), String> {
    Err("process-tree cancellation is unsupported on this platform".to_string())
}

fn signal_owned_process(
    child: &Arc<Mutex<Option<std::process::Child>>>,
    pgid: u32,
    sig: Signal,
) -> Result<(), String> {
    #[cfg(unix)]
    {
        let _ = child;
        signal_process_group(pgid, sig)
    }
    #[cfg(windows)]
    {
        let _ = child;
        signal_process_group(pgid, sig)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = pgid;
        let mut guard = child.lock().unwrap();
        match guard.as_mut() {
            Some(child) => child
                .kill()
                .map_err(|e| format!("failed to stop owned process: {e}")),
            None => Ok(()),
        }
    }
}

#[cfg(unix)]
fn configure_process_group(cmd: &mut Command) {
    use std::os::unix::process::CommandExt;
    cmd.process_group(0);
}

#[cfg(not(unix))]
fn configure_process_group(_cmd: &mut Command) {}

fn register_process(execution_id: &str, child: Arc<Mutex<Option<std::process::Child>>>, pgid: u32) {
    registry()
        .lock()
        .unwrap()
        .insert(execution_id.to_string(), RunningProcess { pgid, child });
}

fn unregister_process(execution_id: &str) {
    registry().lock().unwrap().remove(execution_id);
}

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
        _ => build_prompt(issue),
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

struct CommandOutput {
    status: std::process::ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

fn output_detail(stdout: &[u8], stderr: &[u8]) -> String {
    let stderr = String::from_utf8_lossy(stderr);
    let stdout = String::from_utf8_lossy(stdout);
    let detail = if stderr.trim().is_empty() {
        stdout.trim()
    } else {
        stderr.trim()
    };
    detail.chars().take(2000).collect()
}

const MAX_CAPTURED_OUTPUT: usize = 1024 * 1024;

fn read_bounded_tail(reader: &mut impl Read, output: &mut Vec<u8>) {
    let mut chunk = [0; 8192];
    loop {
        match reader.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                output.extend_from_slice(&chunk[..n]);
                if output.len() > MAX_CAPTURED_OUTPUT {
                    let excess = output.len() - MAX_CAPTURED_OUTPUT;
                    output.drain(..excess);
                }
            }
        }
    }
}

fn run_supervised_command(
    mut cmd: Command,
    execution_id: Option<&str>,
    label: &str,
) -> Result<CommandOutput, String> {
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    configure_process_group(&mut cmd);

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to launch {label}: {e}"))?;
    let pgid = child.id();
    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();
    let child = Arc::new(Mutex::new(Some(child)));
    if let Some(exec_id) = execution_id {
        register_process(exec_id, child.clone(), pgid);
    }

    let stdout_handle = std::thread::spawn(move || -> Vec<u8> {
        let mut buf = Vec::new();
        if let Some(mut pipe) = stdout_pipe {
            read_bounded_tail(&mut pipe, &mut buf);
        }
        buf
    });
    let stderr_handle = std::thread::spawn(move || -> Vec<u8> {
        let mut buf = Vec::new();
        if let Some(mut pipe) = stderr_pipe {
            read_bounded_tail(&mut pipe, &mut buf);
        }
        buf
    });

    let mut cancel_sent = false;
    let mut force_sent = false;
    let mut cancel_seen_at: Option<std::time::Instant> = None;
    let force_after = Duration::from_secs(10);

    let status = loop {
        if let Some(exec_id) = execution_id {
            if execution::is_cancel_requested(exec_id) {
                if cancel_seen_at.is_none() {
                    cancel_seen_at = Some(std::time::Instant::now());
                }
                if !cancel_sent {
                    let _ = signal_owned_process(&child, pgid, Signal::Term);
                    cancel_sent = true;
                }
                let force_now = execution::get_execution(exec_id)
                    .ok()
                    .flatten()
                    .and_then(|r| r.cancel_outcome.clone())
                    .map(|o| o == "force_requested")
                    .unwrap_or(false);
                if force_now && !force_sent {
                    let _ = signal_owned_process(&child, pgid, Signal::Kill);
                    force_sent = true;
                }
                if !force_sent
                    && cancel_seen_at
                        .map(|t| t.elapsed() >= force_after)
                        .unwrap_or(false)
                {
                    let _ = signal_owned_process(&child, pgid, Signal::Kill);
                    force_sent = true;
                }
            }
        }

        let maybe_status = {
            let mut guard = child.lock().unwrap();
            match guard.as_mut() {
                Some(c) => c.try_wait().map_err(|e| format!("wait failed: {e}"))?,
                None => None,
            }
        };
        if let Some(status) = maybe_status {
            break status;
        }
        std::thread::sleep(Duration::from_millis(200));
    };

    if let Some(exec_id) = execution_id {
        unregister_process(exec_id);
    }

    let stdout = stdout_handle.join().unwrap_or_default();
    let stderr = stderr_handle.join().unwrap_or_default();

    if let Some(exec_id) = execution_id {
        if execution::is_cancel_requested(exec_id) {
            let _ = execution::mark_cancel_confirmed(
                exec_id,
                if force_sent {
                    "force_killed"
                } else {
                    "cancelled"
                },
            );
            return Err("execution cancelled".to_string());
        }
    }

    // Reap: child already exited; drop handle.
    let _ = child.lock().unwrap().take();
    Ok(CommandOutput {
        status,
        stdout,
        stderr,
    })
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

    #[test]
    fn captured_output_keeps_only_the_tail() {
        let mut captured = Vec::new();
        let input = vec![b'a'; MAX_CAPTURED_OUTPUT + 7];
        read_bounded_tail(&mut input.as_slice(), &mut captured);
        assert_eq!(captured.len(), MAX_CAPTURED_OUTPUT);
        assert!(captured.iter().all(|b| *b == b'a'));
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
        std::env::set_var("ISSUE_FACTORY_DB_FILE", tmp.to_str().unwrap());

        let script = std::env::temp_dir().join(format!("factory_sleep_{}.sh", std::process::id()));
        std::fs::write(&script, "#!/bin/sh\nsleep 30\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        std::env::set_var("ISSUE_FACTORY_T3", script.to_str().unwrap());

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
        std::env::remove_var("ISSUE_FACTORY_T3");
        std::env::remove_var("ISSUE_FACTORY_DB_FILE");
    }
}
