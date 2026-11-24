use anyhow::{Result, bail};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::state;

const LOG_DIR_NAME: &str = "logs";
const DEFAULT_FOLLOW_POLL_MS: u64 = 200;

pub fn logs_dir() -> PathBuf {
    state::state_file()
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join(LOG_DIR_NAME)
}

pub fn log_path(execution_id: &str) -> PathBuf {
    logs_dir().join(format!("{execution_id}.log"))
}

pub fn is_safe_execution_id(execution_id: &str) -> bool {
    let mut chars = execution_id.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_alphanumeric() {
        return false;
    }
    execution_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
}

fn ensure_safe_execution_id(execution_id: &str) -> Result<()> {
    if !is_safe_execution_id(execution_id) {
        bail!("invalid execution id: {execution_id}");
    }
    Ok(())
}

pub fn ensure_logs_dir() -> Result<PathBuf> {
    let dir = logs_dir();
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogAvailability {
    Available,
    Empty,
    Unavailable,
}

#[derive(Debug)]
pub struct LogRead {
    pub availability: LogAvailability,
    pub lines: Vec<String>,
}

#[derive(Default)]
struct StreamBuffers {
    stdout: String,
    stderr: String,
}

pub struct ExecutionLogWriter {
    file: Mutex<File>,
    buffers: Mutex<StreamBuffers>,
}

impl ExecutionLogWriter {
    pub fn open(execution_id: &str) -> Result<Self> {
        ensure_safe_execution_id(execution_id)?;
        ensure_logs_dir()?;
        let path = log_path(execution_id);
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        Ok(Self {
            file: Mutex::new(file),
            buffers: Mutex::new(StreamBuffers::default()),
        })
    }

    pub fn write_chunk(&self, stream: &str, data: &[u8]) {
        if data.is_empty() {
            return;
        }
        let text = String::from_utf8_lossy(data);
        let redacted = redact_credentials(&text);

        let mut buffers = match self.buffers.lock() {
            Ok(b) => b,
            Err(_) => return,
        };
        let buf = match stream {
            "stderr" => &mut buffers.stderr,
            _ => &mut buffers.stdout,
        };
        buf.push_str(&redacted);

        let mut out = String::new();
        while let Some(pos) = buf.find('\n') {
            let mut line: String = buf.drain(..=pos).collect();
            if line.ends_with('\n') {
                line.pop();
            }
            if line.ends_with('\r') {
                line.pop();
            }
            out.push_str(stream);
            out.push_str(": ");
            out.push_str(&line);
            out.push('\n');
        }

        drop(buffers);
        if !out.is_empty()
            && let Ok(mut f) = self.file.lock()
        {
            let _ = f.write_all(out.as_bytes());
            let _ = f.flush();
        }
    }

    pub fn flush_partial(&self) {
        let mut buffers = match self.buffers.lock() {
            Ok(b) => b,
            Err(_) => return,
        };
        let mut pending: Vec<(&str, String)> = Vec::new();
        if !buffers.stdout.is_empty() {
            pending.push(("stdout", std::mem::take(&mut buffers.stdout)));
        }
        if !buffers.stderr.is_empty() {
            pending.push(("stderr", std::mem::take(&mut buffers.stderr)));
        }
        drop(buffers);
        if pending.is_empty() {
            return;
        }
        let mut out = String::new();
        for (stream, mut line) in pending {
            if line.ends_with('\r') {
                line.pop();
            }
            out.push_str(stream);
            out.push_str(": ");
            out.push_str(&line);
            out.push('\n');
        }
        if let Ok(mut f) = self.file.lock() {
            let _ = f.write_all(out.as_bytes());
            let _ = f.flush();
        }
    }
}

impl Drop for ExecutionLogWriter {
    fn drop(&mut self) {
        self.flush_partial();
    }
}

pub fn redact_credentials(text: &str) -> String {
    // Redact common secret patterns in log output. This is best-effort and
    // avoids exposing obvious credentials.
    let mut out = text.to_string();
    // Authorization: Bearer <token>
    let auth_bearer =
        regex::Regex::new(r"(?i)\bAuthorization\s*:\s*Bearer\s+[A-Za-z0-9\-._~+/]+=*").unwrap();
    out = auth_bearer
        .replace_all(&out, "Authorization: Bearer [REDACTED]")
        .to_string();
    // KEY=value / KEY: value patterns for sensitive names
    let re = regex::Regex::new(
        r#"(?i)\b(authorization|token|secret|password|passwd|api[_-]?key|private[_-]?key|access[_-]?key|client[_-]?secret|session[_-]?cookie|cookie|set-cookie)\b([=:]\s*)([^\s,;"']+)"#,
    )
    .unwrap();
    out = re
        .replace_all(&out, |caps: &regex::Captures| {
            format!("{}{}[REDACTED]", &caps[1], &caps[2])
        })
        .to_string();
    // GitHub tokens and common prefixes
    let gh = regex::Regex::new(r"\b(gh[pousr]_[A-Za-z0-9_]{20,}|github_pat_[A-Za-z0-9_]{20,})\b")
        .unwrap();
    out = gh.replace_all(&out, "[REDACTED]").to_string();
    let bearer = regex::Regex::new(r"(?i)\bBearer\s+[A-Za-z0-9\-._~+/]+=*").unwrap();
    out = bearer.replace_all(&out, "Bearer [REDACTED]").to_string();
    out
}

pub fn read_log(execution_id: &str, lines: Option<u32>) -> Result<LogRead> {
    ensure_safe_execution_id(execution_id)?;
    let path = log_path(execution_id);
    if !path.exists() {
        return Ok(LogRead {
            availability: LogAvailability::Unavailable,
            lines: Vec::new(),
        });
    }
    let file = File::open(&path)?;
    let reader = BufReader::new(file);
    let all_lines: Vec<String> = reader.lines().collect::<std::result::Result<Vec<_>, _>>()?;
    if all_lines.is_empty() {
        return Ok(LogRead {
            availability: LogAvailability::Empty,
            lines: Vec::new(),
        });
    }
    let selected = match lines {
        Some(n) => {
            let n = n as usize;
            if all_lines.len() > n {
                all_lines[all_lines.len() - n..].to_vec()
            } else {
                all_lines
            }
        }
        None => all_lines,
    };
    Ok(LogRead {
        availability: LogAvailability::Available,
        lines: selected,
    })
}

pub fn follow_log(
    execution_id: &str,
    lines: Option<u32>,
    mut out: impl Write,
    interrupt: Arc<AtomicBool>,
) -> Result<()> {
    ensure_safe_execution_id(execution_id)?;
    let path = log_path(execution_id);
    if !path.exists() {
        return Ok(());
    }

    let initial = read_log(execution_id, lines)?;
    let mut offset = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    for line in &initial.lines {
        writeln!(out, "{line}")?;
    }
    out.flush()?;

    loop {
        if interrupt.load(Ordering::Relaxed) {
            break;
        }
        std::thread::sleep(Duration::from_millis(DEFAULT_FOLLOW_POLL_MS));
        if interrupt.load(Ordering::Relaxed) {
            break;
        }
        let meta = match fs::metadata(&path) {
            Ok(m) => m,
            Err(_) => break,
        };
        let len = meta.len();
        if len < offset {
            offset = 0; // truncated/rotated
        }
        if len == offset {
            continue;
        }
        let mut file = match File::open(&path) {
            Ok(f) => f,
            Err(_) => break,
        };
        file.seek(SeekFrom::Start(offset))?;
        let mut buf = String::new();
        file.read_to_string(&mut buf)?;
        offset = len;
        for line in buf.split_inclusive('\n') {
            if line.ends_with('\n') {
                out.write_all(line.as_bytes())?;
            } else {
                // partial line, write and wait for completion on next poll
                out.write_all(line.as_bytes())?;
                out.write_all(b"\n")?;
            }
        }
        out.flush()?;
    }
    Ok(())
}

pub fn read_bounded_tail_with_log(
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
                if let Some(l) = log {
                    l.write_chunk(stream, data);
                }
                output.extend_from_slice(data);
                const MAX_CAPTURED_OUTPUT: usize = 1024 * 1024;
                if output.len() > MAX_CAPTURED_OUTPUT {
                    let excess = output.len() - MAX_CAPTURED_OUTPUT;
                    output.drain(..excess);
                }
            }
        }
    }
}

