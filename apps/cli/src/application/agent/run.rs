use super::process::{Signal, signal_owned_process};
use super::*;
use crate::execution::logs::ExecutionLogWriter;
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

pub(super) struct CommandOutput {
    pub(super) status: std::process::ExitStatus,
    pub(super) stdout: Vec<u8>,
    pub(super) stderr: Vec<u8>,
}

pub(super) fn output_detail(stdout: &[u8], stderr: &[u8]) -> String {
    let stderr = String::from_utf8_lossy(stderr);
    let stdout = String::from_utf8_lossy(stdout);
    let detail = if stderr.trim().is_empty() {
        stdout.trim()
    } else {
        stderr.trim()
    };
    detail.chars().take(2000).collect()
}

pub(super) const MAX_CAPTURED_OUTPUT: usize = 1024 * 1024;

#[cfg(test)]
pub(super) fn read_bounded_tail(reader: &mut impl Read, output: &mut Vec<u8>) {
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

pub(super) fn read_bounded_tail_with_optional_log(
    reader: &mut impl Read,
    output: &mut Vec<u8>,
    log: Option<&Arc<ExecutionLogWriter>>,
    stream: &str,
) {
    let mut chunk = [0u8; 8192];
    loop {
        match reader.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let data = &chunk[..n];
                if let Some(writer) = log {
                    writer.write_chunk(stream, data);
                }
                output.extend_from_slice(data);
                if output.len() > MAX_CAPTURED_OUTPUT {
                    let excess = output.len() - MAX_CAPTURED_OUTPUT;
                    output.drain(..excess);
                }
            }
        }
    }
}

pub(super) fn run_supervised_command(
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

    let log_writer = execution_id.and_then(|id| match ExecutionLogWriter::open(id) {
        Ok(w) => Some(Arc::new(w)),
        Err(e) => {
            tracing::warn!("failed to open execution log for {id}: {e}");
            None
        }
    });

    let stdout_log = log_writer.clone();
    let stdout_handle = std::thread::spawn(move || -> Vec<u8> {
        let mut buf = Vec::new();
        if let Some(mut pipe) = stdout_pipe {
            read_bounded_tail_with_optional_log(&mut pipe, &mut buf, stdout_log.as_ref(), "stdout");
        }
        buf
    });
    let stderr_log = log_writer;
    let stderr_handle = std::thread::spawn(move || -> Vec<u8> {
        let mut buf = Vec::new();
        if let Some(mut pipe) = stderr_pipe {
            read_bounded_tail_with_optional_log(&mut pipe, &mut buf, stderr_log.as_ref(), "stderr");
        }
        buf
    });

    let mut cancel_sent = false;
    let mut force_sent = false;
    let mut cancel_seen_at: Option<std::time::Instant> = None;
    let force_after = Duration::from_secs(10);

    let status = loop {
        if let Some(exec_id) = execution_id
            && execution::is_cancel_requested(exec_id)
        {
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

    if let Some(exec_id) = execution_id
        && execution::is_cancel_requested(exec_id)
    {
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

    // Reap: child already exited; drop handle.
    let _ = child.lock().unwrap().take();
    Ok(CommandOutput {
        status,
        stdout,
        stderr,
    })
}
