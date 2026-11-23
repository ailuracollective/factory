use super::*;
use std::collections::HashMap;
use std::process::Command;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub(super) struct RunningProcess {
    pgid: u32,
    child: Arc<Mutex<Option<std::process::Child>>>,
}

pub(super) fn registry() -> &'static Mutex<HashMap<String, RunningProcess>> {
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
pub(super) enum Signal {
    Term,
    Kill,
}

#[cfg(unix)]
pub(super) fn signal_process_group(pgid: u32, sig: Signal) -> Result<(), String> {
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
pub(super) fn signal_process_group(pgid: u32, sig: Signal) -> Result<(), String> {
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
pub(super) fn signal_process_group(_pgid: u32, _sig: Signal) -> Result<(), String> {
    Err("process-tree cancellation is unsupported on this platform".to_string())
}

pub(super) fn signal_owned_process(
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
pub(super) fn configure_process_group(cmd: &mut Command) {
    use std::os::unix::process::CommandExt;
    cmd.process_group(0);
}

#[cfg(not(unix))]
pub(super) fn configure_process_group(_cmd: &mut Command) {}

pub(super) fn register_process(
    execution_id: &str,
    child: Arc<Mutex<Option<std::process::Child>>>,
    pgid: u32,
) {
    registry()
        .lock()
        .unwrap()
        .insert(execution_id.to_string(), RunningProcess { pgid, child });
}

pub(super) fn unregister_process(execution_id: &str) {
    registry().lock().unwrap().remove(execution_id);
}