pub fn follow_log_from_offset(
    execution_id: &str,
    mut out: impl Write,
    interrupt: Arc<AtomicBool>,
) -> Result<()> {
    ensure_safe_execution_id(execution_id)?;
    let path = log_path(execution_id);
    let mut offset = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);

    loop {
        if interrupt.load(Ordering::Relaxed) {
            break;
        }
        std::thread::sleep(Duration::from_millis(DEFAULT_FOLLOW_POLL_MS));
        if interrupt.load(Ordering::Relaxed) {
            break;
        }
        let meta = match fs::metadata(&path) {
            Ok(m) => m,
            Err(_) => {
                // If the file disappears, keep waiting; execution may still be starting.
                continue;
            }
        };
        let len = meta.len();
        if len < offset {
            offset = 0;
        }
        if len == offset {
            continue;
        }
        let mut file = match File::open(&path) {
            Ok(f) => f,
            Err(_) => continue,
        };
        if file.seek(SeekFrom::Start(offset)).is_err() {
            continue;
        }
        let mut buf = Vec::new();
        if file.read_to_end(&mut buf).is_err() {
            continue;
        }
        offset = offset.saturating_add(buf.len() as u64);
        if !buf.is_empty() {
            let _ = out.write_all(&buf);
            let _ = out.flush();
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::tests::TEST_ENV_LOCK;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    fn temp_db(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "factory_logs_{name}_{}_{}",
            std::process::id(),
            uuid::Uuid::now_v7().simple()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir.join("factory.db")
    }

    fn with_temp_db<T>(name: &str, f: impl FnOnce() -> T) -> T {
        let _lock = TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let db = temp_db(name);
        let prev = std::env::var("ISSUE_FACTORY_DB_FILE").ok();
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("ISSUE_FACTORY_DB_FILE", &db) };
        let result = f();
        match prev {
            Some(v) => unsafe { std::env::set_var("ISSUE_FACTORY_DB_FILE", v) },
            None => unsafe { std::env::remove_var("ISSUE_FACTORY_DB_FILE") },
        }
        let _ = fs::remove_file(&db);
        let _ = fs::remove_dir_all(db.parent().unwrap());
        result
    }

    #[test]
    fn read_log_unknown_file_is_unavailable() {
        with_temp_db("unavailable", || {
            let read = read_log("exec_missing", None).unwrap();
            assert_eq!(read.availability, LogAvailability::Unavailable);
            assert!(read.lines.is_empty());
        });
    }

    #[test]
    fn read_log_empty_file_is_empty() {
        with_temp_db("empty", || {
            let id = "exec_empty";
            ensure_logs_dir().unwrap();
            File::create(log_path(id)).unwrap();
            let read = read_log(id, None).unwrap();
            assert_eq!(read.availability, LogAvailability::Empty);
            assert!(read.lines.is_empty());
        });
    }

    #[test]
    fn read_log_respects_line_limit() {
        with_temp_db("limit", || {
            let id = "exec_limit";
            let writer = ExecutionLogWriter::open(id).unwrap();
            for i in 1..=5 {
                writer.write_chunk("stdout", format!("line-{i}\n").as_bytes());
            }
            drop(writer);
            let read = read_log(id, Some(2)).unwrap();
            assert_eq!(read.availability, LogAvailability::Available);
            assert_eq!(read.lines, vec!["stdout: line-4", "stdout: line-5"]);
        });
    }

    #[test]
    fn concurrent_executions_have_distinguishable_log_streams() {
        with_temp_db("concurrent", || {
            let a = "exec_a";
            let b = "exec_b";
            let wa = ExecutionLogWriter::open(a).unwrap();
            let wb = ExecutionLogWriter::open(b).unwrap();
            wa.write_chunk("stdout", b"from-a\n");
            wb.write_chunk("stdout", b"from-b\n");
            drop(wa);
            drop(wb);

            let ra = read_log(a, None).unwrap();
            let rb = read_log(b, None).unwrap();
            assert_eq!(ra.lines, vec!["stdout: from-a"]);
            assert_eq!(rb.lines, vec!["stdout: from-b"]);
            assert_ne!(log_path(a), log_path(b));
        });
    }

    #[test]
    fn follow_emits_subsequent_records_without_repeating_history() {
        with_temp_db("follow", || {
            let id = "exec_follow";
            let writer = ExecutionLogWriter::open(id).unwrap();
            writer.write_chunk("stdout", b"old\n");
            drop(writer);

            let interrupt = Arc::new(AtomicBool::new(false));
            let interrupt_for_thread = interrupt.clone();
            let id_for_thread = id.to_string();
            let handle = std::thread::spawn(move || {
                let mut out = Vec::new();
                follow_log_from_offset(&id_for_thread, &mut out, interrupt_for_thread).unwrap();
                out
            });

            std::thread::sleep(Duration::from_millis(250));
            let writer = ExecutionLogWriter::open(id).unwrap();
            writer.write_chunk("stdout", b"new\n");
            drop(writer);
            std::thread::sleep(Duration::from_millis(350));
            interrupt.store(true, Ordering::Relaxed);
            let out = handle.join().unwrap();
            let text = String::from_utf8(out).unwrap();
            assert!(text.contains("stdout: new"));
            assert!(!text.contains("stdout: old"));
        });
    }

    #[test]
    fn interrupting_follow_does_not_cancel_execution() {
        with_temp_db("interrupt", || {
            let id = "exec_interrupt";
            let writer = ExecutionLogWriter::open(id).unwrap();
            writer.write_chunk("stdout", b"line\n");
            drop(writer);

            let interrupt = Arc::new(AtomicBool::new(true));
            let mut out = Vec::new();
            follow_log_from_offset(id, &mut out, interrupt).unwrap();
            // Follow returned due to interrupt; no execution cancellation side effects exist here.
            assert!(out.is_empty());
        });
    }

    #[test]
    fn redact_credentials_masks_sensitive_values() {
        let input =
            "token=abc123 Authorization: Bearer secretvalue ghp_abcdefghijklmnopqrstuvwxyz123456";
        let redacted = redact_credentials(input);
        assert!(!redacted.contains("abc123"));
        assert!(!redacted.contains("secretvalue"));
        assert!(!redacted.contains("ghp_abcdefghijklmnopqrstuvwxyz123456"));
        assert!(redacted.contains("[REDACTED]"));
    }

    #[test]
    fn safe_execution_id_rejects_path_traversal() {
        assert!(is_safe_execution_id("exec_123abc"));
        assert!(!is_safe_execution_id("../evil"));
        assert!(!is_safe_execution_id("exec/../../etc/passwd"));
        assert!(!is_safe_execution_id(""));
    }
}
