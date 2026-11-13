use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum IssueStatus {
    #[default]
    Pending,
    Running,
    Failed,
    Completed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueState {
    pub status: IssueStatus,
    #[serde(default)]
    pub attempts: u32,
    #[serde(default)]
    pub last_error: Option<String>,
    #[serde(default)]
    pub thread_id: Option<String>,
    #[serde(default)]
    pub updated_at: u64,
}

impl Default for IssueState {
    fn default() -> Self {
        Self {
            status: IssueStatus::Pending,
            attempts: 0,
            last_error: None,
            thread_id: None,
            updated_at: now(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct State {
    #[serde(default)]
    pub issues: HashMap<String, IssueState>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub processed: Vec<String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub threads: HashMap<String, String>,
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn state_file() -> PathBuf {
    if let Ok(p) = std::env::var("ISSUE_FACTORY_STATE_FILE") {
        if !p.trim().is_empty() {
            return PathBuf::from(p);
        }
    }
    dirs::config_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap().join(".config"))
        .join("ailuracollective")
        .join("factory")
        .join("state.json")
}

pub fn load_state() -> HashMap<String, IssueState> {
    let path = state_file();
    if !path.exists() {
        return HashMap::new();
    }
    let data = match fs::read_to_string(&path) {
        Ok(d) => d,
        Err(_) => return HashMap::new(),
    };
    let state: State = match serde_json::from_str(&data) {
        Ok(s) => s,
        Err(_) => return HashMap::new(),
    };

    let mut issues = state.issues;
    if issues.is_empty() && (!state.processed.is_empty() || !state.threads.is_empty()) {
        for key in &state.processed {
            let thread_id = state.threads.get(key).cloned();
            issues.insert(
                key.clone(),
                IssueState {
                    status: IssueStatus::Completed,
                    attempts: 1,
                    thread_id,
                    updated_at: now(),
                    ..Default::default()
                },
            );
        }
        for (key, tid) in &state.threads {
            issues.entry(key.clone()).or_insert(IssueState {
                status: IssueStatus::Completed,
                attempts: 1,
                thread_id: Some(tid.clone()),
                updated_at: now(),
                ..Default::default()
            });
        }
    }

    for rec in issues.values_mut() {
        if rec.status == IssueStatus::Running {
            rec.status = IssueStatus::Failed;
            if rec.last_error.is_none() {
                rec.last_error = Some("interrupted by restart".to_string());
            }
            rec.updated_at = now();
        }
    }

    issues
}

pub fn save_state(issues: &HashMap<String, IssueState>) -> Result<()> {
    let state = State {
        issues: issues.clone(),
        processed: Vec::new(),
        threads: HashMap::new(),
    };
    let path = state_file();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    let data = serde_json::to_string_pretty(&state)?;
    fs::write(&tmp, data)?;
    fs::rename(&tmp, &path)?;
    Ok(())
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::sync::Mutex;
    pub static TEST_ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn roundtrip_state() {
        let _lock = TEST_ENV_LOCK.lock().unwrap();
        let mut m = HashMap::new();
        m.insert(
            "repo#1".to_string(),
            IssueState {
                status: IssueStatus::Completed,
                attempts: 2,
                thread_id: Some("t-1".to_string()),
                ..Default::default()
            },
        );
        let tmp = std::env::temp_dir().join("factory_state_test.json");
        std::env::set_var("ISSUE_FACTORY_STATE_FILE", tmp.to_str().unwrap());
        save_state(&m).unwrap();
        let loaded = load_state();
        assert_eq!(loaded.get("repo#1").unwrap().status, IssueStatus::Completed);
        assert_eq!(
            loaded.get("repo#1").unwrap().thread_id.as_deref(),
            Some("t-1")
        );
        std::fs::remove_file(&tmp).ok();
        std::env::remove_var("ISSUE_FACTORY_STATE_FILE");
    }

    #[test]
    fn migrate_legacy_processed() {
        let _lock = TEST_ENV_LOCK.lock().unwrap();
        let tmp = std::env::temp_dir().join("factory_state_legacy.json");
        std::env::set_var("ISSUE_FACTORY_STATE_FILE", tmp.to_str().unwrap());
        let legacy = serde_json::json!({
            "processed": ["r#1", "r#2"],
            "threads": {"r#1": "thread-a"}
        });
        std::fs::write(&tmp, legacy.to_string()).unwrap();
        let loaded = load_state();
        assert_eq!(loaded.get("r#1").unwrap().status, IssueStatus::Completed);
        assert_eq!(
            loaded.get("r#1").unwrap().thread_id.as_deref(),
            Some("thread-a")
        );
        assert_eq!(loaded.get("r#2").unwrap().status, IssueStatus::Completed);
        std::fs::remove_file(&tmp).ok();
        std::env::remove_var("ISSUE_FACTORY_STATE_FILE");
    }

    #[test]
    fn reconcile_running_to_failed() {
        let _lock = TEST_ENV_LOCK.lock().unwrap();
        let tmp = std::env::temp_dir().join("factory_state_running.json");
        std::env::set_var("ISSUE_FACTORY_STATE_FILE", tmp.to_str().unwrap());
        let mut m = HashMap::new();
        m.insert(
            "r#1".to_string(),
            IssueState {
                status: IssueStatus::Running,
                attempts: 1,
                ..Default::default()
            },
        );
        save_state(&m).unwrap();
        let loaded = load_state();
        assert_eq!(loaded.get("r#1").unwrap().status, IssueStatus::Failed);
        std::fs::remove_file(&tmp).ok();
        std::env::remove_var("ISSUE_FACTORY_STATE_FILE");
    }
}
